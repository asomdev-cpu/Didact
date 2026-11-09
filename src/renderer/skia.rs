//! Backend tiny-skia : dessine les formes dans une image en mémoire (sur processeur).
//!
//! L'image (`Pixmap`) peut ensuite être affichée dans la fenêtre,
//! enregistrée en PNG, ou (plus tard) envoyée à un encodeur vidéo.

use super::geometry::{PathCmd, Rgba, Shape};
use super::RenderBackend;
use tiny_skia::{BlendMode, Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

pub struct SkiaBackend {
    pub pixmap: Pixmap,
}

impl SkiaBackend {
    /// None si la taille est nulle
    pub fn new(width: u32, height: u32) -> Option<SkiaBackend> {
        Pixmap::new(width, height).map(|pixmap| SkiaBackend { pixmap })
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    pub fn save_png(&self, path: &str) -> Result<(), String> {
        self.pixmap.save_png(path).map_err(|e| e.to_string())
    }
}

fn to_color(c: Rgba) -> Color {
    Color::from_rgba(c.r, c.g, c.b, c.a.clamp(0.0, 1.0)).unwrap_or(Color::BLACK)
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(to_color(c));
    p.anti_alias = true;
    p
}

/// Flou en boîte, trois passes (≈ flou gaussien), sur une image RGBA prémultipliée
fn blur(pixmap: &mut Pixmap, radius: usize) {
    if radius == 0 {
        return;
    }
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data_mut();
    let mut line: Vec<[u32; 4]> = Vec::new();
    let mut pass = |data: &mut [u8], horizontal: bool| {
        let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
        let index = |o: usize, i: usize| if horizontal { (o * w + i) * 4 } else { (i * w + o) * 4 };
        for o in 0..outer {
            line.clear();
            line.extend((0..inner).map(|i| {
                let k = index(o, i);
                [data[k] as u32, data[k + 1] as u32, data[k + 2] as u32, data[k + 3] as u32]
            }));
            // Somme glissante sur [i - r, i + r]
            let mut sum = [0u32; 4];
            let get = |i: isize| if i < 0 || i >= inner as isize { [0; 4] } else { line[i as usize] };
            for i in -(radius as isize)..=(radius as isize) {
                let p = get(i);
                for c in 0..4 {
                    sum[c] += p[c];
                }
            }
            let n = (2 * radius + 1) as u32;
            for i in 0..inner {
                let k = index(o, i);
                for c in 0..4 {
                    data[k + c] = (sum[c] / n) as u8;
                }
                let (add, sub) = (get(i as isize + radius as isize + 1), get(i as isize - radius as isize));
                for c in 0..4 {
                    sum[c] = sum[c] + add[c] - sub[c];
                }
            }
        }
    };
    for _ in 0..3 {
        pass(data, true);
        pass(data, false);
    }
}

impl RenderBackend for SkiaBackend {
    /// Halo : les formes sont dessinées à part, floutées, renforcées, puis ajoutées en mode « écran »
    /// (elles éclaircissent ce qui est dessous ; les formes nettes sont déjà dessinées).
    fn draw_glow(&mut self, shapes: &[Shape], radius: f32) {
        let r = radius.max(1.0);
        // Zone touchée : boîte des formes, agrandie du halo
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for s in shapes {
            let pad = s.stroke.map(|(_, w)| w).unwrap_or(0.0);
            for cmd in &s.path {
                let pts: &[(f32, f32)] = &match *cmd {
                    PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) => [(x, y), (x, y), (x, y)],
                    PathCmd::CubicTo(a, b, c, d, x, y) => [(a, b), (c, d), (x, y)],
                    PathCmd::Close => continue,
                };
                for &(x, y) in pts {
                    (x0, y0, x1, y1) = (x0.min(x - pad), y0.min(y - pad), x1.max(x + pad), y1.max(y + pad));
                }
            }
        }
        let margin = 3.0 * r + 2.0;
        let (w, h) = (self.pixmap.width() as f32, self.pixmap.height() as f32);
        let (bx0, by0) = ((x0 - margin).max(0.0).floor(), (y0 - margin).max(0.0).floor());
        let (bx1, by1) = ((x1 + margin).min(w).ceil(), (y1 + margin).min(h).ceil());
        if bx1 > bx0 && by1 > by0 {
            if let Some(mut layer) = SkiaBackend::new((bx1 - bx0) as u32, (by1 - by0) as u32) {
                let shift = Transform::from_translate(-bx0, -by0);
                for s in shapes {
                    layer.draw_with(s, shift);
                }
                blur(&mut layer.pixmap, (r / 2.0).round().max(1.0) as usize);
                // Renforce le halo (sinon le flou le rend presque invisible)
                for v in layer.pixmap.data_mut() {
                    *v = (*v as u32 * 2).min(255) as u8;
                }
                let paint = PixmapPaint { blend_mode: BlendMode::Screen, ..PixmapPaint::default() };
                self.pixmap.draw_pixmap(bx0 as i32, by0 as i32, layer.pixmap.as_ref(), &paint, Transform::identity(), None);
            }
        }
    }

    fn clear(&mut self, color: Rgba) {
        self.pixmap.fill(to_color(color));
    }

    fn draw(&mut self, shape: &Shape) {
        self.draw_with(shape, Transform::identity());
    }
}

impl SkiaBackend {
    /// Dessine une forme avec une transformation (décalage pour le calque du halo)
    fn draw_with(&mut self, shape: &Shape, transform: Transform) {
        let mut pb = PathBuilder::new();
        for cmd in &shape.path {
            match *cmd {
                PathCmd::MoveTo(x, y) => pb.move_to(x, y),
                PathCmd::LineTo(x, y) => pb.line_to(x, y),
                PathCmd::CubicTo(x1, y1, x2, y2, x, y) => pb.cubic_to(x1, y1, x2, y2, x, y),
                PathCmd::Close => pb.close(),
            }
        }
        let Some(path) = pb.finish() else { return }; // chemin vide ou dégénéré

        if let Some(fill) = shape.fill {
            self.pixmap
                .fill_path(&path, &paint(fill), FillRule::Winding, transform, None);
        }
        if let Some((color, width)) = shape.stroke {
            if width > 0.0 {
                let stroke = Stroke {
                    width,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                };
                self.pixmap
                    .stroke_path(&path, &paint(color), &stroke, transform, None);
            }
        }
    }
}
