//! Metin temizleme ve hedef pencereye göre yapıştırma kısayolu (platformdan bağımsız).

pub const MAX_CHARS: usize = 32000;

/// Hedef pencere: durdurma (veya elle durdurmada başlatma) anındaki ön plan penceresi.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub hwnd: isize,
    pub title: String,
    pub class: String,
    /// Küçük harfli çalıştırılabilir dosya adı, ör. "windowsterminal.exe".
    pub exe: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    CtrlV,
    CtrlShiftV,
    Enter,
}

// Bu uygulamalarda Ctrl+V terminal yerine editöre veya uygulamaya gidebilir; Ctrl+Shift+V terminal yapıştırmasıdır.
const SHIFT_PASTE_EXES: &[&str] = &[
    "windowsterminal.exe",
    "code.exe",
    "cursor.exe",
    "windsurf.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "conemu64.exe",
    "conemu.exe",
];

pub fn paste_chord(target: &Window) -> Chord {
    if target.class == "ConsoleWindowClass" {
        return Chord::CtrlV;
    }
    if SHIFT_PASTE_EXES.contains(&target.exe.as_str()) {
        Chord::CtrlShiftV
    } else {
        Chord::CtrlV
    }
}

/// Python'daki `str.isprintable()` karşılığı: kontrol, boşluk (ASCII boşluk hariç) ve görünmez
/// biçim karakterleri (sıfır genişlikli, yön değiştiren vb.) yazdırılamaz sayılır.
fn printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    if c.is_control() || c.is_whitespace() {
        return false;
    }
    !matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E000}'..='\u{F8FF}')
}

/// Yeni satır ve kontrol karakterleri terminalde komut gönderimine dönüşmesin diye tek satıra çevirir.
pub fn normalize_text(text: Option<&str>) -> Result<String, String> {
    let text = text.ok_or_else(|| "Metin geçerli değil.".to_string())?;
    let cleaned: String = text.chars().map(|c| if printable(c) { c } else { ' ' }).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        return Err("Ses algılanmadı; metin boş.".into());
    }
    if joined.chars().count() > MAX_CHARS {
        return Err("Metin çok uzun (en fazla 32000 karakter).".into());
    }
    Ok(joined)
}

/// İlk `n` karakter (bayt değil).
pub fn prefix(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn terminal() -> Window {
        Window { hwnd: 111, title: "Claude Code".into(), class: "CASCADIA_HOSTING_WINDOW_CLASS".into(),
                 exe: "windowsterminal.exe".into() }
    }

    #[test]
    fn control_characters_cannot_submit_input() {
        assert_eq!(normalize_text(Some("Türkçe\r\nmetin\tİngilizce\x1b[31m")).unwrap(), "Türkçe metin İngilizce [31m");
        let long = "a".repeat(32001);
        for value in [Some(""), Some("\n\x00"), None, Some(long.as_str())] {
            assert!(normalize_text(value).is_err(), "{value:?}");
        }
        assert_eq!(normalize_text(Some(&"ğ".repeat(32000))).unwrap().chars().count(), 32000);
        assert_eq!(normalize_text(Some("a\u{202E}b\u{200B}c\u{00A0}d")).unwrap(), "a b c d");
    }

    #[test]
    fn paste_chord_per_window() {
        assert_eq!(paste_chord(&terminal()), Chord::CtrlShiftV);
        let console = Window { class: "ConsoleWindowClass".into(), exe: "cmd.exe".into(), ..terminal() };
        assert_eq!(paste_chord(&console), Chord::CtrlV);
        let notepad = Window { class: "Notepad".into(), exe: "notepad.exe".into(), ..terminal() };
        assert_eq!(paste_chord(&notepad), Chord::CtrlV);
    }

    #[test]
    fn prefix_counts_characters() {
        assert_eq!(prefix("çğüşöı", 3), "çğü");
        assert_eq!(prefix("ab", 5), "ab");
    }
}
