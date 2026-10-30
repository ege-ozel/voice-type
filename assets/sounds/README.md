# Bildirim sesleri

- `listening.wav`: mikrofon dinlemeye başladığında (API ve ChatGPT sekmesi).
- `error.wav`: dikte, kayıt veya metin teslimi hatasında.
- `pasting.wav`: başarılı yapıştırmada veya yalnızca panoya kopyalamada.
- `mouth-listening.wav`, `mouth-error.wav`, `mouth-pasting.wav`: aynı olaylar için **Ağız** paketi.
- `processing.wav`: **Yazıyor** aşamasına geçişte bir kez çalan 480 ms yumuşak geçiş sesi;
  iki paket için ortak. Filtrelenmiş gürültüden üretilmiş, yumuşak giriş/çıkışlı iki hafif fırça darbesi;
  başında 120 ms çıkış aygıtının açılması için pay vardır. Diğer sesler gibi genel ses ayarına bağlıdır.

Rozet/tepsi sağ tık → **Ses paketi → Orijinal / Ağız** seçimi anında uygulanır ve `sound_pack`
ayarında korunur. Durum penceresindeki **Ses paketi** düğmesi aynı seçimi sunar.
Varsayılan Orijinal'dir; bilinmeyen ayar da Orijinal'e döner. İptal ve işlem sürerken tekrar basma
hata sesiyle bildirilir. Sentetik bip kullanılmaz.

Paket sesleri kullanıcının verdiği WAV dosyalarıdır. Stereo, 48 kHz ve 1 saniyelik süreleri korunarak
24 bit PCM'den 16 bit PCM'e dönüştürülmüştür. `src/sound.rs` dosyaları EXE'ye gömer;
çalışma anında dosya yolu, harici oynatıcı veya ses çözücü gerekmez. Ses seviyesi PCM
örneklerine uygulanır ve çalma tek işçi iş parçacığında, geliş sırasına göre yapılır. Çalma sonucu
ve süresi `dikte.log` içine yazılır; ses hataları sessizce atlanmaz. Eklenti, ChatGPT içinde
elle durdurulduğunda da `transcribing` olayını bildirir; kısayolun sesini tekrar çalmaz.

## Ses yüksekliği

Tam uygulama seviyesinde, pyloudnorm 0.2.0 DeMan / ITU-R BS.1770 filtresi, 400 ms blok ile ölçüm:

| Dosya | Entegre LUFS |
|---|---:|
| error.wav | -33.26 |
| listening.wav | -27.16 |
| pasting.wav | -33.78 |
| mouth-error.wav | -27.01 |
| mouth-listening.wav | -28.14 |
| mouth-pasting.wav | -24.86 |
| processing.wav | -29.15 |

Processing hedefi altı paket sesinin medyanından 1.5 LU daha sakindir; paket sesleri değiştirilmez.
Kısa efektlerde LUFS yalnızca karşılaştırma içindir; Windows/aygıt sesi ve uygulama seviyesi son
duyulan yüksekliği değiştirir. Yeniden üretmek ve ölçmek için yalnızca geliştirme ortamında numpy
ve pyloudnorm kurup `python tools/make_processing_sound.py` çalıştırın. Bu bağımlılıklar EXE'ye
ve uygulamanın çalışma gereksinimlerine eklenmez.

Yeni bir dosyayı aynı biçime çevirmek için:

```powershell
python tools/import_sound.py C:/path/listening.wav assets/sounds/listening.wav
```

Ardından EXE yeniden derlenmelidir. Import betiği standart 44 bayt başlıklı 16 bit PCM WAV üretir.
