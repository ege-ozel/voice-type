//! ChatGPT dikte köprüsü (Rust): Ctrl+Alt+Space ile ChatGPT web diktesini arka planda başlatır,
//! sonucu odaktaki pencereye yapıştırır. Chrome eklentisiyle 127.0.0.1:47831 üzerinden konuşur.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod bridge;
#[cfg(test)]
mod icon;
mod server;
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
