# ChatGPT dikte → terminal (fare yok)

ChatGPT web sitesinin diktesini kullanır (karışık dilleri iyi anlar). OpenAI API'si çağrılmaz;
API anahtarı veya ücret gerekmez. ChatGPT hesabınızın dikte sınırları geçerlidir.

## Kullanım

Terminalde (veya herhangi bir pencerede) çalışırken:

| Kısayol | İş |
|---|---|
| **Ctrl+Alt+Space** | Dikteyi başlat (yüksek bip, gösterge **DİNLİYOR** / kırmızı) |
| **Ctrl+Alt+Space** | Bitir → metin odaktaki pencereye yapıştırılır (kısa bip) |
| **Ctrl+Alt+Enter** | Bitir → yapıştır **ve Enter gönder** |
| **Ctrl+Alt+Backspace** | İptal |

Chrome'a geçmeniz gerekmez; ChatGPT sekmesi arka planda açık durur. Metin, **bitirme kısayoluna
bastığınız anda odakta olan pencereye** gider. 4 terminal yan yana ise: konuşmaya başlayın,
hangi terminale gidecekse ona geçin (Alt+Tab / Win+sayı / terminal sekme kısayolu), bitirin.

Yeni satırlar tek satıra çevrilir; Ctrl+Alt+Space Enter göndermez.

## İlk kurulum

0. Repoyu yeni klonladıysanız: `python -m pip install -r requirements-exe.txt`, `python make_config.py`
   (yerel anahtarı üretir) ve `python build_exe.py`.
1. `dist\ChatGPT-Dikte.exe` dosyasını açın (veya `start.bat`).
2. Chrome'da `chrome://extensions` → **Geliştirici modu** → **Paketlenmemiş öğe yükle** →
   `D:\CodeSpace\voice-type\extension`. Eklenti zaten yüklüyse ↻ (yeniden yükle) düğmesine basın.
3. Sekmeyi bağlamak için bir şey yapmanız gerekmez: eklenti arka planda sabitlenmiş bir
   `chatgpt.com` sekmesi açar ve otomatik bağlanır (Chrome uyutursa yeniden yükler).
   İlk seferde o sekmede giriş yapın ve bir kez elle **Dictate** ile mikrofon izni verin.
4. Sağ alttaki gösterge **HAZIR** (yeşil) olunca kullanabilirsiniz.

## Gösterge

- **BAĞLI DEĞİL / gri:** ChatGPT sekmesi yok, eklenti yüklü değil veya sekme yenilenmeli.
- **HAZIR / yeşil:** Ctrl+Alt+Space ile konuşmaya başlayabilirsiniz.
- **DİNLİYOR / kırmızı:** ChatGPT kaydediyor.
- **YAZIYOR… / mavi:** ChatGPT metne çeviriyor.
- **YAPIŞTIRILDI / turuncu:** metin yapıştırıldı (birkaç saniye görünür).
- **HATA / koyu kırmızı:** ayrıntı için tepsideki mikrofon simgesine çift tıklayın.

ChatGPT sekmesinde kare (durdur) düğmesine elle basarsanız da metin, dikteyi başlattığınız pencereye
yapıştırılır. Dikte sürerken Ctrl+Alt+Space her zaman bitirir.

## Sorun giderme

- Olaylar `dist\dikte.log` dosyasına yazılır (dikte edilen metin yazılmaz). Sorun olursa bu dosyaya bakın.
- Eklenti dosyaları güncellendiğinde (manifest sürümü artınca) eklenti bir dakika içinde kendini yeniden yükler.

- **BAĞLI DEĞİL** kalıyorsa ChatGPT sekmesini yenileyin (F5).
- "Dikte başlamadı": ChatGPT sekmesinde mikrofon izni verilmemiş veya ChatGPT arayüzü değişmiş.
- Windows pencere geçişine izin vermezse metin panoda kalır: Ctrl+V / Ctrl+Shift+V ile yapıştırın.
- Yönetici olarak çalışan terminale normal izinli uygulama yapıştıramaz; ikisini aynı izinle çalıştırın.
- ChatGPT arayüzü İngilizce dışında bir dildeyse düğme adları farklı olabilir; `extension/content.js`
  içindeki `LABELS` listesine ekleyin.

## Teknik kapsam

Eklenti yalnızca chatgpt.com'da çalışır; ChatGPT'nin Dictate / Stop dictation / Cancel dictation düğmelerine basar ve dikte bitince giriş kutusundaki yeni metni okuyup kutuyu temizler. Sohbet geçmişini okumaz, **Transcribe and send** düğmesine asla basmaz, bu yüzden ChatGPT'ye mesaj gitmez. Yalnızca bilgisayardaki `127.0.0.1` yardımcısına erişir. Yardımcı sadece loopback üzerinde dinler ve kurulum başına rastgele anahtarla istekleri doğrular. Taslaklar günlük dosyasına yazılmaz. `bridge-config.json` ve `extension/config.js` bu kurulumun yerel anahtarını içerir; paylaşmayın.

Önceki API sürümü `voice_type.py` dosyasında korunur, ama `start.bat` artık tarayıcı köprüsünü çalıştırır. Python bağımlılıkları yalnızca `keyboard` ve `pyperclip` paketleridir; mevcut sanal ortam kullanılabilir.

## EXE / sağ alt bildirim alanı

`dist\ChatGPT-Dikte.exe` Python kurulumu gerektirmeden çalışır. Yanındaki
`bridge-config.json` dosyası mevcut Chrome eklentisinin anahtarıyla eşleşir;
EXE ile birlikte tutulmalıdır. Bu dosyayı veya bu kuruluma ait paketi paylaşmayın.

Simgeye sağ tıklayın: **Durum penceresi**, **Çıkış**. Durum penceresini kapatmak köprüyü
durdurmaz. Aynı köprünün iki kopyası eşzamanlı çalışmaz.

Tekrar derlemek ve test etmek için:

```powershell
.\.venv\Scripts\python.exe -m unittest test_bridge
.\.venv\Scripts\python.exe build_exe.py
```
