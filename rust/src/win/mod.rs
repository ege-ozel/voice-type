//! Windows girişi: tek kopya kontrolü, yapılandırma, HTTP sunucusu, kısayol işçisi ve arayüz.
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
        message_box("Köprü başlatılamadı. EXE dosyasının yanında bridge-config.json bulunduğunu kontrol edin.", true);
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
