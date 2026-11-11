//! Transformations du plan et de l'espace appliquées à une figure.
//!
//! Une figure placée dans un repère (`axes : repere`, ou un objet 3D dans sa scène) peut être
//! déformée point par point, dans les coordonnées du repère :
//! - `matrix : (a, b, c, d)` : application linéaire (x, y) → (a·x + b·y, c·x + d·y)
//!   (9 nombres en 3D, ligne par ligne) ;
//! - `map : (x + y^2/4, y)` : fonction quelconque (x, y) → (u, v), ou (x, y, z) → (u, v, w) en 3D ;
//! - `cmap : z^2` : fonction complexe z → f(z), avec z = x + iy (applications conformes) ;
//! - `amount : 0.5` : avancement de la déformation (0 : figure d'origine, 1 : transformée).
//!   Avec un paramètre animé (`amount : t`), la figure se déforme progressivement.
//!
//! Les traits et contours sont redécoupés finement avant d'être transformés : une droite
//! devient une courbe si la transformation n'est pas linéaire.

use super::expr::{self, Expr, C};
use super::geometry::{AxesMap, PathCmd, Shape};
use super::three::{Item, Prim, V3};
use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;

/// Propriétés de transformation
pub const WARP_PROPS: &[&str] = &["matrix", "map", "cmap", "amount"];

pub enum Kind {
    Matrix(Vec<f64>),
    Map(Vec<Expr>),
    Complex(Expr),
}

pub struct Warp {
    kind: Kind,
    amount: f64,
    vars: HashMap<String, f64>,
}

/// Les éléments d'un tuple écrit `(a, b)` (déjà découpé par le parser) ou d'une valeur seule
fn items(v: &PropValue) -> Vec<String> {
    match v {
        PropValue::Tuple(items) | PropValue::List(items) => items.clone(),
        other => {
            let s = fmt_value(other);
            let s = s.trim();
            let inner = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')).unwrap_or(s);
            let call = format!("t({})", inner);
            super::geometry::split_call(&call).1.iter().map(|s| s.to_string()).collect()
        }
    }
}

/// Noms utilisables dans `map` : les paramètres, plus y (et z en 3D)
fn names(params: &HashMap<String, f64>, dims: usize) -> Vec<String> {
    let mut n: Vec<String> = params.keys().cloned().collect();
    n.push("y".to_string());
    if dims == 3 {
        n.push("z".to_string());
    }
    n
}

/// La transformation écrite sur la figure (None si elle n'en a pas). `dims` : 2 ou 3.
pub fn of(f: &ResolvedFigure, params: &HashMap<String, f64>, dims: usize) -> Result<Option<Warp>, String> {
    of_props(&f.properties, params, dims)
}

fn of_props(props: &HashMap<String, PropValue>, params: &HashMap<String, f64>, dims: usize) -> Result<Option<Warp>, String> {
    struct F<'a> {
        properties: &'a HashMap<String, PropValue>,
    }
    let f = F { properties: props };
    let given: Vec<&str> = ["matrix", "map", "cmap"].into_iter().filter(|k| f.properties.contains_key(*k)).collect();
    if given.is_empty() {
        if f.properties.contains_key("amount") {
            return Err("`amount` sans transformation : ajouter `matrix`, `map` ou `cmap`".to_string());
        }
        return Ok(None);
    }
    if given.len() > 1 {
        return Err(format!("une seule transformation par figure ({} données)", given.join(", ")));
    }
    let amount = match f.properties.get("amount") {
        None => 1.0,
        Some(PropValue::Number(n)) => *n,
        Some(v) => {
            let s = fmt_value(v);
            let e = expr::parse_with(&s, &params.keys().cloned().collect::<Vec<_>>())
                .map_err(|e| format!("amount '{}' : {}", s, e))?;
            e.eval(0.0, params)
        }
    };
    let pnames: Vec<String> = params.keys().cloned().collect();
    let kind = match given[0] {
        "matrix" => {
            let it = items(&f.properties["matrix"]);
            let want = dims * dims;
            if it.len() != want {
                let ex = if dims == 2 { "(a, b, c, d)" } else { "(a, b, c, d, e, f, g, h, i)" };
                return Err(format!("matrix : {} nombres attendus, ligne par ligne : {}", want, ex));
            }
            let mut m = Vec::new();
            for s in &it {
                let e = expr::parse_with(s, &pnames).map_err(|e| format!("matrix '{}' : {}", s, e))?;
                m.push(e.eval(0.0, params));
            }
            Kind::Matrix(m)
        }
        "map" => {
            let it = items(&f.properties["map"]);
            if it.len() != dims {
                let ex = if dims == 2 { "(u(x, y), v(x, y)), ex : (x + y, y)" } else { "(u, v, w) en x, y, z" };
                return Err(format!("map : {} expressions attendues : {}", dims, ex));
            }
            let n = names(params, dims);
            let mut es = Vec::new();
            for s in &it {
                es.push(expr::parse_with(s, &n).map_err(|e| format!("map '{}' : {}", s, e))?);
            }
            Kind::Map(es)
        }
        _ => {
            if dims != 2 {
                return Err("cmap (fonction complexe) ne s'applique qu'au plan".to_string());
            }
            let s = fmt_value(&f.properties["cmap"]);
            Kind::Complex(expr::parse_complex(&s, &pnames).map_err(|e| format!("cmap '{}' : {}", s, e))?)
        }
    };
    Ok(Some(Warp { kind, amount, vars: params.clone() }))
}

/// Vérification (validateur) : les formules se lisent, avec les bons noms
pub fn check(props: &HashMap<String, PropValue>, params: &[String], dims: usize) -> Result<(), String> {
    let vals: HashMap<String, f64> = params.iter().map(|p| (p.clone(), 1.0)).collect();
    of_props(props, &vals, dims).map(|_| ())
}


/// Chemin → lignes brisées (fermées ou non). Les segments plus longs que `step` pixels sont
/// redécoupés ; les courbes de Bézier sont échantillonnées (12 morceaux).
pub fn flatten(path: &[PathCmd], step: f32) -> Vec<(Vec<(f32, f32)>, bool)> {
    let mut polys: Vec<(Vec<(f32, f32)>, bool)> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let mut last = (0.0f32, 0.0f32);
    let push_seg = |cur: &mut Vec<(f32, f32)>, a: (f32, f32), b: (f32, f32)| {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let n = ((len / step).ceil() as usize).clamp(1, 400);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            cur.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    };
    for cmd in path {
        match *cmd {
            PathCmd::MoveTo(x, y) => {
                if cur.len() > 1 {
                    polys.push((std::mem::take(&mut cur), false));
                }
                cur = vec![(x, y)];
                last = (x, y);
            }
            PathCmd::LineTo(x, y) => {
                if cur.is_empty() {
                    cur.push(last);
                }
                push_seg(&mut cur, last, (x, y));
                last = (x, y);
            }
            PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                if cur.is_empty() {
                    cur.push(last);
                }
                let p0 = last;
                let mut prev = p0;
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    let q = (
                        u * u * u * p0.0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x,
                        u * u * u * p0.1 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y,
                    );
                    push_seg(&mut cur, prev, q);
                    prev = q;
                }
                last = (x, y);
            }
            PathCmd::Close => {
                if let Some(&first) = cur.first() {
                    push_seg(&mut cur, last, first);
                    last = first;
                }
                polys.push((std::mem::take(&mut cur), true));
            }
        }
    }
    if cur.len() > 1 {
        polys.push((cur, false));
    }
    polys
}

impl Warp {
    /// Image d'un point (coordonnées du repère)
    pub fn apply(&self, p: V3) -> V3 {
        let q = match &self.kind {
            Kind::Matrix(m) if m.len() == 4 => [m[0] * p[0] + m[1] * p[1], m[2] * p[0] + m[3] * p[1], p[2]],
            Kind::Matrix(m) => [
                m[0] * p[0] + m[1] * p[1] + m[2] * p[2],
                m[3] * p[0] + m[4] * p[1] + m[5] * p[2],
                m[6] * p[0] + m[7] * p[1] + m[8] * p[2],
            ],
            Kind::Map(es) => {
                let mut vars = self.vars.clone();
                vars.insert("y".to_string(), p[1]);
                vars.insert("z".to_string(), p[2]);
                let u = es[0].eval(p[0], &vars);
                let v = es[1].eval(p[0], &vars);
                let w = es.get(2).map(|e| e.eval(p[0], &vars)).unwrap_or(p[2]);
                [u, v, w]
            }
            Kind::Complex(e) => {
                let r = e.eval_c(C(p[0], p[1]), &self.vars);
                [r.0, r.1, p[2]]
            }
        };
        let a = self.amount;
        if a == 1.0 {
            return q;
        }
        [p[0] + a * (q[0] - p[0]), p[1] + a * (q[1] - p[1]), p[2] + a * (q[2] - p[2])]
    }

    fn apply2(&self, x: f64, y: f64) -> (f64, f64) {
        let q = self.apply([x, y, 0.0]);
        (q[0], q[1])
    }

    /// Transforme des formes 2D dessinées dans le repère `m` (pixels → repère → image → pixels)
    pub fn shapes(&self, shapes: &mut [Shape], m: &AxesMap) {
        // Longueur maximale d'un morceau, en pixels, avant transformation
        const STEP_PX: f32 = 4.0;
        let to_data = |px: f32, py: f32| (((px - m.ox) / m.ux) as f64, ((m.oy - py) / m.uy) as f64);
        for s in shapes.iter_mut().filter(|s| s.image.is_none()) {
            // 1. Ligne brisée fine (les courbes de Bézier sont échantillonnées)
            let polys = flatten(&s.path, STEP_PX);

            // 2. Transformation ; un point non défini coupe le trait
            let mut path = Vec::new();
            for (poly, closed) in polys {
                let mut pen_down = false;
                let mut all_ok = true;
                for (px, py) in poly {
                    let (x, y) = to_data(px, py);
                    let (u, v) = self.apply2(x, y);
                    if !(u.is_finite() && v.is_finite()) || u.abs() > 1e6 || v.abs() > 1e6 {
                        pen_down = false;
                        all_ok = false;
                        continue;
                    }
                    let (qx, qy) = m.to_px(u, v);
                    path.push(if pen_down { PathCmd::LineTo(qx, qy) } else { PathCmd::MoveTo(qx, qy) });
                    pen_down = true;
                }
                if closed && all_ok {
                    path.push(PathCmd::Close);
                }
            }
            s.path = path;
        }
    }

    /// Transforme les morceaux d'un objet 3D (coordonnées de sa scène)
    pub fn items(&self, items: &mut Vec<Item>) {
        const N: usize = 10;
        let lerp = |a: V3, b: V3, t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        let ok = |p: &V3| p.iter().all(|c| c.is_finite());
        let mut out = Vec::with_capacity(items.len());
        for it in items.drain(..) {
            let Item { prim, order, glow } = it;
            let mut push = |prim: Prim| out.push(Item { prim, order, glow });
            match prim {
                Prim::Seg { a, b, color, width, tube } => {
                    let pts: Vec<V3> = (0..=N).map(|i| self.apply(lerp(a, b, i as f64 / N as f64))).collect();
                    if pts.iter().all(ok) {
                        push(Prim::Line { pts, color, width, tube });
                    }
                }
                Prim::Line { pts, color, width, tube } => {
                    let mut fine = vec![self.apply(pts[0])];
                    for w in pts.windows(2) {
                        for i in 1..=N {
                            fine.push(self.apply(lerp(w[0], w[1], i as f64 / N as f64)));
                        }
                    }
                    if fine.iter().all(ok) {
                        push(Prim::Line { pts: fine, color, width, tube });
                    }
                }
                Prim::Face { pts, color, shade } if pts.len() == 4 => {
                    // Quadrilatère : découpé en N × N petits quadrilatères (bilinéaire)
                    let at = |s: f64, t: f64| lerp(lerp(pts[0], pts[1], s), lerp(pts[3], pts[2], s), t);
                    let n = if self.is_linear() { 1 } else { N };
                    for i in 0..n {
                        for j in 0..n {
                            let (s0, s1, t0, t1) =
                                (i as f64 / n as f64, (i + 1) as f64 / n as f64, j as f64 / n as f64, (j + 1) as f64 / n as f64);
                            let q: Vec<V3> = [at(s0, t0), at(s1, t0), at(s1, t1), at(s0, t1)].iter().map(|p| self.apply(*p)).collect();
                            if q.iter().all(ok) {
                                push(Prim::Face { pts: q, color, shade });
                            }
                        }
                    }
                }
                Prim::Face { pts, color, shade } if pts.len() == 3 && !self.is_linear() => {
                    // Triangle : découpé en N² petits triangles
                    let (a, b, c) = (pts[0], pts[1], pts[2]);
                    let at = |i: usize, j: usize| {
                        let (u, v) = (i as f64 / N as f64, j as f64 / N as f64);
                        [
                            a[0] + (b[0] - a[0]) * u + (c[0] - a[0]) * v,
                            a[1] + (b[1] - a[1]) * u + (c[1] - a[1]) * v,
                            a[2] + (b[2] - a[2]) * u + (c[2] - a[2]) * v,
                        ]
                    };
                    for i in 0..N {
                        for j in 0..N - i {
                            let tri = [at(i, j), at(i + 1, j), at(i, j + 1)];
                            let q: Vec<V3> = tri.iter().map(|p| self.apply(*p)).collect();
                            if q.iter().all(ok) {
                                push(Prim::Face { pts: q, color, shade });
                            }
                            if i + j + 1 < N {
                                let tri = [at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                                let q: Vec<V3> = tri.iter().map(|p| self.apply(*p)).collect();
                                if q.iter().all(ok) {
                                    push(Prim::Face { pts: q, color, shade });
                                }
                            }
                        }
                    }
                }
                Prim::Face { pts, color, shade } => {
                    let q: Vec<V3> = pts.iter().map(|p| self.apply(*p)).collect();
                    if q.iter().all(ok) {
                        push(Prim::Face { pts: q, color, shade });
                    }
                }
                Prim::Dot { p, color, radius } => {
                    let p = self.apply(p);
                    if ok(&p) {
                        push(Prim::Dot { p, color, radius });
                    }
                }
                Prim::Label { p, tex, size, color } => {
                    let p = self.apply(p);
                    if ok(&p) {
                        push(Prim::Label { p, tex, size, color });
                    }
                }
            }
        }
        *items = out;
    }

    /// Transformation linéaire : les faces planes le restent, pas besoin de les découper
    fn is_linear(&self) -> bool {
        matches!(self.kind, Kind::Matrix(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fig(props: &[(&str, PropValue)]) -> ResolvedFigure {
        let mut f = ResolvedFigure {
            name: "w".into(),
            index: 0,
            ftype: "grid".into(),
            layer: 0.0,
            line: 1,
            properties: HashMap::new(),
            prop_lines: HashMap::new(),
        };
        for (k, v) in props {
            f.properties.insert(k.to_string(), v.clone());
        }
        f
    }

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn matrice_fonction_complexe() {
        let params: HashMap<String, f64> = [("t".to_string(), 0.5)].into();
        let tuple = |v: &[&str]| PropValue::Tuple(v.iter().map(|s| s.to_string()).collect());
        let w = of(&fig(&[("matrix", tuple(&["2", "1", "0", "1"]))]), &params, 2).unwrap().unwrap();
        assert!(close(w.apply([1.0, 2.0, 0.0]), [4.0, 2.0, 0.0]));
        let w = of(&fig(&[("map", tuple(&["x + y^2", "-y"]))]), &params, 2).unwrap().unwrap();
        assert!(close(w.apply([1.0, 2.0, 0.0]), [5.0, -2.0, 0.0]));
        let w = of(&fig(&[("cmap", PropValue::Single("z^2".into()))]), &params, 2).unwrap().unwrap();
        assert!(close(w.apply([1.0, 1.0, 0.0]), [0.0, 2.0, 0.0]));
        // amount : à mi-chemin
        let w = of(&fig(&[("matrix", tuple(&["3", "0", "0", "1"])), ("amount", PropValue::Single("t".into()))]), &params, 2)
            .unwrap()
            .unwrap();
        assert!(close(w.apply([1.0, 1.0, 0.0]), [2.0, 1.0, 0.0]));
        // 3D
        let w = of(&fig(&[("map", tuple(&["x", "y", "z + x*y"]))]), &params, 3).unwrap().unwrap();
        assert!(close(w.apply([2.0, 3.0, 1.0]), [2.0, 3.0, 7.0]));
    }

    #[test]
    fn erreurs() {
        let params = HashMap::new();
        let tuple = |v: &[&str]| PropValue::Tuple(v.iter().map(|s| s.to_string()).collect());
        let e = |props: &[(&str, PropValue)], d| of(&fig(props), &params, d).err().unwrap();
        assert!(e(&[("matrix", tuple(&["1", "2", "3"]))], 2).contains("4 nombres"));
        assert!(e(&[("map", tuple(&["x"]))], 2).contains("2 expressions"));
        assert!(e(&[("map", tuple(&["x", "w"]))], 2).contains("nom inconnu"));
        assert!(e(&[("cmap", PropValue::Single("z".into()))], 3).contains("plan"));
        assert!(e(&[("amount", PropValue::Number(1.0))], 2).contains("sans transformation"));
        assert!(e(&[("cmap", PropValue::Single("z".into())), ("matrix", tuple(&["1", "0", "0", "1"]))], 2).contains("une seule"));
    }

    #[test]
    fn segments_courbes() {
        let params = HashMap::new();
        let w = of(&fig(&[("cmap", PropValue::Single("z^2".into()))]), &params, 2).unwrap().unwrap();
        let m = AxesMap { ox: 0.0, oy: 100.0, ux: 10.0, uy: 10.0, xrange: (-5.0, 5.0), yrange: (-5.0, 5.0), step: (1.0, 1.0) };
        // Segment vertical x = 1, de y = -1 à 1 : devient un arc de parabole
        let (a, b) = (m.to_px(1.0, -1.0), m.to_px(1.0, 1.0));
        let mut shapes = vec![Shape { path: vec![PathCmd::MoveTo(a.0, a.1), PathCmd::LineTo(b.0, b.1)], fill: None, stroke: None, glow: 0.0, image: None }];
        w.shapes(&mut shapes, &m);
        assert!(shapes[0].path.len() > 4);
        // Milieu (1, 0) → (1, 0) ; extrémités (1, ±1) → (0, ±2)
        let to_data = |x: f32, y: f32| ((x - m.ox) / m.ux, (m.oy - y) / m.uy);
        let PathCmd::MoveTo(x, y) = shapes[0].path[0] else { panic!() };
        let (u, v) = to_data(x, y);
        assert!(u.abs() < 1e-3 && (v + 2.0).abs() < 1e-3, "{} {}", u, v);
    }
}
