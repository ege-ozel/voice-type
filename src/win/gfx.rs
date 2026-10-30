//! Ucuz "neon" çizim yardımcıları (düz GDI). Katmanlı pencere yok: parlama, aynı şeklin zemine
//! karıştırılmış sönük renklerle birkaç piksel kaydırılıp altına çizilmesidir. Tek istisna rozetteki
//! mikrofon: `icon::glyph` ile yumuşak kenarlı çizilip `blend` ile yapıştırılır.
//! Çizim yalnızca durum değişince yapıldığından boştayken maliyet sıfırdır.
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{COLORREF, RECT};
use windows_sys::Win32::Graphics::Gdi::*;

pub fn rgb(c: u32) -> COLORREF {
    ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff)
}

/// `a` ile `b` arası doğrusal karışım; `t` = `a`'nın payı.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xff) as f32, ((b >> shift) & 0xff) as f32);
        ((x * t + y * (1.0 - t)).round() as u32).min(255) << shift
    };
    ch(16) | ch(8) | ch(0)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub unsafe fn fill(dc: HDC, r: &RECT, color: u32) {
    let brush = CreateSolidBrush(rgb(color));
    FillRect(dc, r, brush);
    DeleteObject(brush);
}

/// Dikey gradyan: 2 piksellik şeritlerle.
pub unsafe fn gradient(dc: HDC, r: &RECT, top: u32, bottom: u32) {
    let height = (r.bottom - r.top).max(1);
    let mut y = r.top;
    while y < r.bottom {
        let t = 1.0 - (y - r.top) as f32 / height as f32;
        fill(dc, &RECT { left: r.left, top: y, right: r.right, bottom: (y + 2).min(r.bottom) }, mix(top, bottom, t));
        y += 2;
    }
}

/// Yuvarlak köşeli dikdörtgen; `fill_color` None ise yalnızca çerçeve.
pub unsafe fn round_rect(dc: HDC, r: &RECT, radius: i32, fill_color: Option<u32>, border: u32, width: i32) {
    let pen = CreatePen(PS_SOLID, width.max(1), rgb(border));
    let brush = match fill_color {
        Some(c) => CreateSolidBrush(rgb(c)),
        None => GetStockObject(NULL_BRUSH),
    };
    let (old_pen, old_brush) = (SelectObject(dc, pen), SelectObject(dc, brush));
    RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    if fill_color.is_some() {
        DeleteObject(brush);
    }
}

/// Neon çerçeve: ortada parlak çizgi, iki yanında zemine karışan sönük çizgiler.
pub unsafe fn neon_frame(dc: HDC, r: &RECT, radius: i32, accent: u32, bg: u32, unit: i32) {
    let inset = |n: i32| RECT { left: r.left + n, top: r.top + n, right: r.right - n, bottom: r.bottom - n };
    round_rect(dc, &inset(0), radius, None, mix(accent, bg, 0.22), unit);
    round_rect(dc, &inset(2 * unit), radius - 2 * unit, None, mix(accent, bg, 0.30), unit);
    round_rect(dc, &inset(unit), radius - unit, None, accent, unit);
}

pub unsafe fn text(dc: HDC, font: HFONT, color: u32, s: &str, r: &RECT, flags: DRAW_TEXT_FORMAT) {
    let old = SelectObject(dc, font);
    SetTextColor(dc, rgb(color));
    let w = wide(s);
    let mut r = *r;
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, flags | DT_NOPREFIX);
    SelectObject(dc, old);
}

/// `width` genişliğe sarıldığında metnin yüksekliği.
pub unsafe fn height(dc: HDC, font: HFONT, s: &str, width: i32) -> i32 {
    let old = SelectObject(dc, font);
    let w = wide(s);
    let mut r = RECT { left: 0, top: 0, right: width, bottom: 0 };
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
    SelectObject(dc, old);
    r.bottom - r.top
}

/// Metnin tek satırlık genişliği.
pub unsafe fn width(dc: HDC, font: HFONT, s: &str) -> i32 {
    let old = SelectObject(dc, font);
    let w = wide(s);
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, old);
    r.right - r.left
}

#[allow(clippy::too_many_arguments)]
/// Parlayan metin: önce iki halka halinde sönük kopyalar, en üstte biraz beyaza çekilmiş net metin.
pub unsafe fn glow_text(dc: HDC, font: HFONT, s: &str, r: &RECT, flags: DRAW_TEXT_FORMAT, accent: u32, bg: u32,
                        unit: i32) {
    let shifted = |dx: i32, dy: i32| RECT { left: r.left + dx, top: r.top + dy, right: r.right + dx, bottom: r.bottom + dy };
    let far = 2 * unit;
    for (dx, dy) in [(-far, 0), (far, 0), (0, -far), (0, far)] {
        text(dc, font, mix(accent, bg, 0.16), s, &shifted(dx, dy), flags);
    }
    for (dx, dy) in [(-unit, 0), (unit, 0), (0, -unit), (0, unit), (-unit, -unit), (unit, unit), (unit, -unit), (-unit, unit)] {
        text(dc, font, mix(accent, bg, 0.34), s, &shifted(dx, dy), flags);
    }
    text(dc, font, mix(0xffffff, accent, 0.28), s, r, flags);
}

/// Parlayan dolu daire (durum noktası).
pub unsafe fn glow_dot(dc: HDC, cx: i32, cy: i32, radius: i32, accent: u32, bg: u32) {
    for (extra, color) in [(radius, mix(accent, bg, 0.14)), (radius / 2, mix(accent, bg, 0.32)), (0, accent)] {
        let brush = CreateSolidBrush(rgb(color));
        let pen = GetStockObject(NULL_PEN);
        let (old_pen, old_brush) = (SelectObject(dc, pen), SelectObject(dc, brush));
        let r = radius + extra;
        Ellipse(dc, cx - r, cy - r, cx + r + 1, cy + r + 1);
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(brush);
    }
}

/// Önce bellekte çizip tek seferde kopyalar (titreme olmasın).
pub unsafe fn buffered(dc: HDC, w: i32, h: i32, draw: impl FnOnce(HDC)) {
    let mem = CreateCompatibleDC(dc);
    let bmp = CreateCompatibleBitmap(dc, w, h);
    let old = SelectObject(mem, bmp);
    SetBkMode(mem, TRANSPARENT as _);
    draw(mem);
    BitBlt(dc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old);
    DeleteObject(bmp);
    DeleteDC(mem);
}

pub unsafe fn line(dc: HDC, points: &[(i32, i32)], color: u32, width: i32) {
    let pen = CreatePen(PS_SOLID, width.max(1), rgb(color));
    let old = SelectObject(dc, pen);
    if let Some(&(x, y)) = points.first() {
        MoveToEx(dc, x, y, null_mut());
        for &(x, y) in &points[1..] {
            LineTo(dc, x, y);
        }
    }
    SelectObject(dc, old);
    DeleteObject(pen);
}

/// Önceden çarpılmış alfalı, yukarıdan aşağı BGRA kareyi (`size` x `size`) yumuşak kenarlarıyla çizer.
pub unsafe fn blend(dc: HDC, x: i32, y: i32, size: i32, pixels: &[u8]) {
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = size;
    info.bmiHeader.biHeight = -size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    let mut bits = null_mut();
    let bmp = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bmp.is_null() || bits.is_null() {
        return;
    }
    let len = pixels.len().min((size * size * 4) as usize);
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, len);
    let mem = CreateCompatibleDC(dc);
    let old = SelectObject(mem, bmp);
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    GdiAlphaBlend(dc, x, y, size, size, mem, 0, 0, size, size, blend);
    SelectObject(mem, old);
    DeleteObject(bmp);
    DeleteDC(mem);
}
