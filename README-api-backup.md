# voice-type

Bas-konuş dikte. `Ctrl+Alt+Space`'e bas, konuş, tekrar bas: metin imlecin olduğu
pencereye (Claude Code / Codex terminali, tarayıcı, her yer) yazılır ve panoya da kopyalanır.
Ses tanıma OpenAI `gpt-4o-transcribe` ile yapılır (Türkçe/İngilizce karışık konuşmayı iyi çözer).

## Kurulum (bir kez)

1. https://platform.openai.com/api-keys adresinden bir API key oluştur.
   (ChatGPT aboneliğinden ayrı faturalanır; dakikası birkaç sent.)
2. PowerShell'de key'i kalıcı olarak kaydet, sonra terminali kapatıp aç:

   ```powershell
   setx OPENAI_API_KEY "sk-..."
   ```

3. Bağımlılıklar `.venv` içine kurulu. Yeniden kurmak gerekirse:

   ```powershell
   python -m venv .venv
   .venv\Scripts\pip install -r requirements.txt
   ```

## Kullanım

`start.bat`'a çift tıkla, pencere açık kalsın. Sonra herhangi bir yerde:

- `Ctrl+Alt+Space` → bip (yüksek) → konuş
- `Ctrl+Alt+Space` → bip (alçak) → bir iki saniye sonra metin yazılır

## Ayarlar (ortam değişkenleri, isteğe bağlı)

| Değişken | Varsayılan | Açıklama |
|---|---|---|
| `VOICE_TYPE_HOTKEY` | `ctrl+alt+space` | Kısayol |
| `VOICE_TYPE_MODE` | `type` | `type` = karakter karakter yazar, `paste` = Ctrl+V ile yapıştırır |
| `VOICE_TYPE_MODEL` | `gpt-4o-transcribe` | Daha ucuz: `gpt-4o-mini-transcribe` |
