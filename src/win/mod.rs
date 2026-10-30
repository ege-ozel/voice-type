//! Windows girişi: tek kopya kontrolü, yapılandırma, HTTP sunucusu, kısayol işçisi ve arayüz.
mod audio;
pub mod gfx;
mod http;
pub mod platform;
pub mod ui;

use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use serde_json::Value;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::CreateMutexW;

use crate::bridge::{Bridge, Engine, Platform};
use crate::hotkey::{Hotkeys, Slot};
use crate::server;
use platform::WinPlatform;
use ui::{message_box, wide, Action};

fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// bridge-config.json içindeki anahtar (dosya UTF-8 BOM ile başlayabilir).
pub fn read_token(dir: &Path) -> Option<String> {
    let raw = fs::read(dir.join("bridge-config.json")).ok()?;
    let raw = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&raw);
    let config: Value = serde_json::from_slice(raw).ok()?;
    config.get("token")?.as_str().filter(|t| !t.is_empty()).map(String::from)
}

const SETTINGS: &str = "dikte-ayarlar.json";

/// Kullanıcı ayarları (`copy_only`, `engine`, `hotkeys`, `sound_volume`, `sound_pack`, isteğe bağlı API ayarları).
/// Dosya yoksa ya da bozuksa boş nesne: yapıştır + ChatGPT sekmesi.
pub fn read_settings(dir: &Path) -> Value {
    fs::read(dir.join(SETTINGS))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&raw)).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Tek bir alanı yazar; dosyadaki diğer alanlar (ör. API anahtarı) korunur.
pub fn save_setting(dir: &Path, key: &str, value: Value) -> std::io::Result<()> {
    let mut settings = read_settings(dir);
    settings[key] = value;
    fs::write(dir.join(SETTINGS), serde_json::to_string_pretty(&settings).unwrap_or_default())
}

pub fn read_copy_only(dir: &Path) -> bool {
    read_settings(dir).get("copy_only").and_then(Value::as_bool).unwrap_or(false)
}

pub fn save_copy_only(dir: &Path, copy_only: bool) -> std::io::Result<()> {
    save_setting(dir, "copy_only", Value::Bool(copy_only))
}

pub fn read_engine(dir: &Path) -> Engine {
    match read_settings(dir).get("engine").and_then(Value::as_str) {
        Some("api") => Engine::Api,
        _ => Engine::Browser,
    }
}

pub fn save_engine(dir: &Path, engine: Engine) -> std::io::Result<()> {
    save_setting(dir, "engine", Value::from(if engine == Engine::Api { "api" } else { "browser" }))
}

pub fn read_hotkeys(dir: &Path) -> Hotkeys {
    Hotkeys::from_json(read_settings(dir).get("hotkeys"))
}

pub fn save_hotkeys(dir: &Path, hotkeys: &Hotkeys) -> std::io::Result<()> {
    save_setting(dir, "hotkeys", hotkeys.to_json())
}

/// Anahtar okunamayınca gösterilen mesaj: aranan tam yolu ve dosyanın yok mu bozuk mu olduğunu söyler.
fn config_error(dir: &Path) -> String {
    let path = dir.join("bridge-config.json");
    let problem = if path.exists() { "okunamadı ya da içinde \"token\" yok" } else { "bulunamadı" };
    format!(
        "Köprü başlatılamadı: bridge-config.json {problem}.\n\nAranan yer:\n{}\n\nEXE dosyasını \
         bridge-config.json ile aynı klasöre (ör. dist\\) koyup oradan açın.",
        path.display()
    )
}

pub fn run() -> i32 {
    let dir = app_dir();
    // Tek kopya: ikinci açılış sessizce değil, açıklamayla çıkar.
    unsafe {
        CreateMutexW(null(), 0, wide("Local\\ChatGPTDikte.Bridge").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            message_box("ChatGPT Dikte zaten açık (sağ alttaki bildirim alanında mikrofon simgesi).", false);
            return 0;
        }
    }
    let Some(token) = read_token(&dir) else {
        message_box(&config_error(&dir), true);
        return 1;
    };
    let listener = match server::bind(server::PORT) {
        Ok(listener) => listener,
        Err(_) => {
            message_box("Köprü zaten açık veya 47831 portu kullanımda.\n\nDiğer ChatGPT Dikte kopyasından çıkıp \
                         bu EXE dosyasını yeniden açın.", true);
            return 1;
        }
    };

    let bridge = Bridge::new(WinPlatform::new(dir.clone(), Some(dir.join("dikte.log"))));
    if read_copy_only(&dir) {
        bridge.set_copy_only(true);
    }
    if read_engine(&dir) == Engine::Api {
        bridge.set_engine(Engine::Api);
    }
    let hotkeys = read_hotkeys(&dir);
    bridge.set_hotkeys(hotkeys);
    {
        let bridge = Arc::clone(&bridge);
        let token: Arc<str> = Arc::from(token);
        thread::spawn(move || server::serve(listener, bridge, token));
    }
    // Kısayollar arayüz iş parçacığını asla bekletmesin: sırayla tek işçide çalışır.
    let (tx, rx) = mpsc::channel::<Action>();
    {
        let bridge = Arc::clone(&bridge);
        thread::spawn(move || {
            for action in rx {
                match action {
                    Action::Toggle => bridge.toggle(false),
                    Action::ToggleEnter => bridge.toggle(true),
                    Action::Cancel => bridge.cancel(),
                }
            }
        });
    }
    bridge.platform.log(&format!(
        "Hazır (Rust). Chrome'da chatgpt.com açık kalsın (arka planda olabilir). {} ile konuşun.",
        hotkeys.get(Slot::Toggle)
    ));
    match ui::run(Arc::clone(&bridge), tx) {
        Ok(()) => {
            bridge.platform.log("Çıkış.");
            0
        }
        Err(error) => {
            message_box(&error, true);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dikte-test-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_error_names_missing_file_and_path() {
        let dir = temp_dir("missing");
        let message = config_error(&dir);
        assert!(message.contains("bulunamadı"), "{message}");
        assert!(message.contains(&dir.join("bridge-config.json").display().to_string()), "{message}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_only_setting_round_trips_and_defaults_to_paste() {
        let dir = temp_dir("settings");
        assert!(!read_copy_only(&dir), "dosya yokken yapıştır");
        save_copy_only(&dir, true).unwrap();
        assert!(read_copy_only(&dir));
        save_copy_only(&dir, false).unwrap();
        assert!(!read_copy_only(&dir));
        fs::write(dir.join(SETTINGS), "bozuk").unwrap();
        assert!(!read_copy_only(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_one_setting_keeps_the_others() {
        let dir = temp_dir("merge");
        fs::write(dir.join(SETTINGS), r#"{"openai_api_key": "sk-test", "openai_model": "gpt-4o-mini-transcribe"}"#)
            .unwrap();
        save_copy_only(&dir, true).unwrap();
        save_engine(&dir, Engine::Api).unwrap();
        let s = read_settings(&dir);
        assert_eq!(s["openai_api_key"], "sk-test");
        assert_eq!(s["openai_model"], "gpt-4o-mini-transcribe");
        assert!(read_copy_only(&dir));
        assert_eq!(read_engine(&dir), Engine::Api);
        save_engine(&dir, Engine::Browser).unwrap();
        assert_eq!(read_engine(&dir), Engine::Browser);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hotkeys_round_trip_through_settings() {
        let dir = temp_dir("hotkeys");
        assert_eq!(read_hotkeys(&dir), Hotkeys::default());
        save_copy_only(&dir, true).unwrap();
        let keys = Hotkeys::default()
            .with(Slot::Cancel, crate::hotkey::Hotkey::parse("Ctrl+Alt+Q").unwrap())
            .unwrap();
        save_hotkeys(&dir, &keys).unwrap();
        assert_eq!(read_hotkeys(&dir), keys);
        assert!(read_copy_only(&dir), "diğer ayarlar korunur");
        assert_eq!(read_settings(&dir)["hotkeys"]["cancel"], "Ctrl+Alt+Q");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sound_volume_survives_restart_and_preserves_other_settings() {
        let dir = temp_dir("volume");
        save_copy_only(&dir, true).unwrap();
        let platform = WinPlatform::new(dir.clone(), None);
        assert_eq!(platform.sound_volume(), crate::sound::DEFAULT_VOLUME);
        for volume in [35, 0, 100] {
            platform.set_sound_volume(volume).unwrap();
            assert_eq!(platform.sound_volume(), volume);
            let restarted = WinPlatform::new(dir.clone(), None);
            assert_eq!(restarted.sound_volume(), volume);
            assert!(read_copy_only(&dir));
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_volume_save_keeps_previous_level() {
        let dir = temp_dir("volume-failure");
        let platform = WinPlatform::new(dir.clone(), None);
        fs::create_dir(dir.join(SETTINGS)).unwrap();
        assert!(platform.set_sound_volume(25).is_err());
        assert_eq!(platform.sound_volume(), crate::sound::DEFAULT_VOLUME);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sound_pack_switch_survives_restart_and_keeps_volume_and_other_settings() {
        use crate::sound::Pack;
        let dir = temp_dir("sound-pack");
        save_copy_only(&dir, true).unwrap();
        let platform = WinPlatform::new(dir.clone(), None);
        assert_eq!(platform.sound_pack(), Pack::Original);
        platform.set_sound_volume(35).unwrap();
        for pack in [Pack::Mouth, Pack::Original, Pack::Mouth] {
            platform.set_sound_pack(pack).unwrap();
            assert_eq!(platform.sound_pack(), pack);
            let restarted = WinPlatform::new(dir.clone(), None);
            assert_eq!(restarted.sound_pack(), pack);
            assert_eq!(restarted.sound_volume(), 35);
            assert!(read_copy_only(&dir));
        }
        platform.set_sound_volume(0).unwrap();
        assert_eq!(WinPlatform::new(dir.clone(), None).sound_pack(), Pack::Mouth);
        fs::remove_file(dir.join(SETTINGS)).unwrap();
        fs::create_dir(dir.join(SETTINGS)).unwrap();
        assert!(platform.set_sound_pack(Pack::Original).is_err());
        assert_eq!(platform.sound_pack(), Pack::Mouth);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn config_error_tells_invalid_file_apart() {
        let dir = temp_dir("invalid");
        fs::write(dir.join("bridge-config.json"), "{}").unwrap();
        assert_eq!(read_token(&dir), None);
        assert!(config_error(&dir).contains("\"token\" yok"));
        fs::write(dir.join("bridge-config.json"), "\u{feff}{\"token\": \"abc\"}").unwrap();
        assert_eq!(read_token(&dir).as_deref(), Some("abc"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Ses çıkışını sıfır PCM ile dener; açık uygulamayı veya kısayollarını kullanmaz.
    #[test]
    #[ignore]
    fn sound_queue_plays_silent_waveforms_in_order() {
        use crate::sound::{Cue, Pack, Player};
        use std::time::{Duration, Instant};
        use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
        let (tx, rx) = mpsc::channel();
        let player = Player::new(move |cue, pack, _volume| {
            let wav = crate::sound::render(cue, pack, 0);
            let start = Instant::now();
            let ok = unsafe {
                PlaySoundW(wav.as_ptr() as _, std::ptr::null_mut(), SND_MEMORY | SND_NODEFAULT | SND_SYNC)
            };
            tx.send((cue, ok, start.elapsed())).unwrap();
        }).unwrap();
        let cues = [Cue::Listening, Cue::Transcribing, Cue::Pasting];
        for cue in cues { player.enqueue(cue, Pack::Mouth, 100).unwrap(); }
        drop(player);
        for cue in cues {
            let (played, ok, duration) = rx.recv_timeout(Duration::from_secs(8)).unwrap();
            assert_eq!(played, cue);
            assert_ne!(ok, 0, "Windows çalmayı reddetti: {cue:?}");
            let minimum = if cue == Cue::Transcribing { 400 } else { 900 };
            assert!(duration >= Duration::from_millis(minimum), "Ses erken kesildi: {cue:?}, {duration:?}");
        }
    }

    /// Gerçek mikrofon gerekir: `cargo test -- --ignored microphone`.
    #[test]
    #[ignore]
    fn microphone_records_about_one_second() {
        let recording = audio::start().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let pcm = recording.finish();
        let seconds = pcm.len() as f32 / (crate::openai::SAMPLE_RATE * 2) as f32;
        assert!((0.8..1.5).contains(&seconds), "{seconds} sn");
    }

    /// İnternet gerekir; geçersiz anahtarla TLS + HTTP yolunu dener: `cargo test -- --ignored openai`.
    #[test]
    #[ignore]
    fn openai_rejects_invalid_key_over_https() {
        let wav = crate::openai::wav(&[0; 32_000], crate::openai::SAMPLE_RATE);
        let headers = format!("Authorization: Bearer sk-gecersiz
Content-Type: {}
",
                              crate::openai::content_type());
        let (status, body) = http::post(crate::openai::HOST, crate::openai::PATH, &headers,
                                        &crate::openai::body(&wav, crate::openai::DEFAULT_MODEL)).unwrap();
        assert_eq!(status, 401);
        assert_eq!(crate::openai::parse(status, &body), Err("OpenAI API anahtarı geçersiz.".into()));
    }
}
