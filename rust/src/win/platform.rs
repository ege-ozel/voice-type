//! `Platform`'un Win32 uygulaması: pencere bilgisi, odak, pano, tuş gönderimi, bip ve günlük.
use std::fs::OpenOptions;
use std::io::Write;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Diagnostics::Debug::Beep;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};

use super::{audio, http};
use crate::bridge::Platform;
use crate::openai;
use crate::text::{Chord, Window};

const CF_UNICODETEXT: u32 = 13;

pub struct WinPlatform {
    log_path: Option<PathBuf>,
    log_lock: Mutex<()>,
    /// EXE klasörü: dikte-ayarlar.json burada.
    dir: PathBuf,
    recording: Mutex<Option<audio::Recording>>,
}

impl WinPlatform {
    pub fn new(dir: PathBuf, log_path: Option<PathBuf>) -> Self {
        WinPlatform { log_path, log_lock: Mutex::new(()), dir, recording: Mutex::new(None) }
    }

    fn api_key(&self) -> Result<String, String> {
        let env = std::env::var("OPENAI_API_KEY").ok().or_else(user_env_key);
        openai::api_key(env, &super::read_settings(&self.dir)).ok_or_else(|| {
            "OpenAI API anahtarı yok. PowerShell'de: setx OPENAI_API_KEY \"sk-...\" (ya da \
             dikte-ayarlar.json içine \"openai_api_key\")."
                .to_string()
        })
    }
}

/// `setx` ile sonradan eklenen anahtar süreç ortamına gelmez; kullanıcı ortamından (kayıt defteri) okunur.
fn user_env_key() -> Option<String> {
    let (key, name) = (wide_z("Environment"), wide_z("OPENAI_API_KEY"));
    let mut buf = vec![0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                     null_mut(), buf.as_mut_ptr() as *mut _, &mut size)
    };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn hwnd(h: isize) -> HWND {
    h as HWND
}

fn wide_buffer_text(buf: &[u16], len: usize) -> String {
    String::from_utf16_lossy(&buf[..len.min(buf.len())])
}

pub fn window_info(h: HWND) -> Window {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        let mut title = vec![0u16; 1024];
        let n = GetWindowTextW(h, title.as_mut_ptr(), title.len() as i32).max(0) as usize;
        let mut class = vec![0u16; 256];
        let c = GetClassNameW(h, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        let mut exe = String::new();
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !process.is_null() {
            let mut path = vec![0u16; 32768];
            let mut size = path.len() as u32;
            if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size) != 0 {
                let full = wide_buffer_text(&path, size as usize);
                exe = full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
            }
            CloseHandle(process);
        }
        Window { hwnd: h as isize, title: wide_buffer_text(&title, n), class: wide_buffer_text(&class, c), exe }
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let mut input: INPUT = unsafe { zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0,
                                      dwExtraInfo: 0 };
    input
}

fn copy_to_clipboard(text: &str) -> bool {
    // Windows panosu satır sonu olarak CRLF bekler; bazı uygulamalar yalın LF'yi satır saymaz.
    let wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(Some(0)).collect();
    let bytes = wide.len() * size_of::<u16>();
    unsafe {
        // Başka bir uygulama panoyu kısa süre tutuyor olabilir.
        let mut opened = false;
        for _ in 0..20 {
            if OpenClipboard(null_mut()) != 0 {
                opened = true;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        if !opened {
            return false;
        }
        let mut ok = false;
        if EmptyClipboard() != 0 {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if !memory.is_null() {
                let target = GlobalLock(memory);
                if !target.is_null() {
                    copy_nonoverlapping(wide.as_ptr() as *const u8, target as *mut u8, bytes);
                    GlobalUnlock(memory);
                    ok = !SetClipboardData(CF_UNICODETEXT, memory).is_null();
                }
                if !ok {
                    GlobalFree(memory);
                }
            }
        }
        CloseClipboard();
        ok
    }
}

/// Yerel tarih ve saat: ("YYYY-AA-GG", "SS:DD:ss").
pub fn local_time() -> (String, String) {
    let mut t = unsafe { zeroed() };
    unsafe { GetLocalTime(&mut t) };
    (format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay), format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond))
}

pub fn spawn_small(job: impl FnOnce() + Send + 'static) {
    let _ = thread::Builder::new().stack_size(64 * 1024).spawn(job);
}

impl Platform for WinPlatform {
    fn foreground_window(&self) -> Window {
        window_info(unsafe { GetForegroundWindow() })
    }

    fn foreground_hwnd(&self) -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    fn is_window(&self, h: isize) -> bool {
        h != 0 && unsafe { IsWindow(hwnd(h)) != 0 }
    }

    fn activate(&self, h: isize) {
        unsafe {
            // Yalnızca simge durumundaysa geri yükle. Python sürümü her zaman SW_RESTORE çağırıyordu;
            // bu, tam ekran (maximize) terminali küçültüyordu.
            if IsIconic(hwnd(h)) != 0 {
                ShowWindow(hwnd(h), SW_RESTORE);
            }
            SetForegroundWindow(hwnd(h));
        }
    }

    fn modifiers_down(&self) -> bool {
        [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0)
    }

    fn set_clipboard(&self, text: &str) -> Result<(), String> {
        if copy_to_clipboard(text) {
            Ok(())
        } else {
            Err("Pano kullanılamadı. Yeniden deneyin.".into())
        }
    }

    fn send_chord(&self, chord: Chord) -> Result<(), String> {
        let keys: &[VIRTUAL_KEY] = match chord {
            Chord::CtrlV => &[VK_CONTROL, b'V' as u16],
            Chord::CtrlShiftV => &[VK_CONTROL, VK_SHIFT, b'V' as u16],
            Chord::Enter => &[VK_RETURN],
        };
        let mut inputs: Vec<INPUT> = keys.iter().map(|&k| key(k, false)).collect();
        inputs.extend(keys.iter().rev().map(|&k| key(k, true)));
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            // Kısmi gönderimde değiştirici tuş basılı kalmasın; Enter'ı tekrar gönderme.
            let releases: Vec<INPUT> = keys.iter().rev().map(|&k| key(k, true)).collect();
            unsafe { SendInput(releases.len() as u32, releases.as_ptr(), size_of::<INPUT>() as i32) };
            return Err(if chord == Chord::Enter {
                "Metin yapıştırıldı ama Windows Enter tuşunun gönderimini tamamlayamadı."
            } else {
                "Windows yapıştırma tuşlarını gönderemedi. Metin panoda; Ctrl+V ile yapıştırın."
            }.into());
        }
        Ok(())
    }

    fn beep(&self, freq: u32, ms: u32) {
        spawn_small(move || unsafe {
            Beep(freq, ms);
        });
    }

    fn log(&self, line: &str) {
        let Some(path) = &self.log_path else { return };
        let _one_writer = self.log_lock.lock();
        let (date, time) = local_time();
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{date} {time} {line}");
        }
    }

    fn clock(&self) -> String {
        local_time().1
    }

    fn after(&self, delay: Duration, job: Box<dyn FnOnce() + Send>) {
        spawn_small(move || {
            thread::sleep(delay);
            job();
        });
    }

    fn sleep(&self, d: Duration) {
        thread::sleep(d);
    }

    fn record_start(&self) -> Result<(), String> {
        self.api_key()?;
        let recording = audio::start()?;
        *self.recording.lock().unwrap_or_else(|e| e.into_inner()) = Some(recording);
        Ok(())
    }

    fn record_stop(&self) -> Option<Vec<u8>> {
        let recording = self.recording.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        Some(openai::wav(&recording.finish(), openai::SAMPLE_RATE))
    }

    fn transcribe(&self, wav: &[u8]) -> Result<String, String> {
        let key = self.api_key()?;
        let model = openai::model(&super::read_settings(&self.dir));
        let headers = format!("Authorization: Bearer {key}\r\nContent-Type: {}\r\n", openai::content_type());
        let (status, body) = http::post(openai::HOST, openai::PATH, &headers, &openai::body(wav, &model))?;
        openai::parse(status, &body)
    }

    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        let _ = thread::Builder::new().stack_size(256 * 1024).spawn(job);
    }
}
