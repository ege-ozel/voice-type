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

pub struct BadgeStyle {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub color: u32, // 0xRRGGBB
    pub ink: u32,
}

impl BadgePhase {
    pub fn style(self) -> BadgeStyle {
        let (title, subtitle, color, ink) = match self {
            BadgePhase::Closed => ("BAĞLI DEĞİL", "chatgpt.com sekmesini açın", 0x555b63, 0xffffff),
            BadgePhase::Ready => ("HAZIR", "Ctrl+Alt+Space: konuş", 0x18864b, 0xffffff),
            BadgePhase::Listening => ("DİNLİYOR", "Ctrl+Alt+Space: bitir", 0xc62828, 0xffffff),
            BadgePhase::Transcribing => ("YAZIYOR…", "ChatGPT yazıya döküyor", 0x1f5fbf, 0xffffff),
            BadgePhase::Copied => ("YAPIŞTIRILDI", "Metin panoda", 0xe8881b, 0x20160b),
            BadgePhase::Error => ("HATA", "Ayrıntı için simgeye tıkla", 0x8e2424, 0xffffff),
        };
        BadgeStyle { title, subtitle, color, ink }
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

/// Durum penceresinin satırları.
pub struct StatusText {
    pub heading: String,
    pub heading_color: u32,
    pub connection: &'static str,
    pub transfer: &'static str,
    pub transfer_color: u32,
    pub checks: String,
    pub details: String,
}

pub const HELP: &str = "Ctrl+Alt+Space — dikteyi başlat / bitir ve yapıştır\n\
                        Ctrl+Alt+Enter — bitir, yapıştır ve Enter gönder\n\
                        Ctrl+Alt+Backspace — iptal\n\
                        Metin, bitirdiğin anda odakta olan pencereye gider.";

pub fn status_text(s: &Snapshot) -> StatusText {
    let busy = s.phase != Phase::Idle;
    let phase_text = match s.phase {
        Phase::Idle => "Hazır",
        Phase::Starting => "Dikte başlatılıyor…",
        Phase::Listening => "Dinliyor",
        Phase::Transcribing => "ChatGPT yazıya döküyor…",
    };
    let t = &s.transfer;
    let mut checks = String::from(if t.copied { "✓ Metin panoya kopyalandı" } else { "— Panoya kopyalama bekleniyor" });
    checks += if t.paste_sent { "\n✓ Yapıştırma kısayolu gönderildi" } else { "\n— Yapıştırma kısayolu gönderilmedi" };
    if t.enter_sent {
        checks += "\n✓ Enter gönderildi";
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
        heading: if s.connected || busy { format!("● {phase_text}") } else { "● ChatGPT bağlı değil".into() },
        heading_color: if busy { 0xc62828 } else if s.connected { 0x16734a } else { 0x9c5f0b },
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
        transfer_color: if t.stage == Stage::Error { 0xa52a2a } else { 0x233a2d },
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
        assert_eq!(s.heading, "● ChatGPT bağlı değil");
        assert_eq!(s.details, "Henüz metin aktarılmadı.");
        assert_eq!(status_text(&snap(Phase::Listening, true)).heading, "● Dinliyor");
    }
}
