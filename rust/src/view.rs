//! Rozet, tepsi ipucu ve durum penceresi metinleri (clipboard_badge.py, status_window.py ve
//! tray_app.py'deki metinlerin birebir karşılığı). Çizim Windows tarafında yapılır.
use std::time::{Duration, Instant};

use crate::bridge::{Engine, Mic, Phase, Snapshot, Stage};
use crate::hotkey::{Slot, SLOTS};
use crate::text::prefix;

pub const FLASH: Duration = Duration::from_secs(4);

/// Hata rozette ne kadar kalır: uzun mesaj okunabilsin diye karakter başına biraz daha (en fazla 12 sn).
pub fn error_flash(message: &str) -> Duration {
    (FLASH + Duration::from_millis(60) * message.chars().count() as u32).min(Duration::from_secs(12))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgePhase {
    Closed,
    Ready,
    /// Sekme mikrofonu açıyor; henüz dinlemiyor.
    Starting,
    /// Tarayıcı mikrofon iznini soruyor; kullanıcının yanıtı bekleniyor.
    Permission,
    Listening,
    Transcribing,
    Copied,
    /// Yalnızca kopyalama modunda: panoda, yapıştırılmadı.
    Clipboard,
    Error,
}

/// Siyah zemin + soluk (mat) vurgu paleti (0xRRGGBB). Tonlar eski neon renklerle aynı, doygunluk ve
/// parlaklık düşük; simgedeki renkler ayrı (icon.rs).
pub mod palette {
    pub const TEAL: u32 = 0x5ab3b9; // eski 0x2ef2d0
    pub const PINK: u32 = 0xd0608f; // eski 0xff2e88
    pub const BLUE: u32 = 0x5c97d6; // eski 0x3d9bff
    pub const YELLOW: u32 = 0xd8cd5a; // eski 0xfcee0a
    pub const RED: u32 = 0xd65a66; // eski 0xff3b4e
    pub const ORANGE: u32 = 0xd99a4f; // eski 0xff9f1c
    pub const GRAY: u32 = 0x6e7681;
    /// Zemin: GitHub karanlık temasından biraz daha koyu.
    pub const BG: u32 = 0x07090d;
    pub const BG_TOP: u32 = 0x0d1117;
    pub const CARD: u32 = 0x10151c;
    pub const LINE: u32 = 0x262d36;
    pub const TEXT: u32 = 0xd6dee6;
    pub const MUTED: u32 = 0x8b949e;
}

pub struct BadgeStyle {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub accent: u32,
}

impl BadgePhase {
    pub fn style(self) -> BadgeStyle {
        use palette::*;
        let (title, subtitle, accent) = match self {
            BadgePhase::Closed => ("BAĞLI DEĞİL", "chatgpt.com sekmesini açın", GRAY),
            BadgePhase::Ready => ("HAZIR", "", TEAL),
            BadgePhase::Starting => ("BAŞLIYOR…", "Mikrofon açılıyor", PINK),
            BadgePhase::Permission => ("İZİN BEKLENİYOR", "Chrome'da mikrofona izin ver", ORANGE),
            BadgePhase::Listening => ("DİNLİYOR", "", PINK),
            BadgePhase::Transcribing => ("YAZIYOR…", "ChatGPT yazıya döküyor", BLUE),
            BadgePhase::Copied => ("YAPIŞTIRILDI", "Metin panoda", YELLOW),
            BadgePhase::Clipboard => ("KOPYALANDI", "Ctrl+V ile yapıştır", YELLOW),
            BadgePhase::Error => ("HATA", "Ayrıntı için simgeye tıkla", RED),
        };
        BadgeStyle { title, subtitle, accent }
    }
}

pub fn badge_phase(s: &Snapshot, now: Instant) -> BadgePhase {
    match s.phase {
        Phase::Starting => return BadgePhase::Starting,
        Phase::Permission => return BadgePhase::Permission,
        Phase::Listening => return BadgePhase::Listening,
        Phase::Transcribing => return BadgePhase::Transcribing,
        Phase::Idle => {}
    }
    let recent = s.phase_at.is_some_and(|t| now.saturating_duration_since(t) < FLASH);
    let copied_now = match (s.transfer.copied_at, s.phase_at) {
        (Some(copied), Some(phase)) => copied + Duration::from_secs(1) >= phase,
        _ => false,
    };
    if recent && s.transfer.stage == Stage::Sent && copied_now {
        return BadgePhase::Copied;
    }
    if recent && s.transfer.stage == Stage::Copied && copied_now {
        return BadgePhase::Clipboard;
    }
    let error_recent = s.phase_at.is_some_and(|t| now.saturating_duration_since(t) < error_flash(&s.message));
    if error_recent && !s.message.is_empty() {
        return BadgePhase::Error;
    }
    if ready(s) { BadgePhase::Ready } else { BadgePhase::Closed }
}

/// API modunda sekme gerekmez.
fn ready(s: &Snapshot) -> bool {
    s.connected || s.engine == Engine::Api
}

/// Rozette gösterilecek (aşama, alt yazı).
pub fn badge(s: &Snapshot, now: Instant) -> (BadgePhase, String) {
    let phase = badge_phase(s, now);
    let toggle = s.hotkeys.get(Slot::Toggle);
    let enter = s.hotkeys.get(Slot::ToggleEnter);
    let subtitle = match phase {
        BadgePhase::Copied => format!("{} karakter{}", s.transfer.characters,
                                      if s.transfer.enter_sent { " + Enter" } else { "" }),
        BadgePhase::Clipboard => format!("{} karakter • Ctrl+V", s.transfer.characters),
        // Rozet metne göre genişler; yine de sığmayan uzun hata "…" ile biter (tamamı durum penceresinde).
        BadgePhase::Error => prefix(&s.message, 90).to_string(),
        BadgePhase::Ready if s.engine == Engine::Browser && s.mic == Mic::Denied => {
            "Mikrofon izni reddedildi".to_string()
        }
        BadgePhase::Ready if s.copy_only => format!("{toggle}: konuş • pano"),
        // Başlatmayı iki tuş da yapar; Enter'ı bitirirken basılan tuş belirler.
        BadgePhase::Ready => format!("{toggle}: konuş • {enter} ile bitir: + Enter"),
        // Bitirmenin iki yolu da görünsün; yalnızca kopyalarken ikisi de aynı işi yapar.
        BadgePhase::Listening if s.copy_only => format!("{toggle} / {enter}: bitir"),
        BadgePhase::Listening => format!("{toggle}: bitir • {enter}: + Enter"),
        BadgePhase::Transcribing if s.engine == Engine::Api => "OpenAI yazıya döküyor".to_string(),
        _ => phase.style().subtitle.to_string(),
    };
    (phase, subtitle)
}

pub fn tray_tip(s: &Snapshot) -> String {
    let toggle = s.hotkeys.get(Slot::Toggle);
    let tip = match s.phase {
        Phase::Starting => "Dikte: başlatılıyor…".to_string(),
        Phase::Permission => "Dikte: mikrofon izni bekleniyor — Chrome'da izin verin".to_string(),
        Phase::Listening => format!("Dikte: dinliyor — bitirmek için {toggle}"),
        Phase::Transcribing if s.engine == Engine::Api => "Dikte: OpenAI yazıya döküyor…".to_string(),
        Phase::Transcribing => "Dikte: ChatGPT yazıya döküyor…".to_string(),
        Phase::Idle if !ready(s) => "Dikte: ChatGPT sekmesi bağlı değil".to_string(),
        Phase::Idle if s.transfer.stage == Stage::Sent => {
            format!("Dikte: hazır — son aktarım {} karakter", s.transfer.characters)
        }
        Phase::Idle if s.transfer.stage == Stage::Copied => {
            format!("Dikte: hazır — {} karakter panoda", s.transfer.characters)
        }
        Phase::Idle => format!("Dikte: hazır — {toggle} ile konuş"),
    };
    prefix(&tip, 120).to_string()
}

/// Durum penceresinin içeriği.
pub struct StatusText {
    pub heading: &'static str,
    pub accent: u32,
    pub connection: String,
    pub transfer: &'static str,
    pub transfer_failed: bool,
    /// (tamam mı, metin)
    pub checks: Vec<(bool, &'static str)>,
    pub details: String,
}

pub struct Shortcut {
    pub slot: Slot,
    /// Her tuş ayrı kapak olarak çizilir.
    pub keys: Vec<String>,
    pub what: &'static str,
}

/// Kısayollar; açıklamalar moda göre.
pub fn shortcuts(s: &Snapshot) -> Vec<Shortcut> {
    SLOTS
        .iter()
        .map(|&slot| Shortcut {
            slot,
            keys: s.hotkeys.get(slot).parts(),
            what: match (slot, s.copy_only) {
                (Slot::Toggle, true) => "başlat / bitir ve kopyala",
                (Slot::Toggle, false) => "başlat / bitir ve yapıştır",
                (Slot::ToggleEnter, true) => "bitir ve kopyala",
                (Slot::ToggleEnter, false) => "bitir, yapıştır ve Enter",
                (Slot::Cancel, _) => "iptal",
            },
        })
        .collect()
}

pub fn shortcut_note(copy_only: bool) -> &'static str {
    if copy_only {
        "Mod: yalnızca panoya kopyala; Ctrl+V ile istediğin yere yapıştır. Kısayolu değiştirmek için satırına \
         tıkla; modu değiştirmek için rozete ya da tepsi simgesine sağ tıkla."
    } else {
        "Metin, bitirdiğin anda odakta olan pencereye gider. Kısayolu değiştirmek için satırına tıkla; \
         yalnızca kopyalamak için rozete ya da tepsi simgesine sağ tıkla."
    }
}

/// Bağlantı satırı: yöntem, sekme ve (ChatGPT sekmesi yönteminde) mikrofon izni.
fn connection(s: &Snapshot) -> String {
    if s.engine == Engine::Api {
        return "Yöntem: OpenAI API (ücretli). Ses doğrudan OpenAI'ye gider; ChatGPT sekmesi gerekmez.".into();
    }
    if s.phase == Phase::Permission {
        return format!("Chrome'daki ChatGPT sekmesinde mikrofon izni sorusuna \"İzin ver\" deyin; ardından dinleme \
                        başlar. Vazgeçmek için {}.", s.hotkeys.get(Slot::Cancel));
    }
    if !s.connected {
        return "Chrome'da chatgpt.com sekmesini açın (arka planda kalabilir). Eklenti yüklü olmalı.".into();
    }
    match s.mic {
        Mic::Unknown => "ChatGPT sekmesi bağlı.",
        Mic::Granted => "ChatGPT sekmesi bağlı. Mikrofon izni verildi.",
        Mic::Prompt => "ChatGPT sekmesi bağlı. Mikrofon izni henüz verilmedi: ilk diktede Chrome soracak, \
                        sekme öne gelir; \"İzin ver\" deyin.",
        Mic::Denied => "ChatGPT sekmesi bağlı, ama mikrofon izni reddedilmiş. Chrome'da chatgpt.com → adres \
                        çubuğundaki site ayarları → Mikrofon: İzin ver.",
    }
    .into()
}

pub fn status_text(s: &Snapshot) -> StatusText {
    use palette::*;
    let (heading, accent) = match s.phase {
        Phase::Starting => ("BAŞLATILIYOR…", PINK),
        Phase::Permission => ("MİKROFON İZNİ BEKLENİYOR", ORANGE),
        Phase::Listening => ("DİNLİYOR", PINK),
        Phase::Transcribing => ("YAZIYA DÖKÜLÜYOR…", BLUE),
        Phase::Idle if ready(s) => ("HAZIR", TEAL),
        Phase::Idle => ("BAĞLI DEĞİL", GRAY),
    };
    let t = &s.transfer;
    let mut checks =
        vec![(t.copied, if t.copied { "Metin panoya kopyalandı" } else { "Panoya kopyalama bekleniyor" })];
    if s.copy_only {
        checks.push((false, "Yapıştırma kapalı (yalnızca kopyala modu)"));
    } else {
        checks.push((t.paste_sent,
                     if t.paste_sent { "Yapıştırma kısayolu gönderildi" } else { "Yapıştırma kısayolu gönderilmedi" }));
    }
    if t.enter_sent {
        checks.push((true, "Enter gönderildi"));
    }
    if t.stage == Stage::Idle {
        checks.clear();
    }
    let mut details = if t.time.is_empty() {
        "Kısayola basıp konuşun; bitirdiğinizde sonucu burada takip edebilirsiniz.".to_string()
    } else {
        format!("{} karakter • {}", t.characters, t.time)
    };
    if !s.target.is_empty() {
        details += &format!("\nHedef: {}", prefix(&s.target, 120));
    }
    if !s.message.is_empty() {
        details += &format!("\n{}", s.message);
    } else if !t.error.is_empty() {
        details += &format!("\n{}", t.error);
    }
    StatusText {
        heading,
        accent,
        connection: connection(s),
        transfer: match t.stage {
            Stage::Idle => "Henüz dikte yapılmadı",
            Stage::Working => "Metin hazırlanıyor…",
            Stage::Sent => "Yapıştırıldı",
            Stage::Copied => "Panoya kopyalandı",
            Stage::Error => "Metin gönderilemedi",
        },
        transfer_failed: t.stage == Stage::Error,
        checks,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::Transfer;
    use crate::hotkey::{Hotkey, Hotkeys};

    fn snap(phase: Phase, connected: bool) -> Snapshot {
        Snapshot { connected, phase, phase_at: None, message: String::new(), target: String::new(),
                   transfer: Transfer::default(), copy_only: false, engine: Engine::Browser,
                   hotkeys: Hotkeys::default(), mic: Mic::Unknown }
    }

    #[test]
    fn badge_phases() {
        let now = Instant::now();
        assert_eq!(badge_phase(&snap(Phase::Idle, false), now), BadgePhase::Closed);
        assert_eq!(badge_phase(&snap(Phase::Idle, true), now), BadgePhase::Ready);
        assert_eq!(badge_phase(&snap(Phase::Starting, false), now), BadgePhase::Starting);
        assert_eq!(badge_phase(&snap(Phase::Listening, true), now), BadgePhase::Listening);
        assert_eq!(badge_phase(&snap(Phase::Transcribing, true), now), BadgePhase::Transcribing);

        let mut copied = snap(Phase::Idle, true);
        copied.phase_at = Some(now);
        copied.transfer = Transfer { stage: Stage::Sent, characters: 5, enter_sent: true, copied_at: Some(now),
                                     ..Transfer::default() };
        assert_eq!(badge(&copied, now), (BadgePhase::Copied, "5 karakter + Enter".into()));
        // 4 sn sonra söner.
        assert_eq!(badge_phase(&copied, now + FLASH), BadgePhase::Ready);

        let mut error = snap(Phase::Idle, true);
        error.phase_at = Some(now);
        error.message = "ChatGPT sekmesi bağlı değil. Chrome açık mı? ...".into();
        // Hata rozetin kendisinde tam görünür (ayrı panel yok).
        assert_eq!(badge(&error, now), (BadgePhase::Error, error.message.clone()));
        // Uzun hata daha uzun kalır.
        assert_eq!(badge_phase(&error, now + FLASH), BadgePhase::Error);
        assert_eq!(badge_phase(&error, now + Duration::from_secs(12)), BadgePhase::Ready);
        error.message = "Kısa hata.".into();
        assert_eq!(badge_phase(&error, now + error_flash("Kısa hata.")), BadgePhase::Ready);
    }

    #[test]
    fn tray_and_status_text() {
        assert_eq!(tray_tip(&snap(Phase::Idle, false)), "Dikte: ChatGPT sekmesi bağlı değil");
        assert_eq!(tray_tip(&snap(Phase::Listening, true)), "Dikte: dinliyor — bitirmek için Ctrl+Alt+Space");
        let s = status_text(&snap(Phase::Idle, false));
        assert_eq!((s.heading, s.accent), ("BAĞLI DEĞİL", palette::GRAY));
        assert_eq!(s.transfer, "Henüz dikte yapılmadı");
        assert!(s.details.contains("Kısayola basıp konuşun"));
        assert!(s.checks.is_empty());
        let listening = status_text(&snap(Phase::Listening, true));
        assert_eq!((listening.heading, listening.accent), ("DİNLİYOR", palette::PINK));
    }

    #[test]
    fn copy_only_mode_texts() {
        let now = Instant::now();
        let mut s = snap(Phase::Idle, true);
        s.copy_only = true;
        assert_eq!(badge(&s, now), (BadgePhase::Ready, "Ctrl+Alt+Space: konuş • pano".into()));
        s.phase_at = Some(now);
        s.transfer = Transfer { stage: Stage::Copied, copied: true, characters: 12, copied_at: Some(now),
                                ..Transfer::default() };
        assert_eq!(badge(&s, now), (BadgePhase::Clipboard, "12 karakter • Ctrl+V".into()));
        assert_eq!(tray_tip(&s), "Dikte: hazır — 12 karakter panoda");
        let t = status_text(&s);
        assert_eq!(t.transfer, "Panoya kopyalandı");
        assert_eq!(t.checks[1], (false, "Yapıştırma kapalı (yalnızca kopyala modu)"));
        assert_eq!(shortcuts(&s)[0].what, "başlat / bitir ve kopyala");
        s.copy_only = false;
        assert_eq!(shortcuts(&s)[0].what, "başlat / bitir ve yapıştır");
    }

    #[test]
    fn api_engine_is_ready_without_tab() {
        let now = Instant::now();
        let mut s = snap(Phase::Idle, false);
        s.engine = Engine::Api;
        assert_eq!(badge_phase(&s, now), BadgePhase::Ready);
        assert_eq!(status_text(&s).heading, "HAZIR");
        assert!(status_text(&s).connection.contains("OpenAI API"));
        s.phase = Phase::Transcribing;
        assert_eq!(badge(&s, now).1, "OpenAI yazıya döküyor");
        assert_eq!(tray_tip(&s), "Dikte: OpenAI yazıya döküyor…");
    }

    #[test]
    fn permission_phase_is_not_shown_as_listening() {
        let now = Instant::now();
        let s = snap(Phase::Permission, true);
        assert_eq!(badge(&s, now), (BadgePhase::Permission, "Chrome'da mikrofona izin ver".into()));
        assert_eq!(status_text(&s).heading, "MİKROFON İZNİ BEKLENİYOR");
        assert!(status_text(&s).connection.contains("İzin ver"));
        assert!(tray_tip(&s).contains("izin"));
        let mut denied = snap(Phase::Idle, true);
        denied.mic = Mic::Denied;
        assert_eq!(badge(&denied, now).1, "Mikrofon izni reddedildi");
        assert!(status_text(&denied).connection.contains("reddedilmiş"));
    }

    #[test]
    fn texts_follow_custom_hotkeys() {
        let now = Instant::now();
        let mut s = snap(Phase::Idle, true);
        s.hotkeys = Hotkeys::default().with(Slot::Toggle, Hotkey::parse("Alt+Win+F9").unwrap()).unwrap();
        assert_eq!(badge(&s, now).1, format!("Alt+Win+F9: konuş • {} ile bitir: + Enter", s.hotkeys.get(Slot::ToggleEnter)));
        assert_eq!(tray_tip(&s), "Dikte: hazır — Alt+Win+F9 ile konuş");
        assert_eq!(shortcuts(&s)[0].keys, vec!["Alt", "Win", "F9"]);
        assert_eq!(shortcuts(&s)[2].keys, vec!["Ctrl", "Alt", "Backspace"]);
        s.phase = Phase::Listening;
        assert_eq!(badge(&s, now).1, format!("Alt+Win+F9: bitir • {}: + Enter", s.hotkeys.get(Slot::ToggleEnter)));
        s.copy_only = true;
        assert_eq!(badge(&s, now).1, format!("Alt+Win+F9 / {}: bitir", s.hotkeys.get(Slot::ToggleEnter)));
    }
}
