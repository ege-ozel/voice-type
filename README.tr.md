[English](README.md) · [Türkçe](README.tr.md)

# voice-type — ChatGPT Dikte

Windows'ta odaktaki pencereye sesle yazma. Kısayola bas, konuş, bitir: metin yapıştırılır veya yalnızca
panoya kopyalanır. Tek bir yerel Rust EXE'sidir; ek çalışma ortamı kurulumu gerektirmez.

## Kurulum

1. [Sürümler](https://github.com/ege-ozel/voice-type/releases/latest) sayfasından Windows x64 ZIP'ini indir.
2. Yazılabilir bir klasöre çıkarıp `Start.bat` dosyasını çalıştır. İlk açılışta özel yerel anahtar üretilir.
3. Mikrofon rozetine sağ tıklayıp yöntemi seç:

| Yöntem | Kurulum |
|---|---|
| OpenAI API | Windows kullanıcı ortamına `OPENAI_API_KEY` ekle ve **Dikte: OpenAI API** seç. Ses OpenAI transkripsiyon API'sine gönderilir; API kullanımı ücretlidir. |
| ChatGPT sekmesi | `Setup.bat` çalıştır. Chrome'da `chrome://extensions` → Geliştirici modu → Paketlenmemiş öğe yükle ile paketin `extension` klasörünü seç. Giriş yapılmış bir chatgpt.com sekmesi açık kalsın ve mikrofon izni verilsin. Bu yöntem ChatGPT'nin web arayüzüne bağlıdır. |

API anahtarı için PowerShell'de `setx OPENAI_API_KEY "anahtarınız"` çalıştır. Uygulama Windows kullanıcı
ortamını yeniden okuyabildiği için anahtarı mevcut süreç de alabilir. Varsayılan model `gpt-4o-transcribe`;
EXE'nin yanındaki `dikte-ayarlar.json` dosyasına `openai_model` ekleyerek değiştirilebilir.

## Kullanım

| Varsayılan kısayol | İşlev |
|---|---|
| Ctrl+Alt+Space | Başlat / bitir ve yapıştır |
| Ctrl+Alt+Enter | Bitir, yapıştır ve Enter gönder |
| Ctrl+Alt+Backspace | İptal |

Bitirirken hedef pencere odakta olsun. Terminal dışındaki pencerelerde satır sonları korunur. Rozete
tıklayınca durum penceresi rozetin yapıştığı kenara göre yakınında açılır. Buradan kısayolları,
**Bitirince: Yapıştır / Yalnızca panoya kopyala** seçimini, **Orijinal / Ağız** ses paketini ve bildirim
sesi seviyesini (%0–100, %0 sessiz) değiştirebilirsin. Ayarlar EXE'nin yanında saklanır. Bitirme ve ses
paketi seçimleri sağ tık menüsünde de vardır. Windows genel sesi değişmez. Uygulamadan çıkmak için
tepsi menüsündeki **Çıkış** seçeneğini kullan.

Kapalı rozet %100 Windows ölçeğinde 64×64 px'tir. Sürükleyip istediğin kenara yapıştırabilirsin;
yeri hatırlanır. Fare üstüne gelince kısayollar açılır ve **+ Enter** bölümü daha belirgin gösterilir.

## Rozet durumları

| Durum | Rozet | Açık hâli (fare üstünde / kendiliğinden) |
|---|:---:|---|
| **Hazır**<br><sub>boşta; üstüne gelince iki kısayol</sub> | <img src="docs/badge/ready-square.png" width="64" alt="Hazır"> | <img src="docs/badge/ready-open.png" width="224" alt="Hazır"> |
| **Dinliyor**<br><sub>kayıt sürüyor, nokta yanıp söner</sub> | <img src="docs/badge/listening-square.png" width="64" alt="Dinliyor"> | <img src="docs/badge/listening-open.png" width="214" alt="Dinliyor"> |
| **Yazıyor**<br><sub>ses yazıya dökülüyor</sub> | <img src="docs/badge/transcribing-square.png" width="64" alt="Yazıyor"> | <img src="docs/badge/transcribing-open.png" width="200" alt="Yazıyor"> |
| **Yapıştırıldı**<br><sub>4 sn kendiliğinden açılır</sub> | <img src="docs/badge/pasted-square.png" width="64" alt="Yapıştırıldı"> | <img src="docs/badge/pasted-open.png" width="208" alt="Yapıştırıldı"> |
| **Kopyalandı**<br><sub>yalnızca kopyala modu; kendiliğinden açılır</sub> | <img src="docs/badge/copied-square.png" width="64" alt="Kopyalandı"> | <img src="docs/badge/copied-open.png" width="205" alt="Kopyalandı"> |
| **Hata**<br><sub>kendiliğinden açılır, mesajın tamamı</sub> | <img src="docs/badge/error-square.png" width="64" alt="Hata"> | <img src="docs/badge/error-open.png" width="218" alt="Hata"> |
| **Başlıyor**<br><sub>ChatGPT sekmesi: mikrofon açılıyor</sub> | <img src="docs/badge/starting-square.png" width="64" alt="Başlıyor"> | <img src="docs/badge/starting-open.png" width="192" alt="Başlıyor"> |
| **İzin bekleniyor**<br><sub>ChatGPT sekmesi: Chrome'da mikrofona izin ver; kendiliğinden açılır</sub> | <img src="docs/badge/permission-square.png" width="64" alt="İzin bekleniyor"> | <img src="docs/badge/permission-open.png" width="247" alt="İzin bekleniyor"> |
| **Bağlı değil**<br><sub>ChatGPT sekmesi: chatgpt.com sekmesi yok</sub> | <img src="docs/badge/closed-square.png" width="64" alt="Bağlı değil"> | <img src="docs/badge/closed-open.png" width="220" alt="Bağlı değil"> |

## Güncelleme

Eski uygulamadan çık, EXE ve eklenti klasörünü yenisiyle değiştir; `bridge-config.json` ve
`dikte-ayarlar.json` dosyalarını koru. `Setup.bat` çalıştır; eklentiyi kullanıyorsan eklentiyi ve
ChatGPT sekmesini yenile. Oluşturulan ayar dosyaları anahtar içerebilir; yerel kalmalıdır.

## Derleme

Rust ve Visual Studio C++ Build Tools kurulu Windows'ta, depo kökünde çalıştır:

```powershell
cargo test --locked
./scripts/package.ps1
```

ZIP ve SHA-256 özeti `dist/` içine yazılır. Yerel kullanım için `./scripts/setup.ps1` çalıştırıp
`target/release/ChatGPT-Dikte.exe` dosyasını `dist/` içine kopyala. Python yalnızca isteğe bağlı
varlık üretme araçlarında kullanılır; uygulama ve kurulum için gerekmez.

[Geliştirme rehberi](docs/DEVELOPMENT.md) · [Ses dosyaları](assets/sounds/README.md) · [Sürüm notları](CHANGELOG.md)
