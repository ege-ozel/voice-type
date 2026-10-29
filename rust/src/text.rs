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

// Terminal pencereleri: metin tek satıra indirilir ki satır sonları komut gönderimine dönüşmesin.
const TERMINAL_EXES: &[&str] = &[
    "windowsterminal.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "conemu64.exe",
    "conemu.exe",
    "mintty.exe",
];

// Yalnızca bu terminaller Ctrl+V'yi yapıştırma saymaz. Geri kalan her yere (VS Code, Cursor, Windows
// Terminal, konsol, tarayıcı, eklenti panelleri) Ctrl+V gider: VS Code'da Ctrl+Shift+V yapıştırmıyor,
// Markdown önizlemesi açıyordu.
const SHIFT_PASTE_EXES: &[&str] = &["wezterm-gui.exe", "alacritty.exe"];

pub fn is_terminal(target: &Window) -> bool {
    target.class == "ConsoleWindowClass" || TERMINAL_EXES.contains(&target.exe.as_str())
}

pub fn paste_chord(target: &Window) -> Chord {
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

/// Kontrol ve görünmez karakterleri boşluğa çevirip boşlukları teke indirir. `keep_lines` doğruysa
/// satır sonları korunur (art arda boş satırlar teke iner); değilse metin tek satıra indirilir ki
/// terminalde satır sonları komut gönderimine dönüşmesin.
pub fn normalize_text(text: Option<&str>, keep_lines: bool) -> Result<String, String> {
    let text = text.ok_or_else(|| "Metin geçerli değil.".to_string())?;
    let clean = |s: &str| -> String {
        let s: String = s.chars().map(|c| if printable(c) { c } else { ' ' }).collect();
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let joined = if keep_lines {
        let mut lines: Vec<String> = Vec::new();
        for line in text.replace("\r\n", "\n").split(['\n', '\r', '\u{2028}', '\u{2029}']).map(clean) {
            // Boş satır yalnızca dolu bir satırın ardından ve bir kez eklenir.
            if !line.is_empty() || lines.last().is_some_and(|l| !l.is_empty()) {
                lines.push(line);
            }
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    } else {
        clean(text)
    };
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
        assert_eq!(normalize_text(Some("Türkçe\r\nmetin\tİngilizce\x1b[31m"), false).unwrap(),
                   "Türkçe metin İngilizce [31m");
        let long = "a".repeat(32001);
        for keep in [false, true] {
            for value in [Some(""), Some("\n\x00"), Some("\r\n \u{2029}\n"), None, Some(long.as_str())] {
                assert!(normalize_text(value, keep).is_err(), "{value:?}");
            }
            assert_eq!(normalize_text(Some(&"ğ".repeat(32000)), keep).unwrap().chars().count(), 32000);
            assert_eq!(normalize_text(Some("a\u{202E}b\u{200B}c\u{00A0}d"), keep).unwrap(), "a b c d");
        }
    }

    #[test]
    fn line_breaks_are_kept_outside_terminals() {
        let raw = "\r\n  Birinci  satır\r\nikinci\tsatır\n\n\n\nyeni paragraf\x1b \r\n\n";
        assert_eq!(normalize_text(Some(raw), true).unwrap(), "Birinci satır\nikinci satır\n\nyeni paragraf");
        assert_eq!(normalize_text(Some(raw), false).unwrap(), "Birinci satır ikinci satır yeni paragraf");
        assert_eq!(normalize_text(Some("a\rb\u{2028}c"), true).unwrap(), "a\nb\nc");
    }

    #[test]
    fn paste_chord_per_window() {
        let window = |class: &str, exe: &str| Window { class: class.into(), exe: exe.into(), ..terminal() };
        let console = window("ConsoleWindowClass", "cmd.exe");
        let vscode = window("Chrome_WidgetWin_1", "code.exe");
        let cursor = window("Chrome_WidgetWin_1", "cursor.exe");
        let notepad = window("Notepad", "notepad.exe");
        for w in [&terminal(), &console, &vscode, &cursor, &notepad] {
            assert_eq!(paste_chord(w), Chord::CtrlV, "{}", w.exe);
        }
        assert_eq!(paste_chord(&window("x", "alacritty.exe")), Chord::CtrlShiftV);
        assert_eq!(paste_chord(&window("x", "wezterm-gui.exe")), Chord::CtrlShiftV);
        assert!(is_terminal(&terminal()) && is_terminal(&console));
        assert!(!is_terminal(&vscode) && !is_terminal(&cursor) && !is_terminal(&notepad));
    }

    #[test]
    fn prefix_counts_characters() {
        assert_eq!(prefix("çğüşöı", 3), "çğü");
        assert_eq!(prefix("ab", 5), "ab");
    }
}
