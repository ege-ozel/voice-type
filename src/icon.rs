//! Uygulama simgesi: koyu yuvarlak kare üstünde neon teal, çizgi (line-art) mikrofon.
//! Kenarlar mesafe alanıyla (SDF) yumuşatılır; her boyut ayrı çizilir, küçültülmez. Yalnızca
//! +, -, *, /, sqrt kullanılır, böylece çıktı her platformda bayt bayt aynıdır.
//! `build.rs` EXE/tepsi simgesi için, testler eklenti PNG'leri (`extension/icons/`) için kullanır.
#![allow(dead_code)]

/// `view::palette` ile aynı renkler (build.rs bu modülü tek başına içerdiği için kopya).
const TEAL: u32 = 0x2ef2d0;
const BG_TOP: u32 = 0x121922;
const BG_BOTTOM: u32 = 0x07090d;

/// Eklentinin istediği boyutlar.
pub const EXTENSION_SIZES: [usize; 4] = [16, 32, 48, 128];

fn rgb(c: u32) -> [f32; 3] {
    [(c >> 16 & 0xff) as f32, (c >> 8 & 0xff) as f32, (c & 0xff) as f32]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// (px, py) noktasının a-b doğru parçasına uzaklığı.
fn segment(px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = clamp01(((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy));
    let (ex, ey) = (px - ax - dx * t, py - ay - dy * t);
    (ex * ex + ey * ey).sqrt()
}

/// Merkezde, kenar uzunluğu `side`, köşe yarıçapı `r` olan yuvarlak karenin işaretli uzaklığı.
fn rounded_square(px: f32, py: f32, side: f32, r: f32) -> f32 {
    let h = side / 2.0;
    let qx = (px - h).abs() - (h - r);
    let qy = (py - h).abs() - (h - r);
    let (mx, my) = (qx.max(0.0), qy.max(0.0));
    (mx * mx + my * my).sqrt() + qx.max(qy).min(0.0) - r
}

/// Mikrofonun 24 birimlik ızgarada, çizgi merkezine uzaklığı. `filled` küçük boyutlarda kapsülü
/// içi dolu ve daha ince yapar (16 px'te içi boş kapsül tek piksellik bir boşluğa iner, tam
/// genişlikte dolu kapsül de tutucuya yapışır).
fn mic(gx: f32, gy: f32, filled: bool) -> f32 {
    const CX: f32 = 12.0;
    let core = segment(gx, gy, (CX, 7.0), (CX, 10.5));
    let capsule = if filled { (core - 2.4).max(0.0) } else { (core - 3.25).abs() };
    // Tutucu: alt yarım çember + iki kısa dikey kol.
    let arc_r = 6.5;
    let (dx, dy) = (gx - CX, gy - 10.5);
    let arc = if dy >= 0.0 { ((dx * dx + dy * dy).sqrt() - arc_r).abs() } else { f32::MAX };
    let left = segment(gx, gy, (CX - arc_r, 9.0), (CX - arc_r, 10.5));
    let right = segment(gx, gy, (CX + arc_r, 9.0), (CX + arc_r, 10.5));
    let stem = segment(gx, gy, (CX, 17.0), (CX, 20.25));
    let base = segment(gx, gy, (CX - 3.25, 20.25), (CX + 3.25, 20.25));
    capsule.min(arc).min(left).min(right).min(stem).min(base)
}

/// `size` x `size` RGBA (düz alfa), satırlar yukarıdan aşağı.
pub fn render(size: usize) -> Vec<u8> {
    let s = size as f32;
    let small = size <= 20;
    // Izgara birimi (px): küçük boyutlarda çizim kareye daha çok yayılır.
    let unit = s / 24.0 * if small { 0.95 } else { 0.82 };
    let stroke = (2.0 * unit).max(1.5);
    let glow_width = if size >= 32 { stroke * 0.9 } else { 0.0 };
    let border = (s / 48.0).max(1.0);
    let radius = s * 0.23;
    let (top, bottom, teal) = (rgb(BG_TOP), rgb(BG_BOTTOM), rgb(TEAL));

    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let tile = rounded_square(px, py, s, radius);
            let alpha = clamp01(0.5 - tile);
            if alpha <= 0.0 {
                continue;
            }
            let mut c = mix(top, bottom, py / s);
            // İnce, sönük teal kenarlık: koyu görev çubuğunda da kare seçilsin.
            c = mix(c, teal, 0.22 * clamp01(0.5 + tile + border));

            let gx = 12.0 + (px - s / 2.0) / unit;
            let gy = 12.0 + (py - s / 2.0) / unit;
            let d = mic(gx, gy, small) * unit - stroke / 2.0;
            if glow_width > 0.0 && d > 0.0 {
                let k = d / glow_width;
                c = mix(c, teal, 0.32 / (1.0 + k * k));
            }
            c = mix(c, teal, clamp01(0.5 - d));

            let o = (y * size + x) * 4;
            for i in 0..3 {
                out[o + i] = c[i].round() as u8;
            }
            out[o + 3] = (alpha * 255.0).round() as u8;
        }
    }
    out
}

/// Rozet için yalnızca mikrofon (içi boş, zeminsiz): `size` x `size` kutunun ortasında, 24 birimlik
/// ızgarası `mic_size` px olan çizim. Önceden çarpılmış alfalı BGRA, satırlar yukarıdan aşağı (GDI
/// `AlphaBlend` biçimi). `stroke` çizgi kalınlığı (px). Parlama `GLOW_REACH` çizgi kalınlığında sıfıra
/// iner; kutu ondan geniş tutulursa kenarlar tam saydam kalır (yoksa parlama kare biçiminde kesilir).
pub fn glyph(size: usize, mic_size: f32, stroke: f32, color: u32) -> Vec<u8> {
    const GLOW_REACH: f32 = 2.2;
    let unit = mic_size / 24.0;
    let offset = (size as f32 - mic_size) / 2.0;
    let reach = stroke * GLOW_REACH;
    let [r, g, b] = rgb(color);
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let gx = (x as f32 + 0.5 - offset) / unit;
            let gy = (y as f32 + 0.5 - offset) / unit;
            let d = mic(gx, gy, false) * unit - stroke / 2.0;
            let line = clamp01(0.5 - d);
            let fade = clamp01(1.0 - d.max(0.0) / reach);
            let a = line + (1.0 - line) * 0.35 * fade * fade;
            let o = (y * size + x) * 4;
            out[o] = (b * a).round() as u8;
            out[o + 1] = (g * a).round() as u8;
            out[o + 2] = (r * a).round() as u8;
            out[o + 3] = (a * 255.0).round() as u8;
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Sıkıştırmasız (stored deflate) PNG; bağımlılık gerektirmez, çıktısı belirlenimci.
pub fn png(size: usize, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(size * (size * 4 + 1));
    for row in rgba.chunks(size * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    for (kind, data) in [(b"IHDR", &ihdr), (b"IDAT", &z), (b"IEND", &Vec::new())] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pixel(img: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let o = (y * size + x) * 4;
        [img[o], img[o + 1], img[o + 2], img[o + 3]]
    }

    #[test]
    fn corners_transparent_center_opaque() {
        for size in [16, 20, 24, 32, 48, 64, 128] {
            let img = render(size);
            assert_eq!(pixel(&img, size, 0, 0)[3], 0, "{size}");
            assert_eq!(pixel(&img, size, size - 1, size - 1)[3], 0, "{size}");
            assert_eq!(pixel(&img, size, size / 2, 2)[3], 255, "{size}");
        }
    }

    #[test]
    fn microphone_is_teal_on_dark() {
        for size in [16, 32, 128] {
            let img = render(size);
            // Ayak çizgisinin ortası teal, iki yanındaki zemin koyu.
            let unit = size as f32 / 24.0 * if size <= 20 { 0.95 } else { 0.82 };
            let base_y = (size as f32 / 2.0 + 8.25 * unit) as usize;
            let [r, g, b, _] = pixel(&img, size, size / 2, base_y);
            assert!(g > 200 && b > 170 && r < 120, "{size}: {r},{g},{b}");
            let [r, g, b, _] = pixel(&img, size, 2, size / 2);
            assert!(r < 60 && g < 80 && b < 80, "{size}: {r},{g},{b}");
        }
    }

    #[test]
    fn glyph_is_hollow_with_transparent_edges() {
        let (size, mic_size) = (60, 48.0);
        let img = glyph(size, mic_size, 2.5, TEAL);
        let unit = mic_size / 24.0;
        let at = |gx: f32, gy: f32| pixel(&img, size, (6.0 + gx * unit) as usize, (6.0 + gy * unit) as usize);
        // Kutunun dört kenarı tam saydam: parlama kare biçiminde kesilmez.
        for i in 0..size {
            for (x, y) in [(i, 0), (i, size - 1), (0, i), (size - 1, i)] {
                assert_eq!(pixel(&img, size, x, y)[3], 0, "kenar ({x}, {y})");
            }
        }
        assert!(at(12.0, 8.75)[3] < 120, "kapsülün içi boş");
        assert_eq!(at(12.0 - 3.25, 8.75)[3], 255, "kapsül çizgisi");
        let [b, g, r, a] = at(12.0, 20.25);
        assert_eq!(a, 255);
        assert_eq!([r, g, b], [0x2e, 0xf2, 0xd0], "tam opak noktada renk aynen");
    }

    #[test]
    fn png_structure() {
        let data = png(2, &[255; 16]);
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&data[12..16], b"IHDR");
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert!(data.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
    }

    /// Eklenti simgeleri bu çizimden üretilir. Güncellemek için: `UPDATE_ICONS=1 cargo test icon`.
    #[test]
    fn extension_icons_up_to_date() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("extension/icons");
        let update = std::env::var_os("UPDATE_ICONS").is_some();
        for size in EXTENSION_SIZES {
            let path = dir.join(format!("icon{size}.png"));
            let want = png(size, &render(size));
            if update {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&path, &want).unwrap();
            }
            let have = std::fs::read(&path).unwrap_or_default();
            assert!(have == want, "{} güncel değil; UPDATE_ICONS=1 cargo test icon", path.display());
        }
    }
}
