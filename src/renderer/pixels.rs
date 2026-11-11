//! `pixels` : une couleur par pixel, dans un repère 2D.
//!
//! Trois façons de la donner (une seule par figure) :
//! - `expr : f(x, y)` → couleur de `colormap` selon la valeur (carte de chaleur) ;
//! - `domain : f(z)` → coloration de domaine : teinte = argument de f(z), clarté selon |f(z)|
//!   (zéros noirs, pôles blancs, bandes aux puissances de 2 du module) ;
//! - `escape : z^2 + c` → fractale par temps de fuite : z part de `z0` (défaut 0 ;
//!   `z0 : c` pour un ensemble de Julia), c est le point du pixel ; couleur selon le nombre
//!   d'itérations avant |z| > `bailout`, `inside` pour les points qui ne s'échappent pas.
//!
//! Les formules sont compilées en fonctions (pas d'arbre parcouru à chaque pixel) et le calcul
//! est réparti sur les cœurs. Résultat gardé d'une image à l'autre tant que rien ne change.

use super::expr::{self, call_c, call_real, Expr, C};
use super::geometry::{AxesMap, PathCmd, Raster, Refs, Rgba, Scene, Shape};
use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;
use std::sync::Arc;

type RealFn = Box<dyn Fn(f64, f64) -> f64 + Send + Sync>;

thread_local! {
    /// Géométrie seulement (`--check`) : le cadre, sans calculer les couleurs
    static GEOMETRY_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Active ou coupe le calcul des couleurs (vérification de mise en page : inutile)
pub fn set_geometry_only(on: bool) {
    GEOMETRY_ONLY.with(|g| g.set(on));
}

pub fn geometry_only() -> bool {
    GEOMETRY_ONLY.with(|g| g.get())
}
type ComplexFn = Box<dyn Fn(C, C) -> C + Send + Sync>;

/// f(x, y) réelle compilée ; les paramètres deviennent des constantes
fn compile_real(e: &Expr, params: &HashMap<String, f64>) -> Result<RealFn, String> {
    Ok(match e {
        Expr::Num(n) => {
            let n = *n;
            Box::new(move |_, _| n)
        }
        Expr::X => Box::new(|x, _| x),
        Expr::Var(v) if v == "y" => Box::new(|_, y| y),
        Expr::Var(v) => {
            let n = params.get(v).copied().unwrap_or(f64::NAN);
            Box::new(move |_, _| n)
        }
        Expr::Neg(a) => {
            let a = compile_real(a, params)?;
            Box::new(move |x, y| -a(x, y))
        }
        Expr::Bin(op, a, b) => {
            let (a, b) = (compile_real(a, params)?, compile_real(b, params)?);
            match op {
                '+' => Box::new(move |x, y| a(x, y) + b(x, y)),
                '-' => Box::new(move |x, y| a(x, y) - b(x, y)),
                '*' => Box::new(move |x, y| a(x, y) * b(x, y)),
                '/' => Box::new(move |x, y| a(x, y) / b(x, y)),
                '^' => Box::new(move |x, y| a(x, y).powf(b(x, y))),
                op => {
                    let op = *op;
                    Box::new(move |x, y| {
                        let (u, v) = (a(x, y), b(x, y));
                        let r = match op {
                            '<' => u < v,
                            '>' => u > v,
                            'l' => u <= v,
                            'g' => u >= v,
                            '=' => u == v,
                            _ => u != v,
                        };
                        if r { 1.0 } else { 0.0 }
                    })
                }
            }
        }
        Expr::Call(name, args) => {
            let fs = args.iter().map(|a| compile_real(a, params)).collect::<Result<Vec<_>, _>>()?;
            let name: &'static str = name;
            if name == "if" {
                let mut it = fs.into_iter();
                let (c, a, b) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
                Box::new(move |x, y| {
                    let k = c(x, y);
                    if k != 0.0 && !k.is_nan() { a(x, y) } else { b(x, y) }
                })
            } else if fs.len() == 1 {
                let a = fs.into_iter().next().unwrap();
                Box::new(move |x, y| call_real(name, &[a(x, y)]))
            } else {
                Box::new(move |x, y| {
                    let v: Vec<f64> = fs.iter().map(|f| f(x, y)).collect();
                    call_real(name, &v)
                })
            }
        }
        // Somme, produit : évaluation ordinaire (plus lente), avec y passé en variable
        e @ Expr::Sum { .. } => {
            let e = e.clone();
            let params = params.clone();
            Box::new(move |x, y| {
                let mut vars = params.clone();
                vars.insert("y".to_string(), y);
                e.eval(x, &vars)
            })
        }
        Expr::I | Expr::Bound(_) => return Err("i n'existe pas en réel".to_string()),
    })
}

/// f(z, c) complexe compilée
fn compile_complex(e: &Expr, params: &HashMap<String, f64>) -> Result<ComplexFn, String> {
    Ok(match e {
        Expr::Num(n) => {
            let n = C(*n, 0.0);
            Box::new(move |_, _| n)
        }
        Expr::X => Box::new(|z, _| z),
        Expr::I => Box::new(|_, _| C(0.0, 1.0)),
        Expr::Var(v) if v == "c" => Box::new(|_, c| c),
        Expr::Var(v) => {
            let n = C(params.get(v).copied().unwrap_or(f64::NAN), 0.0);
            Box::new(move |_, _| n)
        }
        Expr::Neg(a) => {
            let a = compile_complex(a, params)?;
            Box::new(move |z, c| {
                let v = a(z, c);
                C(-v.0, -v.1)
            })
        }
        Expr::Bin(op, a, b) => {
            // Puissance entière fixe : multiplications directes (z^2 : le cas des fractales)
            if *op == '^' {
                if let Expr::Num(n) = **b {
                    if n.fract() == 0.0 && (1.0..=8.0).contains(&n) {
                        let a = compile_complex(a, params)?;
                        let k = n as u32;
                        return Ok(Box::new(move |z, c| {
                            let v = a(z, c);
                            let mut r = v;
                            for _ in 1..k {
                                r = r.mul(v);
                            }
                            r
                        }));
                    }
                }
            }
            let (a, b) = (compile_complex(a, params)?, compile_complex(b, params)?);
            match op {
                '+' => Box::new(move |z, c| a(z, c).add(b(z, c))),
                '-' => Box::new(move |z, c| a(z, c).sub(b(z, c))),
                '*' => Box::new(move |z, c| a(z, c).mul(b(z, c))),
                '/' => Box::new(move |z, c| a(z, c).div(b(z, c))),
                '^' => Box::new(move |z, c| a(z, c).pow(b(z, c))),
                _ => return Err("comparaison non disponible dans une formule complexe".to_string()),
            }
        }
        Expr::Call(name, args) => {
            if *name == "if" || args.len() != 1 {
                return Err(format!("{} non disponible dans une formule complexe ici", name));
            }
            let a = compile_complex(&args[0], params)?;
            let name: &'static str = name;
            Box::new(move |z, c| call_c(name, a(z, c)))
        }
        Expr::Sum { .. } | Expr::Bound(_) => return Err("sum / prod non disponibles dans `domain` et `escape`".to_string()),
    })
}

fn text(f: &ResolvedFigure, key: &str) -> Option<String> {
    f.properties.get(key).map(|v| match v {
        PropValue::Single(s) | PropValue::Block(s) => s.clone(),
        v => fmt_value(v),
    })
}

fn number(f: &ResolvedFigure, key: &str, default: f64) -> Result<f64, String> {
    match f.properties.get(key) {
        None => Ok(default),
        Some(PropValue::Number(n)) => Ok(*n),
        Some(v) => fmt_value(v).trim().parse().map_err(|_| format!("{} : nombre attendu, trouvé '{}'", key, fmt_value(v))),
    }
}

enum Mode {
    Heat { f: RealFn, crange: Option<(f64, f64)>, stops: Vec<Rgba> },
    Domain { f: ComplexFn, bands: bool },
    Escape { step: ComplexFn, start: ComplexFn, iters: u32, bailout: f64, stops: Vec<Rgba>, inside: Rgba, mandel: bool },
}

fn real_names(params: &HashMap<String, f64>) -> Vec<String> {
    let mut n: Vec<String> = params.keys().cloned().collect();
    n.push("y".into());
    n
}

fn complex_names(params: &HashMap<String, f64>) -> Vec<String> {
    let mut n: Vec<String> = params.keys().cloned().collect();
    n.push("c".into());
    n
}

fn mode(f: &ResolvedFigure, params: &HashMap<String, f64>) -> Result<Mode, String> {
    let given: Vec<&str> = ["expr", "domain", "escape"].into_iter().filter(|k| f.properties.contains_key(*k)).collect();
    if given.len() != 1 {
        return Err("pixels : une seule de `expr : f(x, y)`, `domain : f(z)`, `escape : z^2 + c`".to_string());
    }
    let pal = |default: &str| -> Result<Vec<Rgba>, String> {
        let v = f.properties.get("colormap").cloned().unwrap_or(PropValue::Single(default.to_string()));
        super::three::palette(&v)
    };
    match given[0] {
        "expr" => {
            let src = text(f, "expr").unwrap_or_default();
            let e = expr::parse_with(&src, &real_names(params)).map_err(|e| format!("expr '{}' : {}", src, e))?;
            let crange = match f.properties.get("crange") {
                None => None,
                Some(PropValue::List(v)) if v.len() == 2 => {
                    let n = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("crange : nombre invalide '{}'", s));
                    Some((n(&v[0])?, n(&v[1])?))
                }
                Some(v) => return Err(format!("crange doit être [min, max], trouvé '{}'", fmt_value(v))),
            };
            Ok(Mode::Heat { f: compile_real(&e, params)?, crange, stops: pal("viridis")? })
        }
        "domain" => {
            let src = text(f, "domain").unwrap_or_default();
            let e = expr::parse_complex(&src, &complex_names(params)).map_err(|e| format!("domain '{}' : {}", src, e))?;
            let bands = !matches!(text(f, "bands").as_deref(), Some("false"));
            Ok(Mode::Domain { f: compile_complex(&e, params)?, bands })
        }
        _ => {
            let src = text(f, "escape").unwrap_or_default();
            let names = complex_names(params);
            let e = expr::parse_complex(&src, &names).map_err(|e| format!("escape '{}' : {}", src, e))?;
            let s = text(f, "z0").unwrap_or_else(|| "0".into());
            let se = expr::parse_complex(&s, &names).map_err(|e| format!("z0 '{}' : {}", s, e))?;
            let iters = number(f, "iterations", 100.0)?.round().clamp(1.0, 100_000.0) as u32;
            let bailout = number(f, "bailout", 2.0)?;
            let inside = match f.properties.get("inside") {
                Some(v) => super::geometry::parse_color(&fmt_value(v))?.ok_or("inside : couleur invalide")?,
                None => Rgba::BLACK,
            };
            // z^2 + c partant de 0 : la boucle la plus courante, calculée sans détour
            let compact: String = src.chars().filter(|c| !c.is_whitespace()).collect();
            let mandel = matches!(compact.as_str(), "z^2+c" | "c+z^2" | "z*z+c") && s.trim() == "0";
            Ok(Mode::Escape { step: compile_complex(&e, params)?, start: compile_complex(&se, params)?, iters, bailout, stops: pal("magma")?, inside, mandel })
        }
    }
}

/// Teinte (0..1), saturation, clarté → couleur
fn hsl(h: f64, s: f64, l: f64) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(1.0)) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Rgba { r: (r + m) as f32, g: (g + m) as f32, b: (b + m) as f32, a: 1.0 }
}

/// Valeurs brutes (réel : f ; fuite : itération lissée ou NaN si dedans) ou couleurs (domaine)
fn compute(md: &Mode, w: usize, h: usize, px_to_data: &(dyn Fn(f64, f64) -> (f64, f64) + Sync)) -> Vec<Rgba> {
    // Calcul d'une ligne de pixels
    let row = |j: usize, out: &mut [f64]| {
        for (i, o) in out.iter_mut().enumerate() {
            let (x, y) = px_to_data(i as f64 + 0.5, j as f64 + 0.5);
            *o = match md {
                Mode::Heat { f, .. } => f(x, y),
                Mode::Domain { .. } => 0.0,
                Mode::Escape { step, start, iters, bailout, mandel, .. } => {
                    let c = C(x, y);
                    let b2 = bailout * bailout;
                    let mut n = 0;
                    let mut z;
                    if *mandel {
                        let (mut zr, mut zi) = (0.0f64, 0.0f64);
                        while n < *iters && zr * zr + zi * zi <= b2 {
                            let t = zr * zr - zi * zi + x;
                            zi = 2.0 * zr * zi + y;
                            zr = t;
                            n += 1;
                        }
                        z = C(zr, zi);
                    } else {
                        z = start(c, c);
                        while n < *iters && z.0 * z.0 + z.1 * z.1 <= b2 {
                            z = step(z, c);
                            n += 1;
                        }
                    }
                    if n >= *iters {
                        f64::NAN
                    } else {
                        // Lissage : pas de marches entre deux nombres d'itérations
                        let m = (z.0 * z.0 + z.1 * z.1).sqrt().max(1.0 + 1e-12);
                        n as f64 + 1.0 - (m.ln().max(1e-300)).ln() / std::f64::consts::LN_2
                    }
                }
            };
        }
    };
    let colors_row = |j: usize, out: &mut [Rgba]| {
        if let Mode::Domain { f, bands } = md {
            for (i, o) in out.iter_mut().enumerate() {
                let (x, y) = px_to_data(i as f64 + 0.5, j as f64 + 0.5);
                let v = f(C(x, y), C(x, y));
                let m = (v.0 * v.0 + v.1 * v.1).sqrt();
                *o = if !m.is_finite() || m.is_nan() {
                    Rgba::WHITE
                } else {
                    let hue = v.1.atan2(v.0) / std::f64::consts::TAU;
                    // Clarté : 0 pour |f| = 0, 1 pour |f| → ∞, 1/2 pour |f| = 1
                    let mut l = 2.0 / std::f64::consts::PI * m.atan();
                    if *bands && m > 0.0 {
                        let frac = m.log2().rem_euclid(1.0);
                        l *= 0.82 + 0.18 * frac;
                    }
                    hsl(hue, 1.0, 0.04 + 0.92 * l)
                };
            }
        }
    };

    let threads = workers().min(h.max(1));
    if let Mode::Domain { .. } = md {
        let mut img = vec![Rgba::BLACK; w * h];
        par_rows(&mut img, w, threads, &colors_row);
        return img;
    }
    let mut vals = vec![0.0f64; w * h];
    par_rows(&mut vals, w, threads, &row);

    match md {
        Mode::Heat { crange, stops, .. } => {
            let (lo, hi) = crange.unwrap_or_else(|| {
                let mut b = (f64::INFINITY, f64::NEG_INFINITY);
                for v in vals.iter().filter(|v| v.is_finite()) {
                    b = (b.0.min(*v), b.1.max(*v));
                }
                if b.0 < b.1 { b } else { (b.0, b.0 + 1.0) }
            });
            vals.iter()
                .map(|v| if v.is_finite() { super::three::sample(stops, ((v - lo) / (hi - lo)).clamp(0.0, 1.0)) } else { Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 } })
                .collect()
        }
        Mode::Escape { stops, inside, iters, .. } => {
            // Échelle racine : les premières itérations (le gros de l'image) ne prennent pas toute la palette
            vals.iter()
                .map(|v| if v.is_nan() { *inside } else { super::three::sample(stops, (v.max(0.0) / *iters as f64).sqrt().clamp(0.0, 1.0)) })
                .collect()
        }
        Mode::Domain { .. } => unreachable!(),
    }
}

/// Nombre de fils de calcul
fn workers() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("DIDACT_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
            .max(1)
    }
}

/// Remplit `buf` (w × h) ligne par ligne, en répartissant les lignes sur `threads` fils
fn par_rows<T: Send>(buf: &mut [T], w: usize, threads: usize, f: &(dyn Fn(usize, &mut [T]) + Sync)) {
    if w == 0 {
        return;
    }
    if threads <= 1 {
        for (j, r) in buf.chunks_mut(w).enumerate() {
            f(j, r);
        }
        return;
    }
    let rows: Vec<(usize, &mut [T])> = buf.chunks_mut(w).enumerate().collect();
    let per = rows.len().div_ceil(threads);
    let mut groups: Vec<Vec<(usize, &mut [T])>> = Vec::new();
    let mut it = rows.into_iter().peekable();
    while it.peek().is_some() {
        groups.push(it.by_ref().take(per).collect());
    }
    std::thread::scope(|s| {
        for g in groups {
            s.spawn(move || {
                for (j, r) in g {
                    f(j, r);
                }
            });
        }
    });
}

/// La forme d'une figure `pixels` : le cadre du repère (dans l'image), rempli par l'image calculée
pub fn shapes(f: &ResolvedFigure, scene: &Scene, refs: &Refs, opacity: f32) -> Result<Vec<Shape>, String> {
    let name = text(f, "axes").ok_or("pixels : `axes : repere` manquant")?;
    let m: AxesMap = super::geometry::resolve_geom(&name, refs)?.axes.ok_or_else(|| format!("'{}' n'est pas un repère axes", name))?;
    let md = mode(f, &refs.params)?;
    let (ax0, ay0) = m.to_px(m.xrange.0, m.yrange.1);
    let (ax1, ay1) = m.to_px(m.xrange.1, m.yrange.0);
    // Partie visible du cadre
    let (x0, y0) = (ax0.min(ax1).max(0.0), ay0.min(ay1).max(0.0));
    let (x1, y1) = (ax0.max(ax1).min(scene.width), ay0.max(ay1).min(scene.height));
    if x1 <= x0 || y1 <= y0 {
        return Ok(Vec::new());
    }
    let res = number(f, "resolution", 1.0)?;
    if !(res > 0.0 && res <= 2.0) {
        return Err("resolution : entre 0.1 et 2 (1 = un calcul par pixel)".to_string());
    }
    let w = (((x1 - x0) as f64 * res).round() as usize).clamp(1, 8192);
    let h = (((y1 - y0) as f64 * res).round() as usize).clamp(1, 8192);
    let (sx, sy) = ((x1 - x0) as f64 / w as f64, (y1 - y0) as f64 / h as f64);
    let to_data = move |i: f64, j: f64| {
        let px = x0 as f64 + i * sx;
        let py = y0 as f64 + j * sy;
        ((px - m.ox as f64) / m.ux as f64, (m.oy as f64 - py) / m.uy as f64)
    };
    let path = vec![
        PathCmd::MoveTo(x0, y0),
        PathCmd::LineTo(x1, y0),
        PathCmd::LineTo(x1, y1),
        PathCmd::LineTo(x0, y1),
        PathCmd::Close,
    ];
    if geometry_only() {
        return Ok(vec![Shape { path, fill: Some(Rgba { r: 1.0, g: 1.0, b: 1.0, a: opacity }), stroke: None, glow: 0.0, image: None }]);
    }
    let colors = compute(&md, w, h, &to_data);
    let mut data = Vec::with_capacity(w * h * 4);
    for c in colors {
        let a = c.a.clamp(0.0, 1.0);
        let q = |v: f32| (v.clamp(0.0, 1.0) * a * 255.0).round() as u8;
        data.extend_from_slice(&[q(c.r), q(c.g), q(c.b), (a * 255.0).round() as u8]);
    }
    Ok(vec![Shape {
        path,
        fill: Some(Rgba { r: 1.0, g: 1.0, b: 1.0, a: opacity }),
        stroke: None,
        glow: 0.0,
        image: Some(Arc::new(Raster { width: w as u32, height: h as u32, data })),
    }])
}

/// Vérification (validateur)
pub fn check(props: &HashMap<String, PropValue>, params: &[String]) -> Vec<(String, String)> {
    let vals: HashMap<String, f64> = params.iter().map(|p| (p.clone(), 1.0)).collect();
    let f = ResolvedFigure {
        name: String::new(),
        index: 0,
        ftype: "pixels".into(),
        layer: 0.0,
        line: 0,
        properties: props.clone(),
        prop_lines: HashMap::new(),
    };
    // Expressions de paramètres (iterations : 100 + 80*k) calculées comme au rendu
    let f = super::params::bind(&f, &vals).ok().flatten().unwrap_or(f);
    let mut errs = Vec::new();
    if !props.contains_key("axes") {
        errs.push(("type".to_string(), "pixels : `axes : repere` manquant".to_string()));
    }
    if let Err(e) = mode(&f, &vals) {
        let key = ["expr", "domain", "escape"].into_iter().find(|k| props.contains_key(*k)).unwrap_or("type");
        errs.push((key.to_string(), e));
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fig(props: &[(&str, &str)]) -> ResolvedFigure {
        let mut f = ResolvedFigure {
            name: "p".into(),
            index: 0,
            ftype: "pixels".into(),
            layer: 0.0,
            line: 1,
            properties: HashMap::new(),
            prop_lines: HashMap::new(),
        };
        for (k, v) in props {
            f.properties.insert(k.to_string(), PropValue::Single(v.to_string()));
        }
        f
    }

    #[test]
    fn formules_compilees_comme_l_evaluation() {
        let params: HashMap<String, f64> = [("k".to_string(), 2.0)].into();
        let names = real_names(&params);
        for src in ["sin(k*x)*cos(y) + x^2 - y/3", "if(x < y, max(x, y, 1), -x)", "sum(j, 1, 3, j*x) + y"] {
            let e = expr::parse_with(src, &names).unwrap();
            let c = compile_real(&e, &params).unwrap();
            for (x, y) in [(0.3, -1.2), (2.0, 0.5)] {
                let mut v = params.clone();
                v.insert("y".into(), y);
                assert!((c(x, y) - e.eval(x, &v)).abs() < 1e-12, "{}", src);
            }
        }
        let e = expr::parse_complex("z^2 + c + k*i/z", &complex_names(&params)).unwrap();
        let c = compile_complex(&e, &params).unwrap();
        let r = c(C(1.0, 1.0), C(0.5, 0.0));
        // (1+i)^2 = 2i ; 2i/(1+i) = 1 + i
        assert!((r.0 - 1.5).abs() < 1e-12 && (r.1 - 3.0).abs() < 1e-12, "{:?}", r);
    }

    #[test]
    fn mandelbrot() {
        let md = mode(&fig(&[("escape", "z^2 + c"), ("iterations", "50")]), &HashMap::new()).unwrap();
        // 3 × 1 pixels : c = -1 (dedans), c = 0 (dedans), c = 1 (s'échappe)
        let to_data = |i: f64, _j: f64| (i - 0.5 - 1.0, 0.0);
        let img = compute(&md, 3, 1, &to_data);
        assert_eq!(img[0], Rgba::BLACK);
        assert_eq!(img[1], Rgba::BLACK);
        assert_ne!(img[2], Rgba::BLACK);
    }

    #[test]
    fn coloration_de_domaine() {
        let md = mode(&fig(&[("domain", "z")]), &HashMap::new()).unwrap();
        // z = 1 : rouge (argument 0) ; z = i : vert-jaune (argument π/2 → teinte 1/4)
        let img = compute(&md, 2, 1, &|i, _| if i < 1.0 { (1.0, 0.0) } else { (0.0, 1.0) });
        assert!(img[0].r > 0.8 && img[0].g < 0.1 && img[0].b < 0.1, "{:?}", img[0]);
        assert!(img[1].g > img[1].b, "{:?}", img[1]);
    }

    #[test]
    fn erreurs() {
        assert!(mode(&fig(&[("expr", "x"), ("domain", "z")]), &HashMap::new()).is_err());
        assert!(mode(&fig(&[("domain", "w")]), &HashMap::new()).err().unwrap().contains("nom inconnu"));
        assert!(mode(&fig(&[("escape", "sum(k, 1, 2, z)")]), &HashMap::new()).err().unwrap().contains("sum"));
    }
}
