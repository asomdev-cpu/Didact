//! Découpe des formes 2D par un rectangle (le cadre d'un repère).
//!
//! Une forme entièrement dedans n'est pas touchée. Sinon son chemin est aplati en lignes
//! brisées : le remplissage est découpé comme un polygone (Sutherland–Hodgman), le trait
//! comme une suite de segments (Liang–Barsky) : le bord du cadre n'est jamais tracé.

use super::geometry::{PathCmd, Shape};
use super::warp::flatten;

/// Rectangle : (x min, y min, x max, y max) en pixels
pub type Rect = (f32, f32, f32, f32);

fn path_bbox(path: &[PathCmd]) -> Option<Rect> {
    let mut b = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut add = |x: f32, y: f32| {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    };
    for c in path {
        match *c {
            PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) => add(x, y),
            PathCmd::CubicTo(a, b2, c2, d, x, y) => {
                add(a, b2);
                add(c2, d);
                add(x, y);
            }
            PathCmd::Close => {}
        }
    }
    (b.0 <= b.2).then_some(b)
}

/// Découpe les formes par `r` (en place)
pub fn clip_shapes(shapes: &mut Vec<Shape>, r: Rect) {
    let mut out = Vec::with_capacity(shapes.len());
    for s in shapes.drain(..) {
        // Une image est calculée pour son cadre : on ne la redécoupe pas
        if s.image.is_some() {
            out.push(s);
            continue;
        }
        let Some(b) = path_bbox(&s.path) else {
            out.push(s);
            continue;
        };
        // Entièrement dedans : inchangée
        if b.0 >= r.0 && b.1 >= r.1 && b.2 <= r.2 && b.3 <= r.3 {
            out.push(s);
            continue;
        }
        // Entièrement dehors : disparaît
        if b.2 < r.0 || b.0 > r.2 || b.3 < r.1 || b.1 > r.3 {
            continue;
        }
        let polys = flatten(&s.path, f32::INFINITY);
        if s.fill.is_some() {
            let mut path = Vec::new();
            for (poly, _) in &polys {
                let clipped = clip_polygon(poly, r);
                if clipped.len() >= 3 {
                    path.push(PathCmd::MoveTo(clipped[0].0, clipped[0].1));
                    path.extend(clipped[1..].iter().map(|p| PathCmd::LineTo(p.0, p.1)));
                    path.push(PathCmd::Close);
                }
            }
            if !path.is_empty() {
                out.push(Shape { path, fill: s.fill, stroke: None, glow: s.glow, image: None });
            }
        }
        if s.stroke.is_some() {
            let mut path = Vec::new();
            for (poly, closed) in &polys {
                let mut pts = poly.clone();
                if *closed && pts.len() > 1 {
                    pts.push(pts[0]);
                }
                let mut pen: Option<(f32, f32)> = None;
                for w in pts.windows(2) {
                    match clip_segment(w[0], w[1], r) {
                        Some((a, b)) => {
                            let joined = pen.is_some_and(|p| (p.0 - a.0).abs() < 1e-3 && (p.1 - a.1).abs() < 1e-3);
                            if !joined {
                                path.push(PathCmd::MoveTo(a.0, a.1));
                            }
                            path.push(PathCmd::LineTo(b.0, b.1));
                            pen = Some(b);
                        }
                        None => pen = None,
                    }
                }
            }
            if !path.is_empty() {
                out.push(Shape { path, fill: None, stroke: s.stroke, glow: s.glow, image: None });
            }
        }
    }
    *shapes = out;
}

/// Segment [a, b] restreint au rectangle (Liang–Barsky) ; None s'il est dehors
fn clip_segment(a: (f32, f32), b: (f32, f32), r: Rect) -> Option<((f32, f32), (f32, f32))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-dx, a.0 - r.0), (dx, r.2 - a.0), (-dy, a.1 - r.1), (dy, r.3 - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(((a.0 + t0 * dx, a.1 + t0 * dy), (a.0 + t1 * dx, a.1 + t1 * dy)))
}

/// Polygone restreint au rectangle (Sutherland–Hodgman, garde le sens de parcours)
fn clip_polygon(poly: &[(f32, f32)], r: Rect) -> Vec<(f32, f32)> {
    let mut pts = poly.to_vec();
    // (axe, borne, garder si >= borne)
    for (axis, bound, keep_ge) in [(0, r.0, true), (0, r.2, false), (1, r.1, true), (1, r.3, false)] {
        if pts.is_empty() {
            break;
        }
        let inside = |p: &(f32, f32)| {
            let v = if axis == 0 { p.0 } else { p.1 };
            if keep_ge { v >= bound } else { v <= bound }
        };
        let cross = |a: (f32, f32), b: (f32, f32)| {
            let (va, vb) = if axis == 0 { (a.0, b.0) } else { (a.1, b.1) };
            let t = (bound - va) / (vb - va);
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let mut next = Vec::with_capacity(pts.len() + 4);
        for i in 0..pts.len() {
            let cur = pts[i];
            let prev = pts[(i + pts.len() - 1) % pts.len()];
            match (inside(&prev), inside(&cur)) {
                (true, true) => next.push(cur),
                (true, false) => next.push(cross(prev, cur)),
                (false, true) => {
                    next.push(cross(prev, cur));
                    next.push(cur);
                }
                (false, false) => {}
            }
        }
        pts = next;
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::geometry::Rgba;

    fn shape(path: Vec<PathCmd>, fill: bool) -> Shape {
        Shape {
            path,
            fill: fill.then_some(Rgba::BLACK),
            stroke: Some((Rgba::BLACK, 2.0)),
            glow: 0.0,
            image: None,
        }
    }

    #[test]
    fn dedans_dehors_a_cheval() {
        let r = (0.0, 0.0, 100.0, 100.0);
        // Dedans : inchangée
        let mut v = vec![shape(vec![PathCmd::MoveTo(10.0, 10.0), PathCmd::LineTo(90.0, 90.0)], false)];
        clip_shapes(&mut v, r);
        assert_eq!(v[0].path, vec![PathCmd::MoveTo(10.0, 10.0), PathCmd::LineTo(90.0, 90.0)]);
        // Dehors : disparaît
        let mut v = vec![shape(vec![PathCmd::MoveTo(110.0, 10.0), PathCmd::LineTo(190.0, 90.0)], false)];
        clip_shapes(&mut v, r);
        assert!(v.is_empty());
        // À cheval : le trait s'arrête au bord
        let mut v = vec![shape(vec![PathCmd::MoveTo(50.0, 50.0), PathCmd::LineTo(150.0, 50.0)], false)];
        clip_shapes(&mut v, r);
        assert_eq!(v[0].path, vec![PathCmd::MoveTo(50.0, 50.0), PathCmd::LineTo(100.0, 50.0)]);
    }

    #[test]
    fn remplissage_et_trait_separes() {
        let r = (0.0, 0.0, 100.0, 100.0);
        let sq = vec![
            PathCmd::MoveTo(50.0, 50.0),
            PathCmd::LineTo(150.0, 50.0),
            PathCmd::LineTo(150.0, 80.0),
            PathCmd::LineTo(50.0, 80.0),
            PathCmd::Close,
        ];
        let mut v = vec![shape(sq, true)];
        clip_shapes(&mut v, r);
        assert_eq!(v.len(), 2);
        // Remplissage : rectangle 50..100 × 50..80
        let xs: Vec<f32> = v[0].path.iter().filter_map(|c| if let PathCmd::LineTo(x, _) | PathCmd::MoveTo(x, _) = c { Some(*x) } else { None }).collect();
        assert!(xs.iter().all(|x| *x <= 100.0) && xs.contains(&100.0));
        // Trait : deux morceaux (le bord du cadre n'est pas tracé)
        let moves = v[1].path.iter().filter(|c| matches!(c, PathCmd::MoveTo(..))).count();
        assert_eq!(moves, 2);
        assert!(v[1].fill.is_none() && v[0].stroke.is_none());
    }
}
