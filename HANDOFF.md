# Devir notu: Rust köprüsü

Son güncelleme: 2026-10-15. Dal: `claude/repo-identification-status-84ct1u`.

## Durum

Python köprüsü (`browser_bridge.py`, `tray_app.py`, `status_window.py`, `clipboard_badge.py`) Rust ile
`rust/` klasöründe yeniden yazıldı. Python dosyaları değişmedi. Eklentide yalnızca simgeler eklendi (`extension/icons/`, manifest 2.3.0);
kod ve protokol aynı.

Yapılanlar:
- Tek süreçli Windows EXE (~463 KB, MinGW ile). Kısayollar `RegisterHotKey` ile; arayüz düz Win32 + GDI;
  HTTP sunucusu `std::net` ile. Bağımlılıklar: `windows-sys` 0.59, `serde_json`; derleme için `embed-resource`.
- HTTP protokolü birebir aynı (`/poll`, `/report`, `/status`, `/trace`, Bearer anahtar, CORS). Eklenti
  değişmeden çalışır; aynı `bridge-config.json` ve `dikte.log` kullanılır.
- 29 birim testi (`cargo test`; Linux'ta da çalışır). `test_bridge.py` senaryolarının hepsi taşındı.
- Siyah + neon arayüz: rozet (HAZIR teal, DİNLİYOR neon pembe + yanıp sönen nokta, YAZIYOR mavi,
  YAPIŞTIRILDI sarı, HATA kırmızı, BAĞLI DEĞİL gri) ve koyu durum penceresi. Parlama ucuz: sönük renkte
  kaydırılmış kopyalar; çizim yalnızca durum değişince.
- Wine + Xvfb altında uçtan uca denendi: kısayol → start → started → kısayol → stop → result → Notepad'e
  tek satır yapıştırma, Enter varyantı, iptal, ikinci kopya engeli.
- Python sürümünde bulunup Rust'ta düzeltilen hatalar (`rust/README.md` → "düzeltilen hatalar"):
  teslim edilmemiş `start`'ın kuyrukta kalması, hızlı iptal + yeniden başlatmada iptalin silinmesi,
  `SW_RESTORE` ile tam ekran terminalin küçülmesi, Enter gitmediğinde günlüğe "+ Enter." yazılması.

Bilerek düzeltilmeyen: `make_config.py`, `bridge-config.json` varken `extension/config.js` eksikse
üretmiyor (Python dosyalarına dokunulmaması istendiği için).

## Bu oturumda eklenenler

- Simgeler (`rust/src/icon.rs`), rozette yumuşak çizgi mikrofon, kısayollarda ayrı tuş kapakları.
- Sağ tık menüsünde (rozet ve tepsi) "yapıştır / yalnızca panoya kopyala" modu; `dikte-ayarlar.json`.
- Config hatasında aranan yol gösteriliyor.
- OpenAI API modu (sağ tık → "Dikte: OpenAI API"): winmm kayıt + WinHTTP, `gpt-4o-transcribe`.
  Anahtar `OPENAI_API_KEY` (süreç ya da kullanıcı ortamı) veya `dikte-ayarlar.json`. Mikrofon ve HTTPS
  yolu gerçek donanımda denendi (`cargo test -- --ignored`); gerçek anahtarla uçtan uca deneme kullanıcıda.
  Çoklu sağlayıcı bilerek eklenmedi (kullanıcı yalnızca OpenAI istedi).
- Eklenti 2.3.1: yeniden yüklemeden sonra eski içerik betiğinin "Extension context invalidated"
  hatası (`content.js` → `log`) yakalanıyor.

## 2026-10-16 eklenenler

- **Kısayol değiştirme** (`rust/src/hotkey.rs`): sağ tık → "Kısayolları değiştir" alt menüsü ya da durum
  penceresinde kısayol satırına tıklama → satır yeni birleşimi bekler (Esc vazgeçer). Yakalama sırasında genel
  kısayollar bırakılır. `dikte-ayarlar.json` → `hotkeys`. Ctrl+V, Ctrl+Shift+V (uygulamanın kendi yapıştırması),
  Alt+F4 vb. engellenir; değiştiricisiz tuş yalnızca F tuşları, Pause, ScrollLock için.
- **Mikrofon izni**: yeni `Phase::Permission`. Eklenti 2.4.0 Permissions API ile izni izler, `/poll?mic=` ve
  `{"event": "mic"}` ile bildirir; izin sorulurken `permission` olayı gönderir, ChatGPT sekmesini öne getirir ve
  izin verilene kadar `started` göndermez. Rozet artık başlatırken "BAŞLIYOR…", izin beklerken turuncu
  "İZİN BEKLENİYOR" gösterir (eskiden ikisi de "DİNLİYOR"du). İzin beklerken kısayol iptal eder.
- **Hata**: rozetin kendisinde tam metinle gösterilir (rozet metne göre genişler, en fazla 400 px; sığmayan
  "…" ile biter). Ayrı hata paneli 3.2.1'de kaldırıldı, hata iki kez görünüyordu. Uzun hatalar rozette daha
  uzun kalır (en fazla 12 sn). Rozete sol tık durum penceresini açar.
- **Rozet (3.2.x)**: 60×60 kare; yumuşak açılma/kapanma, sürükle-bırak ve kenara yapışma (`src/dock.rs`).
  Katmanlı pencere (`WS_EX_LAYERED` + `UpdateLayeredWindow`, köşeler renk anahtarıyla saydam): konum, boyut
  ve içerik tek çağrıda değişir; ayrı ayrı değişince geçişte titreme oluyordu.
- Gerçek Windows'ta görsel deneme yapılmadı (kullanıcının köprüsü açıktı).

## Yapılacaklar

1. ~~Push~~ ve ~~CI~~: dal GitHub'da, `rust` iş akışı yeşil. MSVC derlemesi yerelde de denendi (EXE 384 KB).
2. **Simgeler (yeni):** `rust/src/icon.rs` tek kaynak: EXE/tepsi `.ico` ve eklenti PNG'leri. Kullanıcı
   eklentiyi `chrome://extensions` → yenile ile yeniden yüklemeli. Geri bildirime göre ince ayar.
3. **Gerçek Windows'ta uçtan uca deneme** (kullanıcıyla): `rust/README.md` → "Mevcut kurulumda uçtan uca
   deneme". Özellikle: gerçek ChatGPT sekmesiyle dikte, Windows Terminal ve VS Code (eklenti paneli dahil) içinde Ctrl+V, tam ekran
   terminale odak geçişi, elle durdurma, tepsi menüsü, DPI ölçekleme (125/150%), Bahnschrift ile rozet
   yazılarının sığması, Windows 11'de başlık çubuğu rengi.
4. **Ölçüm raporu**: `rust/measure.ps1` ile Python EXE ve Rust EXE için açılış süresi, RAM (working set +
   private, tüm süreçler) ve EXE boyutu. Kullanıcının verdiği referans: Python ~62 MB RAM, ~18 MB EXE.
5. Kullanıcının geri bildirimine göre tema ayarı (renkler `rust/src/view.rs` → `palette`).

## Çalışma notları

- Derleme/test (Linux konteyner): `cd rust && cargo test`;
  Windows EXE: `rustup target add x86_64-pc-windows-gnu`, `apt-get install mingw-w64`,
  `cargo build --release --target x86_64-pc-windows-gnu`.
- Wine ile deneme: `apt-get install wine64 xvfb xdotool imagemagick`; `Xvfb :99 &`; EXE'nin yanına
  `{"token": "test-token"}` içeren `bridge-config.json` koy; `DISPLAY=:99 wine ChatGPT-Dikte.exe`.
  Sahte eklenti: `/poll` uzun yoklaması + `/report` (started/result) yapan küçük bir Python betiği.
  Kısayolların Wine'da tetiklenmesi için odakta bir Wine penceresi (ör. `wine notepad`) olmalı;
  `xdotool key ctrl+alt+space`. Wine'da Bahnschrift/Segoe UI yok, yazı tipleri farklı görünür.
- `pkill -f` ile Wine süreçlerini öldürmek kabuğu da öldürebiliyor; `setsid wineserver -k` kullan.
- Kod düzeni ve tasarım kararları: `rust/README.md`.
