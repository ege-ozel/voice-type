//! OpenAI ses-metin (API modu): WAV paketleme, multipart istek gövdesi ve yanıt çözme. Ağ ve
//! mikrofon Windows tarafındadır (`win/audio.rs`, `win/openai.rs`); burası her yerde test edilir.
use serde_json::Value;

pub const HOST: &str = "api.openai.com";
pub const PATH: &str = "/v1/audio/transcriptions";
pub const DEFAULT_MODEL: &str = "gpt-4o-transcribe";
pub const SAMPLE_RATE: u32 = 16_000;
/// Bundan kısa kayıt (başlık + 0,3 sn) gönderilmez.
pub const MIN_WAV: usize = 44 + (SAMPLE_RATE as usize * 2 * 3 / 10);
/// Türkçe/İngilizce karışık konuşma ve yazılım terimleri için ipucu (voice_type.py ile aynı).
pub const PROMPT: &str = "Konuşma Türkçe ve İngilizce karışık olabilir. Yazılım terimleri \
                          (commit, branch, build, deploy, test, API, terminal) İngilizce yazılır.";
const BOUNDARY: &str = "----dikte-7f3a9c1e5b";

/// 16 bit mono PCM'i WAV dosyasına sarar.
pub fn wav(pcm: &[u8], rate: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + pcm.len());
    let len = pcm.len() as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

pub fn content_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

/// `file`, `model`, `prompt` alanlarıyla multipart/form-data gövdesi.
pub fn body(wav: &[u8], model: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(wav.len() + 1024);
    let field = |out: &mut Vec<u8>, name: &str, value: &str| {
        out.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    };
    field(&mut out, "model", model);
    field(&mut out, "prompt", PROMPT);
    out.extend_from_slice(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n\
                 Content-Type: audio/wav\r\n\r\n")
            .as_bytes(),
    );
    out.extend_from_slice(wav);
    out.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    out
}

/// HTTP durum kodu ve gövdeden metni ya da kullanıcıya gösterilecek hatayı çıkarır.
pub fn parse(status: u32, body: &[u8]) -> Result<String, String> {
    let json: Option<Value> = serde_json::from_slice(body).ok();
    if status == 200 {
        return json
            .as_ref()
            .and_then(|v| v.get("text")?.as_str())
            .map(|t| t.trim().to_string())
            .ok_or_else(|| "OpenAI yanıtı anlaşılamadı.".to_string());
    }
    let detail = json
        .as_ref()
        .and_then(|v| v.get("error")?.get("message")?.as_str())
        .map(|m| crate::text::prefix(m, 160).to_string());
    let reason = match status {
        401 => "OpenAI API anahtarı geçersiz.".to_string(),
        429 => "OpenAI kota ya da hız sınırı aşıldı (faturalandırmayı kontrol edin).".to_string(),
        _ => format!("OpenAI hatası (HTTP {status})."),
    };
    Err(match detail {
        Some(d) if status != 401 => format!("{reason} {d}"),
        _ => reason,
    })
}

/// Anahtarı ortam değişkeninden, yoksa ayar dosyasındaki `openai_api_key` alanından alır.
pub fn api_key(env: Option<String>, settings: &Value) -> Option<String> {
    env.or_else(|| settings.get("openai_api_key")?.as_str().map(String::from))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

pub fn model(settings: &Value) -> String {
    settings
        .get("openai_model")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wav_header_is_valid() {
        let w = wav(&[1, 0, 2, 0], 16_000);
        assert_eq!(w.len(), 48);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 40);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 32_000);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 4);
        assert_eq!(&w[44..], &[1, 0, 2, 0]);
    }

    #[test]
    fn body_has_fields_and_file() {
        let b = body(b"WAVDATA", "gpt-4o-transcribe");
        let s = String::from_utf8_lossy(&b);
        assert!(content_type().ends_with(BOUNDARY));
        assert!(s.contains("name=\"model\"\r\n\r\ngpt-4o-transcribe\r\n"));
        assert!(s.contains("name=\"prompt\"\r\n\r\nKonuşma Türkçe"));
        assert!(s.contains("filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nWAVDATA\r\n"));
        assert!(s.ends_with(&format!("--{BOUNDARY}--\r\n")));
    }

    #[test]
    fn parse_success_and_errors() {
        assert_eq!(parse(200, br#"{"text": "  merhaba dunya \n"}"#), Ok("merhaba dunya".into()));
        assert_eq!(parse(200, b"bozuk"), Err("OpenAI yanıtı anlaşılamadı.".into()));
        let e = br#"{"error": {"message": "Incorrect API key provided: sk-abc"}}"#;
        assert_eq!(parse(401, e), Err("OpenAI API anahtarı geçersiz.".into()), "anahtar parçası gösterilmez");
        let e = br#"{"error": {"message": "You exceeded your current quota"}}"#;
        assert!(parse(429, e).unwrap_err().contains("kota"));
        assert_eq!(parse(500, b""), Err("OpenAI hatası (HTTP 500).".into()));
    }

    #[test]
    fn key_and_model_sources() {
        let s = json!({"openai_api_key": " sk-dosya ", "openai_model": "gpt-4o-mini-transcribe"});
        assert_eq!(api_key(Some("sk-env".into()), &s).as_deref(), Some("sk-env"));
        assert_eq!(api_key(None, &s).as_deref(), Some("sk-dosya"));
        assert_eq!(api_key(Some("  ".into()), &json!({})), None);
        assert_eq!(model(&s), "gpt-4o-mini-transcribe");
        assert_eq!(model(&json!({})), DEFAULT_MODEL);
    }
}
