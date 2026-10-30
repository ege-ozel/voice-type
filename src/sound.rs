//! Bildirim sesi: ses seviyesi PCM örneklerine uygulanır; Windows ana sesi değişmez.
use serde_json::Value;
use std::sync::mpsc::{self, Sender};
use std::thread;

pub const DEFAULT_VOLUME: u8 = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Pack {
    #[default]
    Original,
    Mouth,
}

impl Pack {
    pub fn from_settings(settings: &Value) -> Self {
        match settings.get("sound_pack").and_then(Value::as_str) {
            Some("mouth") => Self::Mouth,
            _ => Self::Original,
        }
    }

    pub fn key(self) -> &'static str {
        match self { Self::Original => "original", Self::Mouth => "mouth" }
    }

    pub fn label(self) -> &'static str {
        match self { Self::Original => "Orijinal", Self::Mouth => "Ağız" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Listening,
    Error,
    Pasting,
    Transcribing,
    Cancelled,
    Busy,
}

/// Tek işçi FIFO çalar: kısa geçişler başka seslerin arkasına rastgele düşmez.
/// Gönderen arayüzü/dikteyi bekletmez; son gönderici bırakılınca işçi kapanır.
pub struct Player {
    sender: Sender<(Cue, Pack, u8)>,
}

impl Player {
    pub fn new(mut play: impl FnMut(Cue, Pack, u8) + Send + 'static) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new().name("dikte-sounds".into()).stack_size(128 * 1024).spawn(move || {
            for (cue, pack, volume) in receiver {
                play(cue, pack, volume);
            }
        })?;
        Ok(Self { sender })
    }

    pub fn enqueue(&self, cue: Cue, pack: Pack, volume: u8) -> std::io::Result<()> {
        self.sender.send((cue, pack, volume)).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Ses işçisi kapandı")
        })
    }
}

const LISTENING: &[u8] = include_bytes!("../assets/sounds/listening.wav");
const ERROR: &[u8] = include_bytes!("../assets/sounds/error.wav");
const PASTING: &[u8] = include_bytes!("../assets/sounds/pasting.wav");
const MOUTH_LISTENING: &[u8] = include_bytes!("../assets/sounds/mouth-listening.wav");
const MOUTH_ERROR: &[u8] = include_bytes!("../assets/sounds/mouth-error.wav");
const MOUTH_PASTING: &[u8] = include_bytes!("../assets/sounds/mouth-pasting.wav");
const PROCESSING: &[u8] = include_bytes!("../assets/sounds/processing.wav");

pub fn volume(settings: &Value) -> u8 {
    settings.get("sound_volume").and_then(Value::as_u64)
        .map(|v| v.min(100) as u8).unwrap_or(DEFAULT_VOLUME)
}

/// Gömülü WAV'lar import_sound.py ile standart 44 bayt başlıklı 16 bit PCM'e çevrilir.
/// Dosya okuması ve ses çözme yoktur; yalnızca örneklerin genliği ayarlanır.
pub fn render(cue: Cue, pack: Pack, volume: u8) -> Vec<u8> {
    let (listening, error, pasting) = match pack {
        Pack::Original => (LISTENING, ERROR, PASTING),
        Pack::Mouth => (MOUTH_LISTENING, MOUTH_ERROR, MOUTH_PASTING),
    };
    let source = match cue {
        Cue::Listening => listening,
        Cue::Transcribing => PROCESSING,
        Cue::Error | Cue::Cancelled | Cue::Busy => error,
        Cue::Pasting => pasting,
    };
    let mut wav = source.to_vec();
    let volume = volume.min(100) as i32;
    if volume < 100 {
        for sample in wav[44..].chunks_exact_mut(2) {
            let value = i16::from_le_bytes([sample[0], sample[1]]) as i32;
            sample.copy_from_slice(&((value * volume / 100) as i16).to_le_bytes());
        }
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_cues_queue_without_blocking_and_play_in_order() {
        use std::time::Duration;
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (played_tx, played_rx) = mpsc::channel();
        let player = Player::new(move |cue, pack, volume| {
            if cue == Cue::Listening {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            played_tx.send((cue, pack, volume)).unwrap();
        }).unwrap();
        player.enqueue(Cue::Listening, Pack::Original, 35).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Dinleme sesi hâlâ sürerken iki durum daha gelir; çağıran beklemez.
        player.enqueue(Cue::Transcribing, Pack::Mouth, 45).unwrap();
        player.enqueue(Cue::Pasting, Pack::Mouth, 55).unwrap();
        drop(player);
        assert!(played_rx.try_recv().is_err());
        release_tx.send(()).unwrap();
        for expected in [(Cue::Listening, Pack::Original, 35), (Cue::Transcribing, Pack::Mouth, 45),
                         (Cue::Pasting, Pack::Mouth, 55)] {
            assert_eq!(played_rx.recv_timeout(Duration::from_secs(2)).unwrap(), expected);
        }
        assert!(played_rx.recv_timeout(Duration::from_secs(2)).is_err());
    }

    #[test]
    fn missing_or_invalid_volume_defaults_and_large_values_are_clamped() {
        for value in [serde_json::json!({}), serde_json::json!({"sound_volume": -1}),
                      serde_json::json!({"sound_volume": "quiet"})] {
            assert_eq!(volume(&value), DEFAULT_VOLUME);
        }
        assert_eq!(volume(&serde_json::json!({"sound_volume": 200})), 100);
        assert_eq!(volume(&serde_json::json!({"sound_volume": 0})), 0);
    }

    #[test]
    fn pack_defaults_to_original_and_uses_stable_keys() {
        for value in [serde_json::json!({}), serde_json::json!({"sound_pack": "unknown"}),
                      serde_json::json!({"sound_pack": 1})] {
            assert_eq!(Pack::from_settings(&value), Pack::Original);
        }
        for pack in [Pack::Original, Pack::Mouth] {
            assert_eq!(Pack::from_settings(&serde_json::json!({"sound_pack": pack.key()})), pack);
        }
    }

    #[test]
    fn every_cue_uses_the_selected_pack_without_synthetic_beeps() {
        for pack in [Pack::Original, Pack::Mouth] {
            for cue in [Cue::Cancelled, Cue::Busy] {
                assert_eq!(render(cue, pack, 100), render(Cue::Error, pack, 100));
            }
            for cue in [Cue::Listening, Cue::Cancelled, Cue::Busy, Cue::Error, Cue::Pasting] {
                assert_ne!(render(cue, Pack::Original, 100), render(cue, Pack::Mouth, 100));
            }
        }
    }

    #[test]
    fn writing_cue_is_short_soft_distinct_and_respects_mute() {
        for pack in [Pack::Original, Pack::Mouth] {
            let wav = render(Cue::Transcribing, pack, 100);
            assert_ne!(wav, render(Cue::Listening, pack, 100));
            assert_ne!(wav, render(Cue::Pasting, pack, 100));
            assert_eq!(wav.len(), 44 + 23_040 * 4); // 480 ms, 48 kHz stereo PCM16
            let samples: Vec<i32> = wav[44..].chunks_exact(2)
                .map(|b| i16::from_le_bytes(b.try_into().unwrap()) as i32).collect();
            assert!(samples.iter().any(|s| s.abs() > 100));
            assert!(samples.iter().all(|s| s.abs() <= 6000));
            assert!(samples[..5_760 * 2].iter().all(|&s| s == 0)); // 120 ms açılış payı
            assert!(render(Cue::Transcribing, pack, 0)[44..].iter().all(|&b| b == 0));
            assert_eq!(&wav[..44], &render(Cue::Transcribing, pack, 50)[..44]);
        }
    }

    #[test]
    fn embedded_sounds_are_valid_distinct_stereo_pcm_and_scaled_without_header_changes() {
        assert_ne!(LISTENING, ERROR);
        assert_ne!(LISTENING, PASTING);
        assert_ne!(ERROR, PASTING);
        for (pack, assets) in [
            (Pack::Original, [LISTENING, ERROR, PASTING]),
            (Pack::Mouth, [MOUTH_LISTENING, MOUTH_ERROR, MOUTH_PASTING]),
        ] {
            for (cue, source) in [Cue::Listening, Cue::Error, Cue::Pasting].into_iter().zip(assets) {
                assert_eq!(&source[..4], b"RIFF");
                assert_eq!(&source[8..16], b"WAVEfmt ");
                assert_eq!(u32::from_le_bytes(source[16..20].try_into().unwrap()), 16);
                assert_eq!(u16::from_le_bytes(source[20..22].try_into().unwrap()), 1); // PCM
                assert_eq!(u16::from_le_bytes(source[22..24].try_into().unwrap()), 2); // stereo
                assert_eq!(u32::from_le_bytes(source[24..28].try_into().unwrap()), 48_000);
                assert_eq!(u16::from_le_bytes(source[34..36].try_into().unwrap()), 16);
                assert_eq!(&source[36..40], b"data");
                assert_eq!(u32::from_le_bytes(source[40..44].try_into().unwrap()) as usize, source.len() - 44);
                assert_eq!(source.len(), 44 + 48_000 * 2 * 2); // 1 saniye
                assert_eq!(render(cue, pack, 100), source);
                let half = render(cue, pack, 50);
                assert_eq!(&half[..44], &source[..44]);
                for (a, b) in source[44..].chunks_exact(2).zip(half[44..].chunks_exact(2)) {
                    let original = i16::from_le_bytes(a.try_into().unwrap()) as i32;
                    let scaled = i16::from_le_bytes(b.try_into().unwrap()) as i32;
                    assert!((original - scaled * 2).abs() <= 1);
                }
                assert!(render(cue, pack, 0)[44..].iter().all(|&b| b == 0));
            }
        }
    }
}
