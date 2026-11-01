//! `transform(to=b)` : déformation progressive des formes d'une figure vers celles d'une autre.
//!
//! Méthode :
//! 1. chaque sous-chemin (un contour) est découpé en N points régulièrement espacés ;
//! 2. les formes, puis les contours, sont associés dans l'ordre ; s'il en manque d'un côté,
//!    on part (ou on arrive) d'un point au centre de l'autre figure ;
//! 3. pour un contour fermé, on choisit le point de départ qui minimise le trajet total
//!    (sinon la forme « tourne » sur elle-même pendant la déformation) ;
//! 4. chaque point va en ligne droite de sa position de départ à sa position d'arrivée,
//!    et les couleurs et épaisseurs sont interpolées.

use super::geometry::{PathCmd, Rgba, Shape};

/// Points par contour : assez pour qu'un grand cercle reste rond
const N: usize = 96;

type Pt = (f32, f32);

/// Un contour découpé en N points
#[derive(Clone)]
struct Contour {
    pts: Vec<Pt>,
    closed: bool,
}

/// Formes intermédiaires entre `a` (e = 0) et `b` (e = 1)
pub fn morph(a: &[Shape], b: &[Shape], e: f32) -> Vec<Shape> {
    let center_a = center(a);
    let center_b = center(b);
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (sa, sb) = (a.get(i), b.get(i));
        let ca = sa.map(|s| contours(&s.path)).unwrap_or_default();
        let cb = sb.map(|s| contours(&s.path)).unwrap_or_default();
        let m = ca.len().max(cb.len());
        let mut path = Vec::new();
        for k in 0..m {
            let from = ca.get(k).cloned().unwrap_or_else(|| point_contour(center_of(sa, center_a), cb[k].closed));
            let to = cb.get(k).cloned().unwrap_or_else(|| point_contour(center_of(sb, center_b), ca[k].closed));
            let to = align(&from, &to);
            let closed = if e < 0.5 { from.closed } else { to.closed };
            for (j, (p, q)) in from.pts.iter().zip(&to.pts).enumerate() {
                let x = p.0 + (q.0 - p.0) * e;
                let y = p.1 + (q.1 - p.1) * e;
                path.push(if j == 0 { PathCmd::MoveTo(x, y) } else { PathCmd::LineTo(x, y) });
            }
            if closed {
                path.push(PathCmd::Close);
            }
        }
        let fill = mix_opt(sa.and_then(|s| s.fill), sb.and_then(|s| s.fill), e);
        let stroke = match (sa.and_then(|s| s.stroke), sb.and_then(|s| s.stroke)) {
            (None, None) => None,
            (x, y) => {
                let (cx, wx) = x.map(|(c, w)| (Some(c), w)).unwrap_or((None, 0.0));
                let (cy, wy) = y.map(|(c, w)| (Some(c), w)).unwrap_or((None, 0.0));
                mix_opt(cx, cy, e).map(|c| (c, wx + (wy - wx) * e))
            }
        };
        out.push(Shape { path, fill, stroke });
    }
    out
}

/// Couleur intermédiaire ; une couleur absente = la même, transparente
fn mix_opt(a: Option<Rgba>, b: Option<Rgba>, e: f32) -> Option<Rgba> {
    let (a, b) = match (a, b) {
        (None, None) => return None,
        (Some(a), None) => (a, Rgba { a: 0.0, ..a }),
        (None, Some(b)) => (Rgba { a: 0.0, ..b }, b),
        (Some(a), Some(b)) => (a, b),
    };
    Some(Rgba {
        r: a.r + (b.r - a.r) * e,
        g: a.g + (b.g - a.g) * e,
        b: a.b + (b.b - a.b) * e,
        a: a.a + (b.a - a.a) * e,
    })
}

fn center_of(shape: Option<&Shape>, fallback: Pt) -> Pt {
    shape.map(|s| center(std::slice::from_ref(s))).unwrap_or(fallback)
}

/// Centre de la boîte englobante des points
fn center(shapes: &[Shape]) -> Pt {
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for s in shapes {
        for cmd in &s.path {
            if let PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) | PathCmd::CubicTo(_, _, _, _, x, y) = *cmd {
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
        }
    }
    if b.0 > b.2 { (0.0, 0.0) } else { ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0) }
}

fn point_contour(p: Pt, closed: bool) -> Contour {
    Contour { pts: vec![p; N], closed }
}

/// Contours d'un chemin, découpés en N points régulièrement espacés
fn contours(path: &[PathCmd]) -> Vec<Contour> {
    let mut out = Vec::new();
    let mut poly: Vec<Pt> = Vec::new();
    let mut closed = false;
    let finish = |poly: &mut Vec<Pt>, closed: bool, out: &mut Vec<Contour>| {
        if !poly.is_empty() {
            out.push(Contour { pts: resample(poly, closed), closed });
        }
        poly.clear();
    };
    for cmd in path {
        match *cmd {
            PathCmd::MoveTo(x, y) => {
                finish(&mut poly, closed, &mut out);
                closed = false;
                poly.push((x, y));
            }
            PathCmd::LineTo(x, y) => poly.push((x, y)),
            PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                let p0 = *poly.last().unwrap_or(&(x, y));
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    poly.push((
                        u * u * u * p0.0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x,
                        u * u * u * p0.1 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y,
                    ));
                }
            }
            PathCmd::Close => closed = true,
        }
    }
    finish(&mut poly, closed, &mut out);
    out
}

/// N points régulièrement espacés le long de la ligne brisée (en la refermant si `closed`)
fn resample(poly: &[Pt], closed: bool) -> Vec<Pt> {
    let mut pts = poly.to_vec();
    if closed && pts.len() > 1 && pts.first() != pts.last() {
        pts.push(pts[0]);
    }
    let seg = |i: usize| ((pts[i + 1].0 - pts[i].0).powi(2) + (pts[i + 1].1 - pts[i].1).powi(2)).sqrt();
    let total: f32 = (0..pts.len().saturating_sub(1)).map(seg).sum();
    if pts.len() < 2 || total <= 0.0 {
        return vec![pts[0]; N];
    }
    // un contour fermé : N points sans répéter le premier ; ouvert : du début à la fin
    let step = if closed { total / N as f32 } else { total / (N - 1) as f32 };
    let mut out = Vec::with_capacity(N);
    let (mut i, mut walked) = (0, 0.0);
    for k in 0..N {
        let target = step * k as f32;
        while i < pts.len() - 2 && walked + seg(i) < target {
            walked += seg(i);
            i += 1;
        }
        let l = seg(i);
        let t = if l > 0.0 { ((target - walked) / l).clamp(0.0, 1.0) } else { 0.0 };
        out.push((pts[i].0 + (pts[i + 1].0 - pts[i].0) * t, pts[i].1 + (pts[i + 1].1 - pts[i].1) * t));
    }
    out
}

/// Contour d'arrivée, avec le point de départ qui minimise le trajet (contours fermés)
fn align(from: &Contour, to: &Contour) -> Contour {
    if !(from.closed && to.closed) {
        return to.clone();
    }
    let cost = |shift: usize| -> f32 {
        (0..N)
            .map(|j| {
                let (p, q) = (from.pts[j], to.pts[(j + shift) % N]);
                (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)
            })
            .sum()
    };
    let best = (0..N).min_by(|&a, &b| cost(a).partial_cmp(&cost(b)).unwrap()).unwrap_or(0);
    Contour { pts: (0..N).map(|j| to.pts[(j + best) % N]).collect(), closed: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, s: f32, color: Rgba) -> Shape {
        Shape {
            path: vec![
                PathCmd::MoveTo(x, y),
                PathCmd::LineTo(x + s, y),
                PathCmd::LineTo(x + s, y + s),
                PathCmd::LineTo(x, y + s),
                PathCmd::Close,
            ],
            fill: Some(color),
            stroke: None,
        }
    }

    #[test]
    fn mi_chemin() {
        let a = [square(0.0, 0.0, 10.0, Rgba::BLACK)];
        let b = [square(100.0, 0.0, 10.0, Rgba::WHITE)];
        let m = morph(&a, &b, 0.5);
        assert_eq!(m.len(), 1);
        // à mi-chemin : centré en x = 55, couleur grise
        assert!((center(&m).0 - 55.0).abs() < 0.5);
        assert!((m[0].fill.unwrap().r - 0.5).abs() < 1e-5);
        // aux extrémités : les points de départ / d'arrivée
        assert!((center(&morph(&a, &b, 1.0)).0 - 105.0).abs() < 0.5);
    }

    #[test]
    fn nombres_de_formes_differents() {
        let a = [square(0.0, 0.0, 10.0, Rgba::BLACK)];
        let b = [square(0.0, 0.0, 10.0, Rgba::BLACK), square(50.0, 0.0, 10.0, Rgba::BLACK)];
        let m = morph(&a, &b, 0.0);
        assert_eq!(m.len(), 2);
        // la forme en trop part d'un point, transparente
        assert_eq!(m[1].fill.unwrap().a, 0.0);
    }
}
