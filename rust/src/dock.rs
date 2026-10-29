//! Rozetin ekran kenarına yapışma ve genişleme geometrisi (fiziksel piksel, platformdan bağımsız).
//! DeskTuk'taki kenar rayının sade hâli: rozet bırakıldığı yere en yakın kenara sabit boşlukla yapışır,
//! kenar boyunca köşelere kadar kayar. Hesap çalışma alanıyla yapılır; görev çubuğu bu alanın dışında
//! kaldığı için rozet onun üstüne çıkamaz.

/// (sol, üst, sağ, alt)
pub type Bounds = (i32, i32, i32, i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }

    pub fn parse(value: &str) -> Option<Edge> {
        match value {
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            "top" => Some(Edge::Top),
            "bottom" => Some(Edge::Bottom),
            _ => None,
        }
    }
}

/// `at` noktasına en yakın çalışma alanı kenarı.
pub fn nearest_edge(work: Bounds, at: (i32, i32)) -> Edge {
    [
        (Edge::Left, (i64::from(at.0) - i64::from(work.0)).abs()),
        (Edge::Right, (i64::from(at.0) - i64::from(work.2)).abs()),
        (Edge::Top, (i64::from(at.1) - i64::from(work.1)).abs()),
        (Edge::Bottom, (i64::from(at.1) - i64::from(work.3)).abs()),
    ]
    .into_iter()
    .min_by_key(|(_, distance)| *distance)
    .map(|(edge, _)| edge)
    .unwrap_or(Edge::Right)
}

/// Kare rozetin sol üst köşesi: `edge` kenarına `gap` boşlukla yapışık, merkezi `at`'a olabildiğince yakın.
pub fn snap(work: Bounds, edge: Edge, at: (i32, i32), side: i32, gap: i32) -> (i32, i32) {
    let left = (work.0 + gap).min((work.2 - side).max(work.0));
    let right = (work.2 - side - gap).max(left);
    let top = (work.1 + gap).min((work.3 - side).max(work.1));
    let bottom = (work.3 - side - gap).max(top);
    let x = (at.0 - side / 2).clamp(left, right);
    let y = (at.1 - side / 2).clamp(top, bottom);
    match edge {
        Edge::Left => (left, y),
        Edge::Right => (right, y),
        Edge::Top => (x, top),
        Edge::Bottom => (x, bottom),
    }
}

/// Sürüklerken kare serbestçe gezer ama çalışma alanından taşmaz.
pub fn clamp_inside(work: Bounds, pos: (i32, i32), side: i32) -> (i32, i32) {
    (pos.0.clamp(work.0, (work.2 - side).max(work.0)), pos.1.clamp(work.1, (work.3 - side).max(work.1)))
}

/// Genişlemiş rozetin sol x'i ve sola doğru büyüyüp büyümediği: kare yerinde kalır, rozet ekranın
/// geniş tarafına doğru açılır (sağ yarıdaysa sola, sol yarıdaysa sağa).
pub fn expand(work: Bounds, square: (i32, i32), side: i32, width: i32) -> (i32, bool) {
    let grow_left = square.0 + side / 2 > (work.0 + work.2) / 2;
    let x = if grow_left { square.0 + side - width } else { square.0 };
    (x.clamp(work.0, (work.2 - width).max(work.0)), grow_left)
}

/// Yavaşlayarak duran geçiş (kübik).
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn lerp(a: i32, b: i32, t: f32) -> i32 {
    a + ((b - a) as f32 * t).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    // Görev çubuğu altta: çalışma alanı ekranın alt 48 pikselini dışarıda bırakır.
    const WORK: Bounds = (0, 0, 1920, 1032);

    #[test]
    fn snaps_to_every_edge_with_gap_and_reaches_corners() {
        for (at, edge) in [((5, 500), Edge::Left), ((1915, 500), Edge::Right), ((900, 3), Edge::Top),
                           ((900, 1030), Edge::Bottom)] {
            assert_eq!(nearest_edge(WORK, at), edge);
        }
        assert_eq!(snap(WORK, Edge::Right, (1900, 500), 60, 12), (1920 - 60 - 12, 470));
        assert_eq!(snap(WORK, Edge::Left, (10, 500), 60, 12), (12, 470));
        assert_eq!(snap(WORK, Edge::Top, (900, 0), 60, 12), (870, 12));
        assert_eq!(snap(WORK, Edge::Bottom, (900, 1032), 60, 12), (870, 1032 - 60 - 12));
        // Köşeye kadar kayar, görev çubuğuna (y > 1032) taşmaz.
        assert_eq!(snap(WORK, Edge::Right, (5000, 5000), 60, 12), (1848, 960));
        assert_eq!(snap(WORK, Edge::Bottom, (-50, 2000), 60, 12), (12, 960));
    }

    #[test]
    fn works_on_negative_monitors_and_any_taskbar_side() {
        let left_monitor = (-1920, -1080, -60, 0); // görev çubuğu sağda
        let (x, y) = snap(left_monitor, Edge::Right, (0, 0), 60, 12);
        assert_eq!((x + 60, y + 60), (-60 - 12, -12));
        assert_eq!(clamp_inside(left_monitor, (0, 50), 60), (-120, -60));
        assert_eq!(clamp_inside(WORK, (-10, 2000), 60), (0, 972));
    }

    #[test]
    fn expands_toward_the_roomier_side() {
        assert_eq!(expand(WORK, (1848, 960), 60, 200), (1708, true));
        assert_eq!(expand(WORK, (12, 960), 60, 200), (12, false));
        // Çok dar alanda bile çalışma alanında kalır.
        assert_eq!(expand((0, 0, 150, 500), (80, 0), 60, 200).0, 0);
    }

    #[test]
    fn easing_starts_fast_and_lands_exactly() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert!(ease_out(0.5) > 0.8);
        assert_eq!(lerp(100, 200, 0.5), 150);
        assert_eq!(lerp(200, 100, 1.0), 100);
    }

    #[test]
    fn edge_names_round_trip() {
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            assert_eq!(Edge::parse(edge.name()), Some(edge));
        }
        assert_eq!(Edge::parse("middle"), None);
    }
}
