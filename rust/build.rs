//! Windows için: tepsi/EXE simgesini (yeşil yuvarlak kare + beyaz mikrofon, tray_app.make_icon ile
//! aynı çizim) .ico olarak üretir ve uygulama bildirimiyle (DPI, ortak denetimler) EXE'ye gömer.
use std::env;
use std::fs;
use std::path::PathBuf;

fn rounded(px: f32, py: f32, (x0, y0, x1, y1): (f32, f32, f32, f32), r: f32) -> bool {
    if px < x0 || px > x1 || py < y0 || py > y1 {
        return false;
    }
    let dx = (x0 + r - px).max(px - (x1 - r)).max(0.0);
    let dy = (y0 + r - py).max(py - (y1 - r)).max(0.0);
    dx * dx + dy * dy <= r * r
}

fn ellipse(px: f32, py: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> bool {
    let (dx, dy) = ((px - cx) / rx, (py - cy) / ry);
    dx * dx + dy * dy <= 1.0
}

/// 64x64 tasarım alanında bir noktanın rengi (RGBA).
fn sample(px: f32, py: f32) -> [f32; 4] {
    let white = [255.0, 255.0, 255.0, 255.0];
    let mic = rounded(px, py, (25.0, 11.0, 39.0, 36.0), 7.0);
    // PIL arc((18,23,46,46), 0, 180, width=4): elipsin alt yarısı, 4 birim kalınlık.
    let (cx, cy, rx, ry) = (32.0, 34.5, 14.0, 11.5);
    let arc = py >= cy && ellipse(px, py, cx, cy, rx, ry) && !ellipse(px, py, cx, cy, rx - 4.0, ry - 4.0);
    let stem = (30.0..=34.0).contains(&px) && (46.0..=53.0).contains(&py);
    let base = (24.0..=40.0).contains(&px) && (51.0..=55.0).contains(&py);
    if mic || arc || stem || base {
        white
    } else if rounded(px, py, (2.0, 2.0, 62.0, 62.0), 16.0) {
        [0x16 as f32, 0x73 as f32, 0x4a as f32, 255.0]
    } else {
        [0.0; 4]
    }
}

/// Kenar yumuşatmalı BGRA, satırlar aşağıdan yukarı (BMP sırası).
fn render(size: usize) -> Vec<u8> {
    const SS: usize = 4;
    let scale = 64.0 / size as f32;
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) * scale;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) * scale;
                    let c = sample(px, py);
                    let a = c[3] / 255.0;
                    for i in 0..3 {
                        acc[i] += c[i] * a;
                    }
                    acc[3] += c[3];
                }
            }
            let n = (SS * SS) as f32;
            let alpha = acc[3] / n;
            let color = |i: usize| if alpha > 0.0 { (acc[i] / n) / (alpha / 255.0) } else { 0.0 };
            let row = size - 1 - y;
            let o = (row * size + x) * 4;
            out[o] = color(2).round() as u8;
            out[o + 1] = color(1).round() as u8;
            out[o + 2] = color(0).round() as u8;
            out[o + 3] = alpha.round() as u8;
        }
    }
    out
}

fn ico(sizes: &[usize]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| {
            let mut img = Vec::new();
            // BITMAPINFOHEADER; yükseklik XOR + AND maskesi için iki katı.
            for v in [40u32, s as u32, (s * 2) as u32] {
                img.extend_from_slice(&v.to_le_bytes());
            }
            img.extend_from_slice(&1u16.to_le_bytes());
            img.extend_from_slice(&32u16.to_le_bytes());
            img.extend_from_slice(&[0u8; 24]);
            img.extend_from_slice(&render(s));
            let mask_row = s.div_ceil(32) * 4;
            img.extend(std::iter::repeat_n(0u8, mask_row * s));
            img
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&s, img) in sizes.iter().zip(&images) {
        let d = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[d, d, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="ChatGPT.Dikte" version="3.0.0.0"/>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
      processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
  </windowsSettings></application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application>
    <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
  </application></compatibility>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out.join("dictation.ico"), ico(&[16, 20, 24, 32, 40, 48, 64])).unwrap();
    fs::write(out.join("app.manifest"), MANIFEST).unwrap();
    let rc = out.join("app.rc");
    let esc = |name: &str| out.join(name).display().to_string().replace('\\', "\\\\");
    fs::write(&rc, format!("1 ICON \"{}\"\n1 24 \"{}\"\n", esc("dictation.ico"), esc("app.manifest"))).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required().unwrap();
}
