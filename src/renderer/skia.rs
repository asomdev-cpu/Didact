//! Backend tiny-skia : dessine les formes dans une image en mémoire (sur processeur).
//!
//! L'image (`Pixmap`) peut ensuite être affichée dans la fenêtre,
//! enregistrée en PNG, ou (plus tard) envoyée à un encodeur vidéo.

use super::geometry::{PathCmd, Rgba, Shape};
use super::RenderBackend;
use tiny_skia::{BlendMode, Color, FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

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
        self.draw_masked(shape, transform, None);
    }

    /// Dessine une forme ; avec un masque, seuls les pixels du masque sont modifiés
    fn draw_masked(&mut self, shape: &Shape, transform: Transform, mask: Option<&Mask>) {
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

        // Image : posée sur la boîte du chemin, avec l'opacité de `fill`
        if let Some(img) = &shape.image {
            let b = path.bounds();
            let opacity = shape.fill.map(|c| c.a).unwrap_or(1.0).clamp(0.0, 1.0);
            let Some(src) = tiny_skia::PixmapRef::from_bytes(&img.data, img.width, img.height) else { return };
            // Même taille que le cadre, sans rotation ni échelle : copie directe (bien plus rapide)
            let plain = transform.sx == 1.0 && transform.sy == 1.0 && transform.kx == 0.0 && transform.ky == 0.0;
            // ... et cadre droit, pas tourné (un carré tourné de 90° a la même boîte)
            let upright = match shape.path.as_slice() {
                [PathCmd::MoveTo(x0, y0), PathCmd::LineTo(x1, y1), _, PathCmd::LineTo(x3, y3), PathCmd::Close] => {
                    x1 > x0 && (y1 - y0).abs() < 0.01 && (x3 - x0).abs() < 0.01 && y3 > y0
                }
                _ => true,
            };
            let plain = plain && upright;
            if plain && (b.width() - img.width as f32).abs() < 0.5 && (b.height() - img.height as f32).abs() < 0.5 {
                let (x, y) = ((b.left() + transform.tx).round() as i32, (b.top() + transform.ty).round() as i32);
                let paint = PixmapPaint { opacity, blend_mode: BlendMode::SourceOver, quality: tiny_skia::FilterQuality::Nearest };
                self.pixmap.draw_pixmap(x, y, src, &paint, Transform::identity(), mask);
                return;
            }
            // Cadre à 4 coins (haut-gauche, haut-droit, bas-droit, bas-gauche) : l'image suit ses
            // côtés, même tourné ; sinon, la boîte du chemin
            let to_box = match shape.path.as_slice() {
                [PathCmd::MoveTo(x0, y0), PathCmd::LineTo(x1, y1), PathCmd::LineTo(_, _), PathCmd::LineTo(x3, y3), PathCmd::Close] => {
                    let (w, h) = (img.width as f32, img.height as f32);
                    Transform::from_row((x1 - x0) / w, (y1 - y0) / w, (x3 - x0) / h, (y3 - y0) / h, *x0, *y0)
                }
                _ => Transform::from_row(
                    b.width() / img.width as f32,
                    0.0,
                    0.0,
                    b.height() / img.height as f32,
                    b.left(),
                    b.top(),
                ),
            };
            let mut p = tiny_skia::Paint::default();
            p.anti_alias = true;
            p.shader = tiny_skia::Pattern::new(src, tiny_skia::SpreadMode::Pad, tiny_skia::FilterQuality::Bilinear, opacity, to_box);
            self.pixmap.fill_path(&path, &p, FillRule::Winding, transform, mask);
            return;
        }

        if let Some(fill) = shape.fill {
            self.pixmap
                .fill_path(&path, &paint(fill), FillRule::Winding, transform, mask);
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
                    .stroke_path(&path, &paint(color), &stroke, transform, mask);
            }
        }
    }
}

// ─────────────────────────────────────────────
//  DESSIN EN PARALLÈLE
// ─────────────────────────────────────────────

/// Une instruction de dessin enregistrée
pub enum DrawOp {
    Clear(Rgba),
    Shape(Shape),
    Glow(Vec<Shape>, f32),
}

/// Backend qui n'enregistre que les instructions : elles sont rejouées ensuite en parallèle
#[derive(Default)]
pub struct Recorder {
    pub ops: Vec<DrawOp>,
}

impl RenderBackend for Recorder {
    fn clear(&mut self, color: Rgba) {
        self.ops.clear();
        self.ops.push(DrawOp::Clear(color));
    }
    fn draw(&mut self, shape: &Shape) {
        self.ops.push(DrawOp::Shape(shape.clone()));
    }
    fn draw_glow(&mut self, shapes: &[Shape], radius: f32) {
        self.ops.push(DrawOp::Glow(shapes.to_vec(), radius));
    }
}

/// Boîte (y min, y max) d'une forme en pixels, épaisseur du trait comprise
fn vertical_extent(shape: &Shape) -> (f32, f32) {
    let pad = shape.stroke.map(|(_, w)| w).unwrap_or(0.0) + 2.0;
    let (mut y0, mut y1) = (f32::MAX, f32::MIN);
    for cmd in &shape.path {
        let ys: &[f32] = &match *cmd {
            PathCmd::MoveTo(_, y) | PathCmd::LineTo(_, y) => [y, y, y],
            PathCmd::CubicTo(_, a, _, b, _, y) => [a, b, y],
            PathCmd::Close => continue,
        };
        for &y in ys {
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    (y0 - pad, y1 + pad)
}

impl SkiaBackend {
    /// Rejoue des instructions : l'image est découpée en bandes horizontales dessinées en même
    /// temps sur plusieurs cœurs, puis les halos sont ajoutés sur l'image entière.
    /// Chaque bande est dessinée aux vraies coordonnées, à travers un masque : le résultat est
    /// identique au pixel près au dessin direct. Une forme ne va que dans les bandes qu'elle touche.
    pub fn replay(&mut self, ops: &[DrawOp]) {
        // DIDACT_THREADS=1 force un seul cœur (pour comparer) ; le résultat est le même
        let threads = std::env::var("DIDACT_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
            .clamp(1, 8);
        self.replay_with(ops, threads);
    }

    /// `replay` avec un nombre de fils donné. Le résultat ne dépend pas de ce nombre :
    /// toujours dessiné à travers un masque, aux vraies coordonnées.
    pub fn replay_with(&mut self, ops: &[DrawOp], threads: usize) {
        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        {
            // Plus de bandes que de cœurs : la charge se répartit mieux
            let bands = if threads <= 1 { 1 } else { (threads * 3).min(h as usize / 16).max(1) };
            let band_h = (h as usize).div_ceil(bands) as u32;
            let extents: Vec<(f32, f32)> = ops
                .iter()
                .map(|op| match op {
                    DrawOp::Shape(s) => vertical_extent(s),
                    _ => (f32::MIN, f32::MAX),
                })
                .collect();
            let starts: Vec<u32> = (0..bands as u32).map(|i| i * band_h).filter(|y| *y < h).collect();
            let next = std::sync::atomic::AtomicUsize::new(0);
            let results: std::sync::Mutex<Vec<(u32, Vec<u8>)>> = std::sync::Mutex::new(Vec::new());
            let row = w as usize * 4;
            std::thread::scope(|scope| {
                for _ in 0..threads.clamp(1, starts.len()) {
                    scope.spawn(|| {
                        // Une image entière par fil, réutilisée pour ses bandes
                        let Some(mut canvas) = SkiaBackend::new(w, h) else { return };
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&y0) = starts.get(i) else { break };
                            let bh = band_h.min(h - y0);
                            let (top, bottom) = (y0 as f32, (y0 + bh) as f32);
                            let Some(mut mask) = Mask::new(w, h) else { break };
                            if let Some(rect) = Rect::from_xywh(0.0, top, w as f32, bh as f32) {
                                mask.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, false, Transform::identity());
                            }
                            let range = y0 as usize * row..(y0 + bh) as usize * row;
                            for (op, ext) in ops.iter().zip(&extents) {
                                match op {
                                    DrawOp::Clear(c) => {
                                        let px = to_color(*c).premultiply().to_color_u8();
                                        let bytes = [px.red(), px.green(), px.blue(), px.alpha()];
                                        for chunk in canvas.pixmap.data_mut()[range.clone()].chunks_exact_mut(4) {
                                            chunk.copy_from_slice(&bytes);
                                        }
                                    }
                                    DrawOp::Shape(s) if ext.1 >= top && ext.0 <= bottom => {
                                        canvas.draw_masked(s, Transform::identity(), Some(&mask))
                                    }
                                    _ => {}
                                }
                            }
                            let rows = canvas.pixmap.data()[range].to_vec();
                            results.lock().unwrap().push((y0, rows));
                        }
                    });
                }
            });
            let data = self.pixmap.data_mut();
            for (y0, rows) in results.into_inner().unwrap() {
                let start = y0 as usize * row;
                data[start..start + rows.len()].copy_from_slice(&rows);
            }
        }
        for op in ops {
            if let DrawOp::Glow(shapes, r) = op {
                self.draw_glow(shapes, *r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dessin_parallele_identique() {
        // Même image quel que soit le nombre de fils (1, 2, 5)
        let shapes: Vec<Shape> = (0..60)
            .map(|i| {
                let (x, y) = ((i * 37 % 300) as f32, (i * 53 % 200) as f32);
                Shape {
                    glow: if i % 10 == 0 { 6.0 } else { 0.0 },
                    path: vec![PathCmd::MoveTo(x, y), PathCmd::LineTo(x + 80.0, y + 30.0), PathCmd::LineTo(x + 10.0, y + 70.0), PathCmd::Close],
                    fill: Some(Rgba { r: (i % 7) as f32 / 7.0, g: 0.5, b: 0.3, a: 0.7 }),
                    stroke: Some((Rgba::BLACK, 2.5)),
                    image: None,
                }
            })
            .collect();
        let mut direct = SkiaBackend::new(320, 240).unwrap();
        let mut rec = Recorder::default();
        for b in [&mut direct as &mut dyn RenderBackend, &mut rec as &mut dyn RenderBackend] {
            b.clear(Rgba::WHITE);
            for s in &shapes {
                b.draw(s);
            }
            let glowing: Vec<Shape> = shapes.iter().filter(|s| s.glow > 0.0).cloned().collect();
            b.draw_glow(&glowing, 6.0);
        }
        let image = |threads: usize| {
            let mut b = SkiaBackend::new(320, 240).unwrap();
            b.replay_with(&rec.ops, threads);
            b.pixmap.data().to_vec()
        };
        let one = image(1);
        assert!(one == image(2) && one == image(5), "l'image doit être la même quel que soit le nombre de fils");
        // Et presque identique au dessin direct (arrondis du masque : 3/255 au plus)
        let max = one.iter().zip(direct.pixmap.data()).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(max <= 3, "écart {}", max);
    }
}
