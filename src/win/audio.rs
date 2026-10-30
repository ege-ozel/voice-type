//! API modu için mikrofon kaydı (winmm waveIn): 16 kHz, 16 bit, mono. Kayıt yalnızca dikte sürerken
//! açıktır; boştayken ne arabellek ne iş parçacığı vardır.
use std::mem::{size_of, zeroed};
use std::ptr::{addr_of, null, null_mut};
use std::sync::atomic::{fence, AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Media::Audio::{
    waveInAddBuffer, waveInClose, waveInOpen, waveInPrepareHeader, waveInReset, waveInStart, waveInUnprepareHeader,
    CALLBACK_EVENT, HWAVEIN, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::openai::SAMPLE_RATE;

/// 4 x 0,2 sn arabellek: sürücü birini doldururken diğerleri hazır bekler.
const BUFFERS: usize = 4;
const BUFFER_BYTES: usize = SAMPLE_RATE as usize * 2 / 5;

pub struct Recording {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Vec<u8>>,
}

/// Mikrofonu açar; açılamazsa kullanıcıya gösterilecek hata.
pub fn start() -> Result<Recording, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || unsafe { record(&flag, tx) })
        .map_err(|_| "Kayıt başlatılamadı.".to_string())?;
    match rx.recv() {
        Ok(Ok(())) => Ok(Recording { stop, thread }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => Err("Mikrofon açılamadı.".into()),
    }
}

impl Recording {
    /// Kaydı bitirir; ham PCM (16 bit mono).
    pub fn finish(self) -> Vec<u8> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.join().unwrap_or_default()
    }
}

fn open_error(code: u32) -> String {
    match code {
        2 => "Mikrofon bulunamadı.".into(),
        4 => "Mikrofon başka bir uygulama tarafından kullanılıyor.".into(),
        _ => format!(
            "Mikrofon açılamadı (kod {code}). Ayarlar → Gizlilik ve güvenlik → Mikrofon: masaüstü \
             uygulamalarının erişimine izin verin."
        ),
    }
}

unsafe fn record(stop: &AtomicBool, ready: Sender<Result<(), String>>) -> Vec<u8> {
    let event = CreateEventW(null(), 0, 0, null());
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let mut wave: HWAVEIN = null_mut();
    let code = waveInOpen(&mut wave, WAVE_MAPPER, &format, event as usize, 0, CALLBACK_EVENT);
    if code != 0 {
        CloseHandle(event);
        let _ = ready.send(Err(open_error(code)));
        return Vec::new();
    }

    let size = size_of::<WAVEHDR>() as u32;
    let mut data = vec![vec![0u8; BUFFER_BYTES]; BUFFERS];
    let mut headers: Vec<WAVEHDR> = data
        .iter_mut()
        .map(|d| {
            let mut h: WAVEHDR = zeroed();
            h.lpData = d.as_mut_ptr();
            h.dwBufferLength = BUFFER_BYTES as u32;
            h
        })
        .collect();
    for h in headers.iter_mut() {
        waveInPrepareHeader(wave, h, size);
        waveInAddBuffer(wave, h, size);
    }
    waveInStart(wave);
    let _ = ready.send(Ok(()));

    // Arabellekler sırayla dolar ve aynı sırayla geri eklenir; `next` sıradakini gösterir.
    let mut pcm = Vec::new();
    let mut next = 0;
    loop {
        let stopping = stop.load(Ordering::SeqCst);
        if stopping {
            waveInReset(wave); // bekleyenlerin hepsi (yarım dolanlar dahil) "bitti" işaretlenir
        }
        for _ in 0..BUFFERS {
            let h = &mut headers[next];
            // Alanları sürücü doldurur; WAVEHDR 1 bayt hizalı (packed), referans değil ham okuma.
            fence(Ordering::SeqCst);
            if addr_of!(h.dwFlags).read_unaligned() & WHDR_DONE == 0 {
                break;
            }
            let recorded = (addr_of!(h.dwBytesRecorded).read_unaligned() as usize).min(BUFFER_BYTES);
            pcm.extend_from_slice(&data[next][..recorded]);
            h.dwBytesRecorded = 0;
            h.dwFlags &= !WHDR_DONE;
            if !stopping {
                waveInAddBuffer(wave, h, size);
            }
            next = (next + 1) % BUFFERS;
        }
        if stopping {
            break;
        }
        WaitForSingleObject(event, 100);
    }
    for h in headers.iter_mut() {
        waveInUnprepareHeader(wave, h, size);
    }
    waveInClose(wave);
    CloseHandle(event);
    pcm
}
