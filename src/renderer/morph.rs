//! `transform(to=b)` : passage progressif des formes d'une figure à celles d'une autre.
//!
//! Deux méthodes :
//! - **texte et équations** (`morph_glyphs`) : les lettres identiques sont reconnues et glissent
//!   à leur nouvelle place sans se déformer ; les autres apparaissent ou disparaissent en fondu ;
//! - **formes** (`morph`) : déformation point par point, décrite ci-dessous.
//!
//! Méthode de `morph` :
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

// ─────────────────────────────────────────────
//  LETTRE PAR LETTRE (texte et équations)
// ─────────────────────────────────────────────

/// Écart maximal (en part de la taille de la lettre) entre deux contours de la même lettre
const SAME_GLYPH_TOLERANCE: f32 = 0.01;

/// Points d'un chemin, dans l'ordre (points de contrôle compris)
fn points(path: &[PathCmd]) -> Vec<Pt> {
    let mut pts = Vec::with_capacity(path.len());
    for cmd in path {
        match *cmd {
            PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) => pts.push((x, y)),
            PathCmd::CubicTo(a, b, c, d, x, y) => pts.extend([(a, b), (c, d), (x, y)]),
            PathCmd::Close => {}
        }
    }
    pts
}

/// Même suite de commandes (MoveTo, LineTo, ...)
fn same_structure(a: &[PathCmd], b: &[PathCmd]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| std::mem::discriminant(p) == std::mem::discriminant(q))
}

/// Contour ramené à une taille 1, en haut à gauche : ne dépend ni de la position ni de la taille
fn normalized(shape: &Shape) -> Vec<Pt> {
    let pts = points(&shape.path);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &pts {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let size = (x1 - x0).max(y1 - y0).max(1e-6);
    pts.iter().map(|&(x, y)| ((x - x0) / size, (y - y0) / size)).collect()
}

/// Vrai si deux formes sont le même dessin, quelles que soient leur position et leur taille
fn same_glyph(a: &Shape, a_pts: &[Pt], b: &Shape, b_pts: &[Pt]) -> bool {
    same_structure(&a.path, &b.path)
        && a_pts
            .iter()
            .zip(b_pts)
            .all(|(p, q)| (p.0 - q.0).abs() < SAME_GLYPH_TOLERANCE && (p.1 - q.1).abs() < SAME_GLYPH_TOLERANCE)
}

/// Associe les lettres identiques de `a` et `b` : paires (indice dans a, indice dans b).
/// 1. plus longue sous-suite commune : ce qui reste dans le même ordre ;
/// 2. les lettres restantes identiques sont associées dans l'ordre (un terme passé de l'autre
///    côté du signe = se déplace au lieu de s'effacer).
fn match_glyphs(a: &[Shape], b: &[Shape]) -> Vec<(usize, usize)> {
    let na: Vec<Vec<Pt>> = a.iter().map(normalized).collect();
    let nb: Vec<Vec<Pt>> = b.iter().map(normalized).collect();
    let same = |i: usize, j: usize| same_glyph(&a[i], &na[i], &b[j], &nb[j]);

    // lcs[i][j] = longueur de la plus longue sous-suite commune de a[i..] et b[j..]
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if same(i, j) { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if same(i, j) && lcs[i][j] == lcs[i + 1][j + 1] + 1 {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    let mut used_a: Vec<bool> = vec![false; a.len()];
    let mut used_b: Vec<bool> = vec![false; b.len()];
    for &(i, j) in &pairs {
        used_a[i] = true;
        used_b[j] = true;
    }
    for i in 0..a.len() {
        if used_a[i] {
            continue;
        }
        if let Some(j) = (0..b.len()).find(|&j| !used_b[j] && same(i, j)) {
            pairs.push((i, j));
            used_a[i] = true;
            used_b[j] = true;
        }
    }
    pairs
}

/// Forme à l'opacité multipliée par `k`
fn faded(shape: &Shape, k: f32) -> Shape {
    let mut s = shape.clone();
    if let Some(c) = &mut s.fill {
        c.a *= k;
    }
    if let Some((c, _)) = &mut s.stroke {
        c.a *= k;
    }
    s
}

/// Formes intermédiaires entre deux textes ou équations : les lettres communes se déplacent,
/// celles de `a` sans équivalent s'effacent, celles de `b` sans équivalent apparaissent.
pub fn morph_glyphs(a: &[Shape], b: &[Shape], e: f32) -> Vec<Shape> {
    let pairs = match_glyphs(a, b);
    let mut matched_a = vec![false; a.len()];
    let mut matched_b = vec![false; b.len()];
    let mut out = Vec::with_capacity(a.len() + b.len());

    for &(i, j) in &pairs {
        matched_a[i] = true;
        matched_b[j] = true;
        // Même structure : chaque point va en ligne droite vers le point correspondant
        let mix = |p: f32, q: f32| p + (q - p) * e;
        let path = a[i]
            .path
            .iter()
            .zip(&b[j].path)
            .map(|(p, q)| match (p.clone(), q.clone()) {
                (PathCmd::MoveTo(x, y), PathCmd::MoveTo(u, v)) => PathCmd::MoveTo(mix(x, u), mix(y, v)),
                (PathCmd::LineTo(x, y), PathCmd::LineTo(u, v)) => PathCmd::LineTo(mix(x, u), mix(y, v)),
                (PathCmd::CubicTo(x1, y1, x2, y2, x, y), PathCmd::CubicTo(u1, v1, u2, v2, u, v)) => {
                    PathCmd::CubicTo(mix(x1, u1), mix(y1, v1), mix(x2, u2), mix(y2, v2), mix(x, u), mix(y, v))
                }
                (other, _) => other,
            })
            .collect();
        let stroke = match (a[i].stroke, b[j].stroke) {
            (None, None) => None,
            (x, y) => {
                let (cx, wx) = x.map(|(c, w)| (Some(c), w)).unwrap_or((None, 0.0));
                let (cy, wy) = y.map(|(c, w)| (Some(c), w)).unwrap_or((None, 0.0));
                mix_opt(cx, cy, e).map(|c| (c, mix(wx, wy)))
            }
        };
        out.push(Shape { path, fill: mix_opt(a[i].fill, b[j].fill, e), stroke });
    }
    out.extend(a.iter().zip(&matched_a).filter(|(_, m)| !**m).map(|(s, _)| faded(s, 1.0 - e)));
    out.extend(b.iter().zip(&matched_b).filter(|(_, m)| !**m).map(|(s, _)| faded(s, e)));
    out
}

/// Indices dans `shapes` des morceaux identiques à la suite `part`, lettre par lettre.
/// Toutes les occurrences comptent ; vide si `part` n'y est pas.
pub fn find_part(shapes: &[Shape], part: &[Shape]) -> Vec<usize> {
    let mut found = Vec::new();
    if part.is_empty() {
        return found;
    }
    let ns: Vec<Vec<Pt>> = shapes.iter().map(normalized).collect();
    let np: Vec<Vec<Pt>> = part.iter().map(normalized).collect();
    let mut i = 0;
    while i + part.len() <= shapes.len() {
        if (0..part.len()).all(|k| same_glyph(&shapes[i + k], &ns[i + k], &part[k], &np[k])) {
            found.extend(i..i + part.len());
            i += part.len();
        } else {
            i += 1;
        }
    }
    found
}

// ─────────────────────────────────────────────
//  FORMES
// ─────────────────────────────────────────────

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

    /// Formes d'une formule LaTeX (taille 40, centrée en x)
    fn formula(tex: &str, x: f32) -> Vec<Shape> {
        use crate::renderer::typeset::{shapes, Align, Content};
        shapes(&Content::Math(tex.into()), x, 100.0, 40.0, Align::Center, Rgba::BLACK).unwrap()
    }

    #[test]
    fn lettres_communes_reconnues() {
        let a = formula(r"T(x) = x - rac{x^3}{6}", 300.0);
        let b = formula(r"T(x) = x - rac{x^3}{6} + rac{x^5}{120}", 300.0);
        // Tout ce qui existe dans a existe dans b, sauf la barre de fraction si elle diffère
        let pairs = match_glyphs(&a, &b);
        assert_eq!(pairs.len(), a.len());
        let m = morph_glyphs(&a, &b, 0.0);
        assert_eq!(m.len(), b.len());
        // Au début : les nouveaux morceaux sont invisibles ; à la fin : tout est là
        assert_eq!(m.iter().filter(|s| s.fill.is_some_and(|c| c.a == 0.0)).count(), b.len() - a.len());
        let end = morph_glyphs(&a, &b, 1.0);
        assert_eq!(end.iter().filter(|s| s.fill.is_some_and(|c| c.a > 0.99)).count(), b.len());
    }

    #[test]
    fn un_terme_change_de_cote() {
        let a = formula(r"a + b = c", 300.0);
        let b = formula(r"a = c - b", 300.0);
        // a, b, =, c se retrouvent ; seuls + et - changent
        assert_eq!(match_glyphs(&a, &b).len(), 4);
    }

    #[test]
    fn meme_lettre_a_une_autre_taille() {
        use crate::renderer::typeset::{shapes, Align, Content};
        let a = formula("x", 100.0);
        let b = shapes(&Content::Math("x".into()), 400.0, 300.0, 90.0, Align::Center, Rgba::BLACK).unwrap();
        assert_eq!(match_glyphs(&a, &b).len(), 1);
    }

    #[test]
    fn retrouver_un_terme() {
        let f = formula(r"x - rac{x^3}{6} + rac{x^5}{120}", 300.0);
        let part = formula(r"rac{x^5}{120}", 0.0);
        let found = find_part(&f, &part);
        assert_eq!(found.len(), part.len());
        assert!(found.iter().all(|&i| i >= f.len() - part.len()), "{:?}", found);
        assert!(find_part(&f, &formula("z", 0.0)).is_empty());
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
