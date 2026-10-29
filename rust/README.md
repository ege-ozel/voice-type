# ChatGPT Dikte — Rust sürümü

`browser_bridge.py`, `tray_app.py`, `status_window.py` ve `clipboard_badge.py` dosyalarının yerini alan tek
dosyalık Windows uygulaması. Chrome eklentisi (`extension/`) değişmedi; HTTP protokolü (`/poll`, `/report`,
`/status`, `/trace`, 127.0.0.1:47831, `Authorization: Bearer`) birebir aynı olduğu için kurulu eklenti
olduğu gibi çalışır. Aynı `bridge-config.json` dosyasını kullanır ve günlüğü aynı biçimde EXE'nin yanındaki
`dikte.log` dosyasına yazar.

| | Python (PyInstaller) | Rust |
|---|---|---|
| EXE | ~18 MB | ~0.38 MB (MSVC), ~0.45 MB (MinGW) |
| Süreç | 2 (açıcı + asıl) | 1 |
| Kısayol | `keyboard` (düşük seviyeli klavye kancası) | `RegisterHotKey` |
| Arayüz | Tk + pystray + Pillow | Düz Win32 + GDI |
| Bağımlılık | Python, keyboard, pyperclip, pystray, Pillow | `windows-sys`, `serde_json` |

RAM ve açılış süresini kendi makinenizde `measure.ps1` ile ölçün (aşağıda).

## Derleme

Yerel dağıtımda güncel EXE `dist/ChatGPT-Dikte-v3.1.5.exe`, sabit başlatıcı `dist/Baslat.bat`.
Önceki EXE'ler `dist/arsiv/` altında tutulur. Adlandırma `ChatGPT-Dikte-vANA.YENİLİK.DÜZELTME.exe`;
`bridge-config.json` ve `dikte-ayarlar.json` güncel EXE'nin yanında kalır.

Windows'ta ([rustup](https://rustup.rs) ve Visual Studio Build Tools ile):

```powershell
cd rust
cargo test
cargo build --release      # -> rust\target\release\ChatGPT-Dikte.exe
```

GitHub'da `rust` iş akışı her push'ta Windows üzerinde testleri çalıştırır ve EXE'yi
`ChatGPT-Dikte-exe` adlı yapıt (artifact) olarak yükler.

Linux'tan çapraz derleme: `rustup target add x86_64-pc-windows-gnu`, `mingw-w64` paketi, sonra
`cargo build --release --target x86_64-pc-windows-gnu`.

## Mevcut kurulumda uçtan uca deneme

1. Yeni EXE'yi `dist\` klasörüne, `bridge-config.json` dosyasının yanına koyun (ör.
   `dist\ChatGPT-Dikte-rust.exe`). Anahtar aynı kalır, eklentiye dokunmak gerekmez.
2. Eskisini kapatın: tepsideki mikrofon simgesine sağ tıklayın → **Çıkış** (ya da
   `Stop-Process -Name ChatGPT-Dikte`). `start.bat` penceresi açıksa Ctrl+C ile kapatın.
   İkisi aynı portu kullandığı için aynı anda çalışamaz; yenisi "port kullanımda" der.
3. Yenisini başlatın ve günlüğü izleyin:

   ```powershell
   Start-Process dist\ChatGPT-Dikte-rust.exe
   Get-Content dist\dikte.log -Wait -Tail 20 -Encoding UTF8
   ```

   Hemen `Hazır (Rust)…`, en geç bir dakika içinde `sekme bağlandı: …` görünmeli; sağ alttaki rozet
   **HAZIR** (yeşil) olur.
4. Denenecekler:
   - Terminalde Ctrl+Alt+Space → yüksek bip, rozet **DİNLİYOR**; konuşun; Ctrl+Alt+Space → metin terminale
     yapıştırılır, rozet **YAPIŞTIRILDI** (4 sn).
   - Ctrl+Alt+Enter ile bitirme: yapıştırır ve Enter gönderir.
   - Ctrl+Alt+Backspace: iptal (bip, ChatGPT'de dikte iptal edilir).
   - Konuşurken başka bir pencereye geçip bitirin: metin o pencereye gider.
   - ChatGPT sekmesinde kare (durdur) düğmesine elle basın: metin dikteyi başlattığınız pencereye gider.
   - Tam ekran (maximize) bir terminalde, başka pencereye geçip bitirin: terminal küçülmemeli.
   - Tepsi simgesi: çift tık durum penceresini açar; pencereyi kapatmak uygulamayı kapatmaz.
   - EXE'yi ikinci kez açın: "zaten açık" mesajı çıkmalı.
   - Rozete ya da tepsi simgesine sağ tıklayın → **Bitirince yalnızca panoya kopyala**: dikte bitince
     rozet **KOPYALANDI** der, hiçbir tuş gönderilmez; Ctrl+V ile istediğiniz yere yapıştırın. Seçim
     EXE'nin yanındaki `dikte-ayarlar.json` dosyasında saklanır.

## OpenAI API modu (isteğe bağlı, ücretli)

Rozete ya da tepsi simgesine sağ tıklayıp **Dikte: OpenAI API** seçilince ChatGPT sekmesi ve eklenti
kullanılmaz: kısayol mikrofonu doğrudan kaydeder (16 kHz mono), bitirince ses OpenAI'nin
`/v1/audio/transcriptions` uç noktasına (`gpt-4o-transcribe`) gider, metin her zamanki gibi yapıştırılır
ya da panoya kopyalanır. ChatGPT'nin web arayüzünü otomatikleştirmediği için OpenAI kullanım koşullarıyla
uyumludur; dakika başına ücretlendirilir.

Kurulum: https://platform.openai.com/api-keys adresinden anahtar alın, PowerShell'de

```powershell
setx OPENAI_API_KEY "sk-..."
```

Uygulamayı yeniden başlatmak gerekmez (kullanıcı ortamı her istekte okunur). Alternatif olarak EXE'nin
yanındaki `dikte-ayarlar.json` dosyasına `"openai_api_key": "sk-..."` yazılabilir. Daha ucuz model için
aynı dosyaya `"openai_model": "gpt-4o-mini-transcribe"`.

Ek kütüphane yok: kayıt winmm (`waveIn`), HTTPS Windows'un WinHTTP'si ile. EXE ~66 KB büyüdü; kayıt
arabellekleri ve iş parçacığı yalnızca dikte sürerken vardır. En uzun kayıt 10 dakika.
Donanım testleri: `cargo test -- --ignored` (1 sn mikrofon kaydı + geçersiz anahtarla OpenAI'ye istek).

## Ölçüm

Köprü kapalıyken, iki EXE'yi sırayla ölçün:

```powershell
powershell -ExecutionPolicy Bypass -File rust\measure.ps1 -Exe dist\ChatGPT-Dikte.exe
powershell -ExecutionPolicy Bypass -File rust\measure.ps1 -Exe dist\ChatGPT-Dikte-rust.exe
```

Betik, uygulamanın `/status` isteğine yanıt verdiği ana kadar geçen süreyi, 10 saniye sonraki working set
ve private bellek toplamını (tüm süreçler) ve EXE boyutunu yazar, sonra süreci kapatır.

## Görünüm

Siyah zemin + soluk (mat) vurgu (3.2.4): HAZIR mavimsi teal, DİNLİYOR gül pembesi (yanıp sönen kayıt noktasıyla), YAZIYOR
mavi, YAPIŞTIRILDI sarı, HATA kırmızı, BAĞLI DEĞİL gri. Parlama ucuz yoldan yapılır: metin ve
çerçeve, zemine karıştırılmış sönük renklerle birkaç piksel kaydırılarak altına çizilir. Alfa karıştırma,
katmanlı pencere ya da sürekli animasyon döngüsü yoktur; çizim yalnızca durum değişince yapılır (dinlerken
kayıt noktası için saniyede ~1,5 kez). Rozet normalde 60×60'lık karedir; YAPIŞTIRILDI / KOPYALANDI / HATA /
İZİN BEKLENİYOR'da ve fare üstündeyken ~190 ms'lik yavaşlayan geçişle açılır (yalnızca geçiş sürerken ~60 kare/sn
zamanlayıcı çalışır). Sürüklenip bırakılınca en yakın çalışma alanı kenarına 12 px boşlukla oturur
(`src/dock.rs`, DeskTuk'taki kenar rayı); yer `dikte-ayarlar.json` → `badge` içinde saklanır. Rozetteki mikrofon, uygulama simgesindeki çizimin içi boş hâlidir (`icon::glyph`, yumuşak kenarlı, durum
renginde). Durum penceresinde kısayolların her tuşu ayrı kapak olarak çizilir. Başlıklar Bahnschrift'tir (Windows 10+); yoksa Windows en yakın yazı
tipini seçer. Windows 11'de durum penceresinin başlık çubuğu ve kenarlığı da temaya uyar.

## Kod düzeni

- `src/bridge.rs`: durum makinesi (aşamalar, oturum, owner, komut kuyruğu, zaman aşımları, yapıştırma
  adımları). Windows'a dokunan her şey `Platform` arkasında, bu yüzden her yerde test edilir.
- `src/server.rs`: std::net ile HTTP sunucusu, yetkilendirme, CORS.
- `src/text.rs`: metin temizleme (terminalde tek satır, başka yerde satırlar korunur), pencereye göre Ctrl+V / Ctrl+Shift+V.
- `src/openai.rs`: WAV, multipart gövde, yanıt/hata çözme, anahtar ve model seçimi.
- `src/view.rs`: rozet, tepsi ipucu ve durum penceresi metinleri.
- `tools/rgba_to_png.py`: README rozet görsellerini (`docs/badge/`) PNG'ye çevirir; görseller
  `BADGE_SHOTS=<klasör> cargo test badge_shots -- --ignored` ile uygulamanın çizim koduyla üretilir.
- `src/dock.rs`: rozetin kenara yapışma, genişleme yönü ve geçiş eğrisi (platformdan bağımsız, testli).
- `src/win/`: Win32 uygulaması (pano, SendInput, odak, bip, günlük, pencereler, tepsi; `audio.rs` mikrofon,
  `http.rs` WinHTTP); `gfx.rs` neon
  çizim yardımcıları.
- `src/icon.rs`: simge çizimi (koyu yuvarlak kare + neon teal çizgi mikrofon, SDF ile yumuşatılmış) ve
  bağımlılıksız PNG yazıcı. `build.rs` bunu EXE/tepsi simgesi olarak gömer; eklentinin
  `extension/icons/` PNG'leri de buradan üretilir (`UPDATE_ICONS=1 cargo test icon`; test, PNG'ler
  çizimle aynı değilse düşer).

## Python sürümüne göre düzeltilen hatalar

- Hiçbir sekmeye ulaşmamış "başlat" komutu, zaman aşımı veya iptalden sonra kuyrukta kalıyordu. Sekme
  geri gelince mikrofon boşuna açılıyor, sekme de boşta "owner" oluyordu. O sekme sonra yenilenirse
  sonraki her başlatma "Dikte başlamadı" ile düşüyordu. Artık geri çekiliyor.
- İptalden hemen sonra yeniden başlatılınca teslim edilmemiş iptal siliniyordu. Sekme eski dikteyi
  "zaten sürüyor, bitir" diye bitirip iptal edilen metni yapıştırabiliyordu. İptaller artık korunuyor.
- Odak değiştirirken her zaman `SW_RESTORE` çağrılıyordu; tam ekran (maximize) terminal küçülüyordu.
  Artık yalnızca simge durumundaki pencere geri yükleniyor.
- Enter gönderilmediğinde de (hedef öne gelmediyse) günlüğe "+ Enter." yazılıyordu.
