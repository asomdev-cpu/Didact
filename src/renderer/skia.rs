//! Backend tiny-skia : dessine les formes dans une image en mémoire (sur processeur).
//!
//! L'image (`Pixmap`) peut ensuite être affichée dans la fenêtre,
//! enregistrée en PNG, ou (plus tard) envoyée à un encodeur vidéo.

use super::geometry::{PathCmd, Rgba, Shape};
use super::RenderBackend;
use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

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

impl RenderBackend for SkiaBackend {
    fn clear(&mut self, color: Rgba) {
        self.pixmap.fill(to_color(color));
    }

    fn draw(&mut self, shape: &Shape) {
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
                .fill_path(&path, &paint(fill), FillRule::Winding, Transform::identity(), None);
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
                    .stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
            }
        }
    }
}
