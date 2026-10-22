//! The drawing uses only +, -, *, / and sqrt, so the icons come out byte for byte the same on every platform.

use std::path::{Path, PathBuf};

const TILE: [f64; 3] = [31.0, 42.0, 55.0];
const HERE: [f64; 3] = [47.0, 125.0, 225.0];
const OTHER: [f64; 3] = [47.0, 163.0, 107.0];

/// x, y, width, height on a 256 px grid.
type Rect = (f64, f64, f64, f64);

const TILE_RADIUS: f64 = 56.0;
const LARGE: [Rect; 5] = [
    (40.0, 40.0, 24.0, 176.0),
    (84.0, 108.0, 24.0, 40.0),
    (120.0, 85.0, 24.0, 86.0),
    (156.0, 62.0, 24.0, 132.0),
    (192.0, 40.0, 24.0, 176.0),
];
const SMALL: [Rect; 4] =
    [(33.0, 34.0, 34.0, 188.0), (89.0, 100.0, 34.0, 56.0), (139.0, 67.0, 34.0, 122.0), (189.0, 34.0, 34.0, 188.0)];

const SIZES: [u32; 11] = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024];
const ICO: [u32; 8] = [32, 16, 20, 24, 40, 48, 64, 256];
const ICNS: [(&[u8; 4], u32); 10] = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic13", 256),
    (b"ic09", 512),
    (b"ic14", 512),
    (b"ic10", 1024),
];

pub fn files(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let pngs = SIZES.iter().map(|&size| return png(size, &render(size))).collect::<Result<Vec<_>, _>>()?;
    let png = |size: u32| return pngs[SIZES.iter().position(|&s| return s == size).unwrap()].as_slice();
    let dir = root.join("crates/hub/icons");
    return Ok(vec![
        (dir.join("32x32.png"), png(32).to_vec()),
        (dir.join("64x64.png"), png(64).to_vec()),
        (dir.join("128x128.png"), png(128).to_vec()),
        (dir.join("128x128@2x.png"), png(256).to_vec()),
        (dir.join("icon.png"), png(512).to_vec()),
        (dir.join("icon.ico"), ico(&ICO.map(|size| return (size, png(size))))),
        (dir.join("icon.icns"), icns(&ICNS.map(|(kind, size)| return (kind, png(size))))),
    ]);
}

fn render(size: u32) -> Vec<u8> {
    let scale = size as f64 / 256.0;
    let mut pixels = vec![[0.0f64; 4]; (size * size) as usize]; // premultiplied
    let mut fill = |rect: Rect, radius: f64, rgb: [f64; 3]| {
        let (x, y, w, h, r) = (rect.0 * scale, rect.1 * scale, rect.2 * scale, rect.3 * scale, radius * scale);
        let cols = (x.floor() as u32).saturating_sub(1)..((x + w).ceil() as u32 + 1).min(size);
        for py in (y.floor() as u32).saturating_sub(1)..((y + h).ceil() as u32 + 1).min(size) {
            for px in cols.clone() {
                let d = distance(px as f64 + 0.5, py as f64 + 0.5, (x, y, w, h), r);
                let cover = (0.5 - d).clamp(0.0, 1.0);
                let p = &mut pixels[(py * size + px) as usize];
                for (v, &c) in p.iter_mut().zip(&rgb) {
                    *v = c * cover + *v * (1.0 - cover);
                }
                p[3] = cover + p[3] * (1.0 - cover);
            }
        }
    };

    fill((0.0, 0.0, 256.0, 256.0), TILE_RADIUS, TILE);
    let bars: &[Rect] = if size <= 32 { &SMALL } else { &LARGE };
    for (i, &bar) in bars.iter().enumerate() {
        let t = i as f64 / (bars.len() - 1) as f64;
        fill(bar, bar.2 / 2.0, std::array::from_fn(|c| return HERE[c] + (OTHER[c] - HERE[c]) * t));
    }

    let straight = |[r, g, b, a]: [f64; 4]| {
        if a == 0.0 { return [0; 4] } else { return [r / a, g / a, b / a, a * 255.0].map(|c| return c.round() as u8) }
    };
    return pixels.into_iter().flat_map(straight).collect();
}

/// Distance from a point to the edge of a rounded rectangle; negative inside.
fn distance(px: f64, py: f64, (x, y, w, h): Rect, r: f64) -> f64 {
    let qx = (px - x - w / 2.0).abs() - w / 2.0 + r;
    let qy = (py - y - h / 2.0).abs() - h / 2.0 + r;
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    return (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r;
}

fn png(size: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().map_err(|e| return e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| return e.to_string())?;
    writer.finish().map_err(|e| return e.to_string())?;
    return Ok(out);
}

/// Windows .ico with PNG entries, as `cargo tauri icon` writes them.
fn ico(images: &[(u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [0u16, 1, images.len() as u16] {
        out.extend(v.to_le_bytes());
    }
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, image) in images {
        let dim = (size % 256) as u8; // 0 means 256
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes()); // planes
        out.extend(32u16.to_le_bytes()); // bits per pixel
        out.extend((image.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for (_, image) in images {
        out.extend_from_slice(image);
    }
    return out;
}

/// macOS .icns with PNG entries.
fn icns(images: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let length = |data: usize| return (data as u32 + 8).to_be_bytes();
    let mut out = b"icns".to_vec();
    out.extend(length(images.iter().map(|(_, image)| return image.len() + 8).sum()));
    for (kind, image) in images {
        out.extend_from_slice(*kind);
        out.extend(length(image.len()));
        out.extend_from_slice(image);
    }
    return out;
}
