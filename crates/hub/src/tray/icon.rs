use tauri::AppHandle;
use tauri::image::Image;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Colour {
    #[default]
    Searching,
    Connecting,
    Pc,
    Laptop,
    Problem,
}

pub(super) fn tray_size(app: &AppHandle) -> u32 {
    if cfg!(target_os = "macos") {
        return 36;
    }
    if cfg!(windows) {
        return small_icon_size(app);
    }
    return 32;
}

pub(super) fn menu_icon(app: &AppHandle) -> Option<Image<'static>> {
    let size = small_icon_size(app);
    let png: &[u8] =
        if size <= 32 { include_bytes!("../../icons/32x32.png") } else { include_bytes!("../../icons/64x64.png") };
    let source = Image::from_bytes(png).ok()?;
    return Some(downscale(source.rgba(), source.width(), size));
}

fn small_icon_size(app: &AppHandle) -> u32 {
    let scale = app.primary_monitor().ok().flatten().map_or(1.0, |m| return m.scale_factor());
    return ((16.0 * scale).round() as u32).clamp(16, 64);
}

fn downscale(rgba: &[u8], from: u32, to: u32) -> Image<'static> {
    let mut out = vec![0u8; (to * to * 4) as usize];
    let ratio = from as f32 / to as f32;
    for y in 0..to {
        for x in 0..to {
            let (x0, x1) = ((x as f32 * ratio) as u32, (((x + 1) as f32 * ratio).ceil() as u32).min(from));
            let (y0, y1) = ((y as f32 * ratio) as u32, (((y + 1) as f32 * ratio).ceil() as u32).min(from));
            let mut sum = [0f32; 4];
            let mut n = 0f32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * from + sx) * 4) as usize;
                    let a = rgba[i + 3] as f32 / 255.0;
                    sum[0] += rgba[i] as f32 * a;
                    sum[1] += rgba[i + 1] as f32 * a;
                    sum[2] += rgba[i + 2] as f32 * a;
                    sum[3] += a;
                    n += 1.0;
                }
            }
            let o = ((y * to + x) * 4) as usize;
            if sum[3] > 0.0 {
                for c in 0..3 {
                    out[o + c] = (sum[c] / sum[3]).round() as u8;
                }
                out[o + 3] = (sum[3] / n * 255.0).round() as u8;
            }
        }
    }
    return Image::new_owned(out, to, to);
}

type Shape = (f32, f32, f32, f32, f32);

const STEM: Shape = (1.0, 1.5, 2.5, 13.0, 1.25);
const WEDGE: [Shape; 3] = [(5.5, 6.0, 2.5, 4.0, 1.25), (9.0, 3.75, 2.5, 8.5, 1.25), (12.5, 1.5, 2.5, 13.0, 1.25)];
const BADGE: Shape = (8.25, 8.25, 7.5, 7.5, 3.75);
const BADGE_GAP: Shape = (7.0, 7.0, 10.0, 10.0, 5.0);
const MARK: [Shape; 2] = [(11.35, 9.5, 1.3, 3.0, 0.5), (11.35, 13.2, 1.3, 1.3, 0.6)];

pub(super) fn icon_for(colour: Colour, size: u32) -> Image<'static> {
    let (rgb, stem, wedge) = match colour {
        Colour::Searching | Colour::Problem => ([0x8A, 0x8F, 0x98], 1.0, 1.0),
        Colour::Connecting => ([0xE0, 0xA0, 0x30], 1.0, 1.0),
        Colour::Pc => ([0x2F, 0x7D, 0xE1], 1.0, 0.3),
        Colour::Laptop => ([0x2F, 0xA3, 0x6B], 0.3, 1.0),
    };
    let problem = matches!(colour, Colour::Problem);
    let gap = problem.then_some(BADGE_GAP);
    let mut canvas = Canvas::new(size);
    canvas.fill(STEM, gap, rgb, stem);
    for bar in WEDGE {
        canvas.fill(bar, gap, rgb, wedge);
    }
    if problem {
        canvas.fill(BADGE, None, [0xD6, 0x45, 0x45], 1.0);
        for part in MARK {
            canvas.fill(part, None, [0xFF; 3], 1.0);
        }
    }
    return canvas.into_image();
}

struct Canvas {
    size: u32,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(size: u32) -> Self {
        return Self { size, pixels: vec![[0.0; 4]; (size * size) as usize] };
    }

    fn fill(&mut self, shape: Shape, hole: Option<Shape>, rgb: [u8; 3], alpha: f32) {
        let scale = self.size as f32 / 16.0;
        for (i, pixel) in self.pixels.iter_mut().enumerate() {
            let x = ((i as u32 % self.size) as f32 + 0.5) / scale;
            let y = ((i as u32 / self.size) as f32 + 0.5) / scale;
            let mut a = cover(shape, x, y, scale) * alpha;
            if let Some(hole) = hole {
                a *= 1.0 - cover(hole, x, y, scale);
            }
            for (p, &c) in pixel.iter_mut().zip(&rgb) {
                *p = c as f32 * a + *p * (1.0 - a);
            }
            pixel[3] = a + pixel[3] * (1.0 - a);
        }
    }

    fn into_image(self) -> Image<'static> {
        let straight = |[r, g, b, a]: [f32; 4]| {
            if a == 0.0 {
                return [0; 4];
            } else {
                return [r / a, g / a, b / a, a * 255.0].map(|c| return c.round() as u8);
            }
        };
        let rgba = self.pixels.into_iter().flat_map(straight).collect();
        return Image::new_owned(rgba, self.size, self.size);
    }
}

fn cover((left, top, w, h, r): Shape, x: f32, y: f32, scale: f32) -> f32 {
    let qx = (x - left - w / 2.0).abs() - w / 2.0 + r;
    let qy = (y - top - h / 2.0).abs() - h / 2.0 + r;
    let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
    return (0.5 - distance * scale).clamp(0.0, 1.0);
}
