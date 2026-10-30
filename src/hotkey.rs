//! Kısayollar: tuş birleşimi, metin biçimi ("Ctrl+Alt+Space") ve doğrulama (platformdan bağımsız).
//! Tuş kodları Windows sanal tuş kodlarıdır (VK_*).
use std::fmt;

use serde_json::{json, Value};

pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const SHIFT: u8 = 4;
pub const WIN: u8 = 8;

const MODIFIER_NAMES: [(u8, &str); 4] = [(CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift"), (WIN, "Win")];

const NAMED_KEYS: &[(u16, &str)] = &[
    (0x08, "Backspace"), (0x09, "Tab"), (0x0D, "Enter"), (0x13, "Pause"), (0x1B, "Esc"), (0x20, "Space"),
    (0x21, "PageUp"), (0x22, "PageDown"), (0x23, "End"), (0x24, "Home"), (0x25, "Left"), (0x26, "Up"),
    (0x27, "Right"), (0x28, "Down"), (0x2D, "Insert"), (0x2E, "Delete"), (0x6A, "NumMul"), (0x6B, "NumAdd"),
    (0x6D, "NumSub"), (0x6E, "NumDec"), (0x6F, "NumDiv"), (0x91, "ScrollLock"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    /// CTRL | ALT | SHIFT | WIN
    pub mods: u8,
    pub vk: u16,
}

/// Kısayolun işi; sıra, kayıt kimliği (1, 2, 3) ve ayar dosyasındaki ad için kullanılır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Toggle,
    ToggleEnter,
    Cancel,
}

pub const SLOTS: [Slot; 3] = [Slot::Toggle, Slot::ToggleEnter, Slot::Cancel];

impl Slot {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            Slot::Toggle => "toggle",
            Slot::ToggleEnter => "toggle_enter",
            Slot::Cancel => "cancel",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Slot::Toggle => "Başlat / bitir",
            Slot::ToggleEnter => "Bitir + Enter",
            Slot::Cancel => "İptal",
        }
    }
}

fn key_name(vk: u16) -> String {
    if let Some((_, name)) = NAMED_KEYS.iter().find(|(k, _)| *k == vk) {
        return name.to_string();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("0x{vk:02X}"),
    }
}

fn parse_key(name: &str) -> Option<u16> {
    if let Some((vk, _)) = NAMED_KEYS.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)) {
        return Some(*vk);
    }
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Some(bytes[0] as u16);
    }
    if let Some(hex) = upper.strip_prefix("0X") {
        return u16::from_str_radix(hex, 16).ok().filter(|vk| (1..=0xFE).contains(vk) && !is_modifier(*vk));
    }
    if let Some(n) = upper.strip_prefix("NUM").and_then(|n| n.parse::<u16>().ok()) {
        return (n <= 9).then_some(0x60 + n);
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        return (1..=24).contains(&n).then_some(0x6F + n);
    }
    None
}

/// Genel kısayol yapılırsa her yerde bozulacak birleşimler: uygulamanın kendi gönderdiği yapıştırma
/// tuşları (Ctrl+V, Ctrl+Shift+V; kısayol olsalar yapıştırma kendini yakalardı), pano/geri al ve Windows'unkiler.
const RESERVED: &[(u8, u16)] = &[
    (CTRL, 0x56), (CTRL | SHIFT, 0x56), (CTRL, 0x43), (CTRL, 0x58), (CTRL, 0x5A), (CTRL, 0x59), (CTRL, 0x41),
    (CTRL, 0x53), (ALT, 0x73), (ALT, 0x09), (CTRL, 0x1B), (ALT, 0x1B), (CTRL | ALT, 0x2E),
];

/// Tek başına basılınca kısayol sayılmayan tuşlar (Shift, Ctrl, Alt, Win ve sol/sağ türleri).
pub fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

impl Hotkey {
    pub const fn new(mods: u8, vk: u16) -> Self {
        Hotkey { mods, vk }
    }

    /// Tuş kapakları için parçalar: ["Ctrl", "Alt", "Space"].
    pub fn parts(&self) -> Vec<String> {
        let mut parts: Vec<String> =
            MODIFIER_NAMES.iter().filter(|(m, _)| self.mods & m != 0).map(|(_, n)| n.to_string()).collect();
        parts.push(key_name(self.vk));
        parts
    }

    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut mods = 0;
        let mut key = None;
        for part in s.split('+').map(str::trim) {
            if let Some((m, _)) = MODIFIER_NAMES.iter().find(|(_, n)| n.eq_ignore_ascii_case(part)) {
                mods |= m;
            } else if key.is_none() {
                key = Some(parse_key(part)?);
            } else {
                return None;
            }
        }
        Some(Hotkey { mods, vk: key? })
    }

    /// F tuşları, Pause ve ScrollLock tek başına da olur; diğerleri yazarken çalınmasın diye Ctrl, Alt
    /// ya da Win ister (Shift tek başına yetmez: Shift+A büyük A yazmayı engellerdi).
    fn needs_modifier(&self) -> bool {
        !matches!(self.vk, 0x70..=0x87 | 0x13 | 0x91)
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.parts().join("+"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkeys(pub [Hotkey; 3]);

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys([Hotkey::new(CTRL | ALT, 0x20), Hotkey::new(CTRL | ALT, 0x0D), Hotkey::new(CTRL | ALT, 0x08)])
    }
}

impl Hotkeys {
    pub fn get(&self, slot: Slot) -> Hotkey {
        self.0[slot.index()]
    }

    /// `slot` için yeni birleşimi doğrular ve uygular; geçersizse kullanıcıya gösterilecek hata.
    pub fn with(&self, slot: Slot, key: Hotkey) -> Result<Hotkeys, String> {
        if is_modifier(key.vk) {
            return Err("Bir tuş da seçin (yalnızca Ctrl/Alt/Shift olmaz).".into());
        }
        if key.needs_modifier() && key.mods & (CTRL | ALT | WIN) == 0 {
            return Err(format!("{key} yazarken çakışır; Ctrl, Alt ya da Win ile birlikte basın (F tuşları tek başına olabilir)."));
        }
        if RESERVED.contains(&(key.mods, key.vk)) {
            return Err(format!("{key} Windows ve diğer uygulamalar için ayrılmış; başka bir birleşim seçin."));
        }
        if let Some(other) = SLOTS.iter().find(|s| **s != slot && self.get(**s) == key) {
            return Err(format!("{key} zaten \"{}\" için kullanılıyor.", other.label()));
        }
        let mut next = *self;
        next.0[slot.index()] = key;
        Ok(next)
    }

    /// Ayar dosyasındaki `hotkeys` nesnesi; eksik ya da bozuk alanlar varsayılanda kalır, çakışan
    /// bir giriş bütünüyle yok sayılır.
    pub fn from_json(value: Option<&Value>) -> Hotkeys {
        let mut keys = Hotkeys::default();
        let Some(map) = value.and_then(Value::as_object) else { return keys };
        for slot in SLOTS {
            let parsed = map.get(slot.key()).and_then(Value::as_str).and_then(Hotkey::parse);
            if let Some(key) = parsed {
                keys.0[slot.index()] = key;
            }
        }
        // Önce tüm alanları oku: kullanıcı iki kısayolun yerini değiştirmiş olabilir.
        // Geçersiz alanları birlikte sıfırla; varsayılanlara dönüş yeni çakışma yaratırsa yinele.
        loop {
            let mut next = keys;
            for slot in SLOTS {
                if keys.with(slot, keys.get(slot)).is_err() {
                    next.0[slot.index()] = Hotkeys::default().get(slot);
                }
            }
            if next == keys {
                break;
            }
            keys = next;
        }
        keys
    }

    pub fn to_json(self) -> Value {
        json!({
            "toggle": self.get(Slot::Toggle).to_string(),
            "toggle_enter": self.get(Slot::ToggleEnter).to_string(),
            "cancel": self.get(Slot::Cancel).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for text in ["Ctrl+Alt+Space", "Ctrl+Alt+Enter", "Ctrl+Alt+Backspace", "Ctrl+Shift+D", "Alt+Win+F9", "F8",
                     "Ctrl+Num5", "Ctrl+Alt+0xBA", "Shift+F12", "Ctrl+Alt+7"] {
            assert_eq!(Hotkey::parse(text).unwrap().to_string(), text);
        }
        assert_eq!(Hotkey::parse(" alt + ctrl + space ").unwrap().to_string(), "Ctrl+Alt+Space");
        assert_eq!(Hotkey::parse("Ctrl+Alt+Space").unwrap().parts(), vec!["Ctrl", "Alt", "Space"]);
        for bad in ["", "Ctrl+Alt", "Ctrl+A+B", "Ctrl+Bilinmez", "F25", "Ctrl+0x11"] {
            assert_eq!(Hotkey::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn validation_blocks_typing_conflicts_and_duplicates() {
        let keys = Hotkeys::default();
        let a = Hotkey::parse("A").unwrap();
        assert!(keys.with(Slot::Toggle, a).unwrap_err().contains("Ctrl, Alt ya da Win"));
        assert!(keys.with(Slot::Toggle, Hotkey::parse("Shift+A").unwrap()).is_err());
        assert!(keys.with(Slot::Toggle, Hotkey::new(CTRL, 0x11)).is_err());
        for reserved in ["Ctrl+V", "Ctrl+Shift+V", "Alt+F4", "Ctrl+C"] {
            let error = keys.with(Slot::Toggle, Hotkey::parse(reserved).unwrap()).unwrap_err();
            assert!(error.contains("ayrılmış"), "{reserved}: {error}");
        }
        let dup = keys.with(Slot::Toggle, keys.get(Slot::Cancel)).unwrap_err();
        assert!(dup.contains("\"İptal\""), "{dup}");
        // Kendi yerine aynısını yazmak sorun değil.
        assert_eq!(keys.with(Slot::Toggle, keys.get(Slot::Toggle)), Ok(keys));
        let f9 = keys.with(Slot::Toggle, Hotkey::parse("F9").unwrap()).unwrap();
        assert_eq!(f9.get(Slot::Toggle).to_string(), "F9");
        assert_eq!(f9.get(Slot::Cancel), keys.get(Slot::Cancel));
    }

    #[test]
    fn json_round_trip_and_fallbacks() {
        let keys = Hotkeys::default().with(Slot::ToggleEnter, Hotkey::parse("Ctrl+Shift+Enter").unwrap()).unwrap();
        assert_eq!(Hotkeys::from_json(Some(&keys.to_json())), keys);
        assert_eq!(Hotkeys::from_json(None), Hotkeys::default());
        let broken = json!({"toggle": "bozuk", "toggle_enter": 5, "cancel": "Ctrl+Alt+Space"});
        // Bozuklar varsayılanda kalır; "İptal" başlat/bitir ile çakıştığı için yok sayılır.
        assert_eq!(Hotkeys::from_json(Some(&broken)), Hotkeys::default());
    }

    #[test]
    fn swapped_shortcuts_survive_restart() {
        let defaults = Hotkeys::default();
        let swapped = Hotkeys([defaults.get(Slot::Cancel), defaults.get(Slot::Toggle), defaults.get(Slot::ToggleEnter)]);
        assert_eq!(Hotkeys::from_json(Some(&swapped.to_json())), swapped);
    }
}
