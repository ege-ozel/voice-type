//! Windows için: tepsi/EXE simgesini (`src/icon.rs`: koyu kare + neon çizgi mikrofon) .ico olarak
//! üretir ve uygulama bildirimiyle (DPI, ortak denetimler) EXE'ye gömer.
use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "src/icon.rs"]
mod icon;

/// BGRA, satırlar aşağıdan yukarı (BMP sırası).
fn bmp_pixels(size: usize) -> Vec<u8> {
    let rgba = icon::render(size);
    let mut out = Vec::with_capacity(rgba.len());
    for row in rgba.chunks(size * 4).rev() {
        for p in row.chunks(4) {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
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
            img.extend_from_slice(&bmp_pixels(s));
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
    println!("cargo:rerun-if-changed=src/icon.rs");
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
