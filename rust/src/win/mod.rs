//! Windows girişi: tek kopya kontrolü, yapılandırma, HTTP sunucusu, kısayol işçisi ve arayüz.
pub mod gfx;
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

use crate::bridge::{Bridge, Platform};
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
            message_box("Köprü zaten açık veya 47831 portu kullanımda.\n\nEski ChatGPT-Dikte.exe ya da start.bat \
                         yardımcı penceresi açıksa kapatıp bu EXE dosyasını yeniden açın.", true);
            return 1;
        }
    };

    let bridge = Bridge::new(WinPlatform::new(Some(dir.join("dikte.log"))));
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
    bridge.platform.log(
        "Hazır (Rust). Chrome'da chatgpt.com açık kalsın (arka planda olabilir). \
         Ctrl+Alt+Space başlat/bitir, Ctrl+Alt+Enter bitir+Enter, Ctrl+Alt+Backspace iptal.",
    );
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
    fn config_error_tells_invalid_file_apart() {
        let dir = temp_dir("invalid");
        fs::write(dir.join("bridge-config.json"), "{}").unwrap();
        assert_eq!(read_token(&dir), None);
        assert!(config_error(&dir).contains("\"token\" yok"));
        fs::write(dir.join("bridge-config.json"), "\u{feff}{\"token\": \"abc\"}").unwrap();
        assert_eq!(read_token(&dir).as_deref(), Some("abc"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
