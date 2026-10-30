//! Windows dikte uygulaması: OpenAI API veya ChatGPT sekmesinden alınan metni odaktaki pencereye aktarır.
//! Varsayılan başlat/bitir kısayolu Ctrl+Alt+Space; isteğe bağlı eklenti 127.0.0.1:47831'e bağlanır.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod bridge;
#[cfg_attr(not(windows), allow(dead_code))]
mod dock;
mod hotkey;
mod icon;
mod openai;
mod server;
mod sound;
mod text;
mod view;
#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    std::process::exit(win::run());
    #[cfg(not(windows))]
    {
        eprintln!("ChatGPT-Dikte yalnızca Windows'ta çalışır; burada `cargo test` ile çekirdek test edilir.");
        std::process::exit(1);
    }
}
