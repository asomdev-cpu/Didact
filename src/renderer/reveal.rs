//! Apparition progressive : `draw` (le contour se trace, puis la figure se remplit)
//! et `write` (pareil, mais morceau par morceau, comme une écriture : lettre après lettre).
//!
//! Chaque forme a sa propre fenêtre de progression, décalée selon son rang ;
//! dans sa fenêtre, son contour est d'abord tracé, puis son remplissage apparaît en fondu.
//! Une forme sans trait (une lettre, par exemple) est tracée avec un trait fin temporaire.

use super::geometry::{PathCmd, Rgba, Shape};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Draw,
    Write,
}

impl Mode {
    /// (décalage total entre la première et la dernière forme, part du tracé dans la fenêtre)
    fn params(self) -> (f32, f32) {
        match self {
            Mode::Draw => (0.2, 0.7),
            Mode::Write => (0.6, 0.5),
        }
    }
}

/// Formes à l'avancement `e` (0 → 1). `first` = rang de la première forme parmi `total`
/// (un groupe révèle les formes de toutes ses figures à la suite).
/// `line_w` = épaisseur du trait temporaire.
pub fn reveal(shapes: Vec<Shape>, mode: Mode, e: f32, first: usize, total: usize, line_w: f32) -> Vec<Shape> {
    if e >= 1.0 {
        return shapes;
    }
    let (lag, trace_part) = mode.params();
    let window = 1.0 - lag;
    let mut out = Vec::new();
    for (i, shape) in shapes.into_iter().enumerate() {
        let rank = first + i;
        let start = if total > 1 { lag * rank as f32 / (total - 1) as f32 } else { 0.0 };
        let local = ((e - start) / window).clamp(0.0, 1.0);
        if local <= 0.0 {
            continue;
        }
        if local >= 1.0 {
            out.push(shape);
            continue;
        }
        // Une image n'a pas de contour à tracer : elle apparaît en fondu
        if shape.image.is_some() {
            let fill = shape.fill.map(|c| Rgba { a: c.a * local, ..c });
            out.push(Shape { fill, ..shape });
            continue;
        }
        let trace = (local / trace_part).min(1.0);
        let fill_part = ((local - trace_part) / (1.0 - trace_part)).clamp(0.0, 1.0);

        // Remplissage en fondu
        if let (Some(c), true) = (shape.fill, fill_part > 0.0) {
            out.push(Shape { glow: 0.0, path: shape.path.clone(), fill: Some(Rgba { a: c.a * fill_part, ..c }), stroke: None, image: None });
        }
        // Contour tracé
        let path = trim(&shape.path, trace);
        match (shape.stroke, shape.fill) {
            (Some(stroke), _) => out.push(Shape { glow: 0.0, path, fill: None, stroke: Some(stroke), image: None }),
            (None, Some(c)) => {
                // trait temporaire, qui s'efface quand le remplissage arrive
                let c = Rgba { a: c.a * (1.0 - fill_part), ..c };
                out.push(Shape { glow: 0.0, path, fill: None, stroke: Some((c, line_w)), image: None });
            }
            (None, None) => {}
        }
    }
    out
}

/// Début du chemin : chaque sous-chemin est tracé jusqu'à la fraction `frac` de sa longueur
pub fn trim(path: &[PathCmd], frac: f32) -> Vec<PathCmd> {
    if frac >= 1.0 {
        return path.to_vec();
    }
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=path.len() {
        if i == path.len() || matches!(path[i], PathCmd::MoveTo(..)) {
            out.extend(trim_subpath(&path[start..i], frac));
            start = i;
        }
    }
    out
}

/// Un morceau de sous-chemin, avec son point de départ
enum Seg {
    Line((f32, f32), (f32, f32)),
    Cubic((f32, f32), (f32, f32), (f32, f32), (f32, f32)),
}

fn cubic_at(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

impl Seg {
    /// Longueur approchée, et longueurs cumulées aux paramètres i/STEPS (pour une courbe)
    fn lengths(&self) -> (f32, Vec<f32>) {
        const STEPS: usize = 16;
        match *self {
            Seg::Line(a, b) => (dist(a, b), Vec::new()),
            Seg::Cubic(p0, p1, p2, p3) => {
                let mut acc = vec![0.0];
                let mut prev = p0;
                for i in 1..=STEPS {
                    let q = cubic_at(p0, p1, p2, p3, i as f32 / STEPS as f32);
                    acc.push(acc.last().unwrap() + dist(prev, q));
                    prev = q;
                }
                (*acc.last().unwrap(), acc)
            }
        }
    }
}

fn trim_subpath(sub: &[PathCmd], frac: f32) -> Vec<PathCmd> {
    let Some(&PathCmd::MoveTo(x0, y0)) = sub.first() else { return Vec::new() };
    let start = (x0, y0);
    let mut segs = Vec::new();
    let mut cur = start;
    for cmd in &sub[1..] {
        match *cmd {
            PathCmd::LineTo(x, y) => {
                segs.push(Seg::Line(cur, (x, y)));
                cur = (x, y);
            }
            PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                segs.push(Seg::Cubic(cur, (x1, y1), (x2, y2), (x, y)));
                cur = (x, y);
            }
            PathCmd::Close => {
                if cur != start {
                    segs.push(Seg::Line(cur, start));
                }
                cur = start;
            }
            PathCmd::MoveTo(..) => {}
        }
    }
    let lens: Vec<(f32, Vec<f32>)> = segs.iter().map(|s| s.lengths()).collect();
    let total: f32 = lens.iter().map(|l| l.0).sum();
    let mut left = total * frac.max(0.0);
    let mut out = vec![PathCmd::MoveTo(x0, y0)];
    if left <= 0.0 {
        return Vec::new();
    }
    for (seg, (len, acc)) in segs.iter().zip(&lens) {
        if left >= *len {
            match *seg {
                Seg::Line(_, b) => out.push(PathCmd::LineTo(b.0, b.1)),
                Seg::Cubic(_, p1, p2, p3) => out.push(PathCmd::CubicTo(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1)),
            }
            left -= len;
            continue;
        }
        // Dernier morceau, coupé
        match *seg {
            Seg::Line(a, b) => {
                let t = if *len > 0.0 { left / len } else { 0.0 };
                out.push(PathCmd::LineTo(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
            Seg::Cubic(p0, p1, p2, p3) => {
                // paramètre t correspondant à la longueur `left`
                let k = acc.iter().position(|&l| l >= left).unwrap_or(acc.len() - 1).max(1);
                let (l0, l1) = (acc[k - 1], acc[k]);
                let within = if l1 > l0 { (left - l0) / (l1 - l0) } else { 0.0 };
                let t = ((k - 1) as f32 + within) / (acc.len() - 1) as f32;
                // de Casteljau : première partie de la courbe, de 0 à t
                let lerp = |a: (f32, f32), b: (f32, f32)| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                let (q0, q1) = (lerp(p0, p1), lerp(p1, p2));
                let q2 = lerp(p2, p3);
                let (r0, r1) = (lerp(q0, q1), lerp(q1, q2));
                let s = lerp(r0, r1);
                out.push(PathCmd::CubicTo(q0.0, q0.1, r0.0, r0.1, s.0, s.1));
            }
        }
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end_point(path: &[PathCmd]) -> (f32, f32) {
        match *path.last().unwrap() {
            PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) | PathCmd::CubicTo(_, _, _, _, x, y) => (x, y),
            PathCmd::Close => panic!(),
        }
    }

    #[test]
    fn trace_partiel() {
        // carré de côté 10 : à 50 %, on s'arrête au coin opposé
        let sq = vec![
            PathCmd::MoveTo(0.0, 0.0),
            PathCmd::LineTo(10.0, 0.0),
            PathCmd::LineTo(10.0, 10.0),
            PathCmd::LineTo(0.0, 10.0),
            PathCmd::Close,
        ];
        assert_eq!(end_point(&trim(&sq, 0.5)), (10.0, 10.0));
        assert_eq!(end_point(&trim(&sq, 0.125)), (5.0, 0.0));
        assert!(trim(&sq, 0.0).is_empty());
        assert_eq!(trim(&sq, 1.0), sq);
        // courbe : le point coupé est sur la courbe
        let c = vec![PathCmd::MoveTo(0.0, 0.0), PathCmd::CubicTo(0.0, 10.0, 10.0, 10.0, 10.0, 0.0)];
        let (x, y) = end_point(&trim(&c, 0.5));
        assert!((x - 5.0).abs() < 0.2 && (y - 7.5).abs() < 0.2, "{} {}", x, y);
    }

    #[test]
    fn ecriture_progressive() {
        let glyph = |x: f32| Shape { glow: 0.0,
            path: vec![PathCmd::MoveTo(x, 0.0), PathCmd::LineTo(x + 1.0, 0.0), PathCmd::LineTo(x, 1.0), PathCmd::Close],
            fill: Some(Rgba::BLACK),
            stroke: None,
            image: None,
        };
        let shapes: Vec<Shape> = (0..5).map(|i| glyph(i as f32 * 2.0)).collect();
        // au début de l'écriture, seules les premières lettres ont commencé
        let early = reveal(shapes.clone(), Mode::Write, 0.1, 0, 5, 1.0);
        assert!(!early.is_empty() && early.len() < 5);
        assert_eq!(reveal(shapes.clone(), Mode::Write, 1.0, 0, 5, 1.0).len(), 5);
        assert!(reveal(shapes, Mode::Draw, 0.0, 0, 5, 1.0).is_empty());
    }
}
