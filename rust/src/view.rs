//! Rozet, tepsi ipucu ve durum penceresi metinleri (clipboard_badge.py, status_window.py ve
//! tray_app.py'deki metinlerin birebir karşılığı). Çizim Windows tarafında yapılır.
use std::time::{Duration, Instant};

use crate::bridge::{Phase, Snapshot, Stage};
use crate::text::prefix;

pub const FLASH: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgePhase {
    Closed,
    Ready,
    Listening,
    Transcribing,
    Copied,
    Error,
}

/// Siyah zemin + neon vurgu paleti (0xRRGGBB).
pub mod palette {
    pub const TEAL: u32 = 0x2ef2d0;
    pub const PINK: u32 = 0xff2e88;
    pub const BLUE: u32 = 0x3d9bff;
    pub const YELLOW: u32 = 0xfcee0a;
    pub const RED: u32 = 0xff3b4e;
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
            BadgePhase::Ready => ("HAZIR", "Ctrl+Alt+Space: konuş", TEAL),
            BadgePhase::Listening => ("DİNLİYOR", "Ctrl+Alt+Space: bitir", PINK),
            BadgePhase::Transcribing => ("YAZIYOR…", "ChatGPT yazıya döküyor", BLUE),
            BadgePhase::Copied => ("YAPIŞTIRILDI", "Metin panoda", YELLOW),
            BadgePhase::Error => ("HATA", "Ayrıntı için simgeye tıkla", RED),
        };
        BadgeStyle { title, subtitle, accent }
    }
}

pub fn badge_phase(s: &Snapshot, now: Instant) -> BadgePhase {
    match s.phase {
        Phase::Starting | Phase::Listening => return BadgePhase::Listening,
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
    if recent && !s.message.is_empty() {
        return BadgePhase::Error;
    }
    if s.connected { BadgePhase::Ready } else { BadgePhase::Closed }
}

/// Rozette gösterilecek (aşama, alt yazı).
pub fn badge(s: &Snapshot, now: Instant) -> (BadgePhase, String) {
    let phase = badge_phase(s, now);
    let subtitle = match phase {
        BadgePhase::Copied => format!("{} karakter{}", s.transfer.characters,
                                      if s.transfer.enter_sent { " + Enter" } else { "" }),
        BadgePhase::Error => prefix(&s.message, 30).to_string(),
        _ => phase.style().subtitle.to_string(),
    };
    (phase, subtitle)
}

pub fn tray_tip(s: &Snapshot) -> String {
    let tip = match s.phase {
        Phase::Starting | Phase::Listening => "Dikte: dinliyor — bitirmek için Ctrl+Alt+Space".to_string(),
        Phase::Transcribing => "Dikte: ChatGPT yazıya döküyor…".to_string(),
        Phase::Idle if !s.connected => "Dikte: ChatGPT sekmesi bağlı değil".to_string(),
        Phase::Idle if s.transfer.stage == Stage::Sent => {
            format!("Dikte: hazır — son aktarım {} karakter", s.transfer.characters)
        }
        Phase::Idle => "Dikte: hazır — Ctrl+Alt+Space ile konuş".to_string(),
    };
    prefix(&tip, 120).to_string()
}

/// Durum penceresinin içeriği.
pub struct StatusText {
    pub heading: &'static str,
    pub accent: u32,
    pub connection: &'static str,
    pub transfer: &'static str,
    pub transfer_failed: bool,
    /// (tamam mı, metin)
    pub checks: Vec<(bool, &'static str)>,
    pub details: String,
}

pub const SHORTCUTS: [(&str, &str); 3] = [
    ("Ctrl+Alt+Space", "başlat / bitir ve yapıştır"),
    ("Ctrl+Alt+Enter", "bitir, yapıştır ve Enter"),
    ("Ctrl+Alt+Backspace", "iptal"),
];
pub const SHORTCUT_NOTE: &str = "Metin, bitirdiğin anda odakta olan pencereye gider.";

pub fn status_text(s: &Snapshot) -> StatusText {
    use palette::*;
    let (heading, accent) = match s.phase {
        Phase::Starting => ("BAŞLATILIYOR…", PINK),
        Phase::Listening => ("DİNLİYOR", PINK),
        Phase::Transcribing => ("YAZIYA DÖKÜLÜYOR…", BLUE),
        Phase::Idle if s.connected => ("HAZIR", TEAL),
        Phase::Idle => ("BAĞLI DEĞİL", GRAY),
    };
    let t = &s.transfer;
    let mut checks = vec![
        (t.copied, if t.copied { "Metin panoya kopyalandı" } else { "Panoya kopyalama bekleniyor" }),
        (t.paste_sent, if t.paste_sent { "Yapıştırma kısayolu gönderildi" } else { "Yapıştırma kısayolu gönderilmedi" }),
    ];
    if t.enter_sent {
        checks.push((true, "Enter gönderildi"));
    }
    let mut details = if t.time.is_empty() {
        "Henüz metin aktarılmadı.".to_string()
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
        connection: if s.connected {
            "ChatGPT sekmesi bağlı."
        } else {
            "Chrome'da chatgpt.com sekmesini açın (arka planda kalabilir). Eklenti yüklü olmalı."
        },
        transfer: match t.stage {
            Stage::Idle => "Aktarım bekleniyor",
            Stage::Working => "Aktarılıyor…",
            Stage::Sent => "Yapıştırıldı",
            Stage::Error => "Aktarım tamamlanamadı",
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

    fn snap(phase: Phase, connected: bool) -> Snapshot {
        Snapshot { connected, phase, phase_at: None, message: String::new(), target: String::new(),
                   transfer: Transfer::default() }
    }

    #[test]
    fn badge_phases() {
        let now = Instant::now();
        assert_eq!(badge_phase(&snap(Phase::Idle, false), now), BadgePhase::Closed);
        assert_eq!(badge_phase(&snap(Phase::Idle, true), now), BadgePhase::Ready);
        assert_eq!(badge_phase(&snap(Phase::Starting, false), now), BadgePhase::Listening);
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
        assert_eq!(badge(&error, now), (BadgePhase::Error, "ChatGPT sekmesi bağlı değil. C".into()));
        assert_eq!(badge_phase(&error, now + FLASH), BadgePhase::Ready);
    }

    #[test]
    fn tray_and_status_text() {
        assert_eq!(tray_tip(&snap(Phase::Idle, false)), "Dikte: ChatGPT sekmesi bağlı değil");
        assert_eq!(tray_tip(&snap(Phase::Listening, true)), "Dikte: dinliyor — bitirmek için Ctrl+Alt+Space");
        let s = status_text(&snap(Phase::Idle, false));
        assert_eq!((s.heading, s.accent), ("BAĞLI DEĞİL", palette::GRAY));
        assert_eq!(s.details, "Henüz metin aktarılmadı.");
        assert_eq!(s.checks, vec![(false, "Panoya kopyalama bekleniyor"), (false, "Yapıştırma kısayolu gönderilmedi")]);
        let listening = status_text(&snap(Phase::Listening, true));
        assert_eq!((listening.heading, listening.accent), ("DİNLİYOR", palette::PINK));
    }
}
