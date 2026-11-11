//! Courbes de niveau et régions du plan.
//!
//! - `contour` : courbes f(x, y) = c (lignes de niveau), ou une courbe implicite
//!   `expr : x^2 + y^2 = 1` ;
//! - `region` : zone remplie, entre deux courbes (`upper`, `lower`) ou définie par des
//!   inégalités (`where : and(x^2 + y^2 <= 4, y >= x)`).
//!
//! Méthode : la fonction est évaluée sur une grille fine (« marching squares ») ; les lignes
//! sont recollées case à case en lignes brisées, les régions sont la réunion des morceaux de
//! cases où la fonction est ≤ 0, remplis d'un seul tenant (pas de joints visibles).

use super::expr::{self, Expr};
use super::geometry::{AxesMap, PathCmd, Refs, Rgba, Shape};
use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;

/// Valeurs d'une fonction sur une grille de (nx + 1) × (ny + 1) points
pub struct Field {
    pub nx: usize,
    pub ny: usize,
    pub xr: (f64, f64),
    pub yr: (f64, f64),
    pub v: Vec<f64>,
}

impl Field {
    pub fn sample(f: &mut dyn FnMut(f64, f64) -> f64, xr: (f64, f64), yr: (f64, f64), nx: usize, ny: usize) -> Field {
        let mut v = Vec::with_capacity((nx + 1) * (ny + 1));
        for j in 0..=ny {
            let y = yr.0 + (yr.1 - yr.0) * j as f64 / ny as f64;
            for i in 0..=nx {
                let x = xr.0 + (xr.1 - xr.0) * i as f64 / nx as f64;
                v.push(f(x, y));
            }
        }
        Field { nx, ny, xr, yr, v }
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.v[j * (self.nx + 1) + i]
    }

    fn xy(&self, i: f64, j: f64) -> (f64, f64) {
        (
            self.xr.0 + (self.xr.1 - self.xr.0) * i / self.nx as f64,
            self.yr.0 + (self.yr.1 - self.yr.0) * j / self.ny as f64,
        )
    }

    /// Plus petite et plus grande valeur définie
    pub fn bounds(&self) -> Option<(f64, f64)> {
        let mut b = (f64::INFINITY, f64::NEG_INFINITY);
        for v in self.v.iter().filter(|v| v.is_finite()) {
            b = (b.0.min(*v), b.1.max(*v));
        }
        (b.0 <= b.1).then_some(b)
    }

    /// Point où la fonction vaut `c` sur l'arête entre deux sommets de la grille
    fn cross(&self, a: (usize, usize), b: (usize, usize), c: f64) -> (f64, f64) {
        let (va, vb) = (self.at(a.0, a.1), self.at(b.0, b.1));
        let t = if vb != va { ((c - va) / (vb - va)).clamp(0.0, 1.0) } else { 0.5 };
        self.xy(a.0 as f64 + (b.0 as f64 - a.0 as f64) * t, a.1 as f64 + (b.1 as f64 - a.1 as f64) * t)
    }

    /// Courbes f = c, en lignes brisées (coordonnées du repère)
    pub fn isolines(&self, c: f64) -> Vec<Vec<(f64, f64)>> {
        // Arête : (0 = horizontale (i,j)-(i+1,j), 1 = verticale (i,j)-(i,j+1), i, j)
        type Edge = (u8, usize, usize);
        let mut segs: Vec<(Edge, Edge)> = Vec::new();
        for j in 0..self.ny {
            for i in 0..self.nx {
                let v = [self.at(i, j), self.at(i + 1, j), self.at(i + 1, j + 1), self.at(i, j + 1)];
                if v.iter().any(|x| !x.is_finite()) {
                    continue;
                }
                let idx = (v[0] > c) as u8 | ((v[1] > c) as u8) << 1 | ((v[2] > c) as u8) << 2 | ((v[3] > c) as u8) << 3;
                // Arêtes de la case : bas, droite, haut, gauche
                let (b, r, t, l) = ((0u8, i, j), (1u8, i + 1, j), (0u8, i, j + 1), (1u8, i, j));
                let centre_above = (v.iter().sum::<f64>() / 4.0) > c;
                let pairs: &[(Edge, Edge)] = match idx {
                    0 | 15 => &[],
                    1 | 14 => &[(l, b)],
                    2 | 13 => &[(b, r)],
                    3 | 12 => &[(l, r)],
                    4 | 11 => &[(r, t)],
                    6 | 9 => &[(b, t)],
                    7 | 8 => &[(l, t)],
                    // Cas ambigus : tranchés par la valeur au centre
                    5 => if centre_above { &[(l, t), (b, r)] } else { &[(l, b), (r, t)] },
                    10 => if centre_above { &[(l, b), (r, t)] } else { &[(l, t), (b, r)] },
                    _ => &[],
                };
                segs.extend_from_slice(pairs);
            }
        }
        // Recollage : chaque arête touche au plus deux segments
        let mut by_edge: HashMap<Edge, Vec<usize>> = HashMap::new();
        for (k, (a, b)) in segs.iter().enumerate() {
            by_edge.entry(*a).or_default().push(k);
            by_edge.entry(*b).or_default().push(k);
        }
        let point = |e: Edge| {
            let (a, b) = if e.0 == 0 { ((e.1, e.2), (e.1 + 1, e.2)) } else { ((e.1, e.2), (e.1, e.2 + 1)) };
            self.cross(a, b, c)
        };
        let mut used = vec![false; segs.len()];
        let mut lines = Vec::new();
        for start in 0..segs.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut chain: std::collections::VecDeque<Edge> = [segs[start].0, segs[start].1].into_iter().collect();
            // Prolonge d'un côté puis de l'autre
            for forward in [true, false] {
                loop {
                    let end = if forward { *chain.back().unwrap() } else { *chain.front().unwrap() };
                    let next = by_edge.get(&end).and_then(|ks| ks.iter().copied().find(|k| !used[*k]));
                    let Some(k) = next else { break };
                    used[k] = true;
                    let other = if segs[k].0 == end { segs[k].1 } else { segs[k].0 };
                    if forward { chain.push_back(other) } else { chain.push_front(other) }
                }
            }
            lines.push(chain.into_iter().map(point).collect());
        }
        lines
    }

    /// Morceaux de la zone f ≤ 0, en polygones de même sens (à remplir d'un seul chemin)
    pub fn inside(&self) -> Vec<Vec<(f64, f64)>> {
        let mut polys = Vec::new();
        for j in 0..self.ny {
            let mut run: Option<usize> = None; // début d'une suite de cases entièrement dedans
            for i in 0..=self.nx {
                let full = i < self.nx && {
                    let v = [self.at(i, j), self.at(i + 1, j), self.at(i + 1, j + 1), self.at(i, j + 1)];
                    v.iter().all(|x| x.is_finite() && *x <= 0.0)
                };
                if full {
                    run.get_or_insert(i);
                    continue;
                }
                // Fin d'une suite : un seul rectangle
                if let Some(i0) = run.take() {
                    polys.push(vec![self.xy(i0 as f64, j as f64), self.xy(i as f64, j as f64), self.xy(i as f64, (j + 1) as f64), self.xy(i0 as f64, (j + 1) as f64)]);
                }
                if i == self.nx {
                    break;
                }
                // Case partiellement dedans : on parcourt ses bords (sens trigonométrique)
                let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let vals: Vec<f64> = corners.iter().map(|c| self.at(c.0, c.1)).collect();
                if vals.iter().any(|x| !x.is_finite()) || vals.iter().all(|x| *x > 0.0) {
                    continue;
                }
                let mut poly = Vec::new();
                for k in 0..4 {
                    let (a, b) = (corners[k], corners[(k + 1) % 4]);
                    let (va, vb) = (vals[k], vals[(k + 1) % 4]);
                    if va <= 0.0 {
                        poly.push(self.xy(a.0 as f64, a.1 as f64));
                    }
                    if (va <= 0.0) != (vb <= 0.0) {
                        poly.push(self.cross(a, b, 0.0));
                    }
                }
                if poly.len() >= 3 {
                    polys.push(poly);
                }
            }
        }
        polys
    }
}

/// Une équation `g = h` devient `g - h` (la courbe est alors f = 0) ; None s'il n'y a pas de `=` seul
pub fn equation_to_level(src: &str) -> Option<String> {
    let b = src.as_bytes();
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'=' if depth == 0 => {
                let prev = if i > 0 { b[i - 1] } else { b' ' };
                let next = b.get(i + 1).copied().unwrap_or(b' ');
                if !matches!(prev, b'<' | b'>' | b'=' | b'!') && next != b'=' {
                    return Some(format!("({}) - ({})", &src[..i], &src[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Inégalité (ou and / or d'inégalités) → fonction ≤ 0 exactement dans la zone
fn to_level(e: &Expr) -> Result<Expr, String> {
    let bx = Box::new;
    match e {
        Expr::Bin(op @ ('<' | 'l' | '>' | 'g'), a, b) => Ok(match op {
            '<' | 'l' => Expr::Bin('-', a.clone(), b.clone()),
            _ => Expr::Bin('-', b.clone(), a.clone()),
        }),
        Expr::Call("and", args) => Ok(Expr::Call("max", args.iter().map(to_level).collect::<Result<_, _>>()?)),
        Expr::Call("or", args) => Ok(Expr::Call("min", args.iter().map(to_level).collect::<Result<_, _>>()?)),
        Expr::Call("not", args) => Ok(Expr::Neg(bx(to_level(&args[0])?))),
        _ => Err("where attend une inégalité en x et y : ex. x^2 + y^2 <= 4, ou and(..., ...) / or(..., ...)".to_string()),
    }
}

fn names(refs_params: &HashMap<String, f64>) -> Vec<String> {
    let mut n: Vec<String> = refs_params.keys().cloned().collect();
    n.push("y".to_string());
    n
}

/// Formule en x et y : `f(x, y)`, ou une équation `g = h`
fn level_expr(src: &str, params: &HashMap<String, f64>) -> Result<(Expr, bool), String> {
    let (src2, is_eq) = match equation_to_level(src) {
        Some(s) => (s, true),
        None => (src.to_string(), false),
    };
    let e = expr::parse_with(&src2, &names(params)).map_err(|e| format!("expr '{}' : {}", src, e))?;
    Ok((e, is_eq))
}

fn where_expr(src: &str, params: &HashMap<String, f64>) -> Result<Expr, String> {
    let e = expr::parse_with(src, &names(params)).map_err(|e| format!("where '{}' : {}", src, e))?;
    to_level(&e).map_err(|m| format!("'{}' : {}", src, m))
}

fn curve_expr(key: &str, src: &str, params: &HashMap<String, f64>) -> Result<Expr, String> {
    expr::parse_with(src, &params.keys().cloned().collect::<Vec<_>>()).map_err(|e| format!("{} '{}' : {}", key, src, e))
}

/// Niveaux : un nombre n, ou une liste. Avec un nombre, les niveaux suivent la répartition
/// des valeurs (quantiles) : autant de lignes dans un creux que sur les pentes raides.
pub fn levels(v: Option<&PropValue>, default_zero: bool, field: Option<&Field>) -> Result<Vec<f64>, String> {
    let auto = |n: usize| -> Vec<f64> {
        let Some(field) = field else { return Vec::new() };
        let mut vals: Vec<f64> = field.v.iter().copied().filter(|x| x.is_finite()).collect();
        if vals.is_empty() {
            return Vec::new();
        }
        vals.sort_by(f64::total_cmp);
        let mut out: Vec<f64> = (1..=n).map(|k| vals[(vals.len() - 1) * k / (n + 1)]).collect();
        out.dedup();
        out
    };
    match v {
        None if default_zero => Ok(vec![0.0]),
        None => Ok(auto(10)),
        Some(PropValue::Number(n)) if *n >= 1.0 && *n <= 200.0 => Ok(auto(*n as usize)),
        Some(PropValue::List(items)) => items
            .iter()
            .map(|s| s.trim().parse::<f64>().map_err(|_| format!("levels : nombre invalide '{}'", s)))
            .collect(),
        Some(other) => Err(format!("levels invalide '{}' : un nombre de niveaux (10) ou une liste [-1, 0, 1]", fmt_value(other))),
    }
}

fn text(f: &ResolvedFigure, key: &str) -> Option<String> {
    f.properties.get(key).map(|v| match v {
        PropValue::Single(s) | PropValue::Block(s) => s.clone(),
        v => fmt_value(v),
    })
}

fn axes_map(f: &ResolvedFigure, refs: &Refs) -> Result<AxesMap, String> {
    let name = text(f, "axes").ok_or("propriété `axes` manquante : un repère")?;
    super::geometry::resolve_geom(&name, refs)?.axes.ok_or_else(|| format!("'{}' n'est pas un repère axes", name))
}

/// Intervalle [a, b] ; vide si b ≤ a (une borne animée peut partir de a)
fn span(f: &ResolvedFigure, key: &str, default: (f64, f64)) -> Result<(f64, f64), String> {
    match f.properties.get(key) {
        None => Ok(default),
        Some(PropValue::List(items)) if items.len() == 2 => {
            let n = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("{} : nombre invalide '{}'", key, s));
            Ok((n(&items[0])?, n(&items[1])?))
        }
        Some(v) => Err(format!("{} doit être [min, max], trouvé '{}'", key, fmt_value(v))),
    }
}

/// Finesse de la grille : `resolution` cases sur la largeur (défaut 160), autant en hauteur à l'échelle
fn grid_size(f: &ResolvedFigure, m: &AxesMap, xr: (f64, f64), yr: (f64, f64)) -> Result<(usize, usize), String> {
    let n = match f.properties.get("resolution") {
        None => 160.0,
        Some(PropValue::Number(n)) if *n >= 4.0 && *n <= 800.0 => *n,
        Some(v) => return Err(format!("resolution invalide '{}' (entre 4 et 800)", fmt_value(v))),
    };
    let (w, h) = (((xr.1 - xr.0) * m.ux as f64).abs(), ((yr.1 - yr.0) * m.uy as f64).abs());
    let nx = n as usize;
    let ny = ((n * h / w.max(1e-9)).round() as usize).clamp(4, 800);
    Ok((nx, ny))
}

fn to_path(polys: &[Vec<(f64, f64)>], m: &AxesMap, closed: bool) -> Vec<PathCmd> {
    let mut path = Vec::new();
    for p in polys.iter().filter(|p| p.len() >= 2) {
        let (x, y) = m.to_px(p[0].0, p[0].1);
        path.push(PathCmd::MoveTo(x, y));
        for q in &p[1..] {
            let (x, y) = m.to_px(q.0, q.1);
            path.push(PathCmd::LineTo(x, y));
        }
        if closed {
            path.push(PathCmd::Close);
        }
    }
    path
}

/// Formes d'une figure `contour` ou `region`
pub fn shapes(f: &ResolvedFigure, refs: &Refs, color: Option<Rgba>, fill: Option<Rgba>, stroke: f32, opacity: f32) -> Result<Vec<Shape>, String> {
    let m = axes_map(f, refs)?;
    let params = &refs.params;
    let mut vars = params.clone();
    let xr = span(f, "xrange", m.xrange)?;
    let yr = span(f, "yrange", m.yrange)?;
    if f.ftype == "contour" {
        let src = text(f, "expr").ok_or("propriété `expr` manquante : f(x, y), ou une équation x^2 + y^2 = 1")?;
        let (e, is_eq) = level_expr(&src, params)?;
        if !(xr.1 > xr.0 && yr.1 > yr.0) {
            return Ok(Vec::new());
        }
        let (nx, ny) = grid_size(f, &m, xr, yr)?;
        let field = Field::sample(
            &mut |x, y| {
                vars.insert("y".into(), y);
                e.eval(x, &vars)
            },
            xr,
            yr,
            nx,
            ny,
        );
        let lv = levels(f.properties.get("levels"), is_eq, Some(&field))?;
        let pal = match f.properties.get("colormap") {
            Some(v) => Some(super::three::palette(v)?),
            None => None,
        };
        let base = color.unwrap_or(Rgba::BLACK);
        let mut out = Vec::new();
        for (k, c) in lv.iter().enumerate() {
            let col = match &pal {
                Some(stops) => {
                    // Couleur selon le rang du niveau (les niveaux peuvent être très inégalement espacés)
                    let t = if lv.len() > 1 { k as f64 / (lv.len() - 1) as f64 } else { 0.5 };
                    super::three::sample(stops, t).with_opacity(opacity)
                }
                None => base,
            };
            let path = to_path(&field.isolines(*c), &m, false);
            if !path.is_empty() {
                out.push(Shape { path, fill: None, stroke: Some((col, stroke)), glow: 0.0, image: None });
            }
        }
        return Ok(out);
    }

    // region
    let fill = fill.or(Some(Rgba::rgb8(0x25, 0x63, 0xEB).with_opacity(0.35 * opacity)));
    let has_where = f.properties.contains_key("where");
    let has_curves = f.properties.contains_key("upper") || f.properties.contains_key("lower");
    if has_where == has_curves {
        return Err("region : `where : inégalité` ou bien `upper` / `lower` (deux courbes en x)".to_string());
    }
    if has_where {
        let e = where_expr(&text(f, "where").unwrap_or_default(), params)?;
        if !(xr.1 > xr.0 && yr.1 > yr.0) {
            return Ok(Vec::new());
        }
        let (nx, ny) = grid_size(f, &m, xr, yr)?;
        let field = Field::sample(
            &mut |x, y| {
                vars.insert("y".into(), y);
                e.eval(x, &vars)
            },
            xr,
            yr,
            nx,
            ny,
        );
        let mut out = Vec::new();
        let path = to_path(&field.inside(), &m, true);
        if !path.is_empty() {
            out.push(Shape { path, fill, stroke: None, glow: 0.0, image: None });
        }
        // Bord tracé seulement si `color` est donnée
        if let Some(c) = color {
            let path = to_path(&field.isolines(0.0), &m, false);
            if !path.is_empty() {
                out.push(Shape { path, fill: None, stroke: Some((c, stroke)), glow: 0.0, image: None });
            }
        }
        return Ok(out);
    }
    // Entre deux courbes : upper(x) et lower(x) (0 par défaut), pour x dans xrange
    let up = curve_expr("upper", &text(f, "upper").unwrap_or_else(|| "0".into()), params)?;
    let lo = curve_expr("lower", &text(f, "lower").unwrap_or_else(|| "0".into()), params)?;
    if xr.1 <= xr.0 {
        return Ok(Vec::new());
    }
    let n = (((xr.1 - xr.0) * m.ux as f64 / 2.0) as usize).clamp(8, 4000);
    let xs: Vec<f64> = (0..=n).map(|i| xr.0 + (xr.1 - xr.0) * i as f64 / n as f64).collect();
    // Bornées à yrange (une courbe qui part à l'infini reste dans le cadre)
    let clampy = |v: f64| v.clamp(m.yrange.0 - 1.0 * (m.yrange.1 - m.yrange.0), m.yrange.1 + (m.yrange.1 - m.yrange.0));
    let mut poly = Vec::with_capacity(2 * n + 2);
    for &x in &xs {
        let y = up.eval(x, params);
        if y.is_finite() {
            poly.push((x, clampy(y)));
        }
    }
    for &x in xs.iter().rev() {
        let y = lo.eval(x, params);
        if y.is_finite() {
            poly.push((x, clampy(y)));
        }
    }
    let path = to_path(&[poly], &m, true);
    let stroke = if f.properties.contains_key("color") { color.map(|c| (c, stroke)) } else { None };
    Ok(vec![Shape { path, fill, stroke, glow: 0.0, image: None }])
}

/// Vérification (validateur) des formules de `contour` et `region`
pub fn check(ftype: &str, props: &HashMap<String, PropValue>, params: &[String]) -> Vec<(String, String)> {
    let vals: HashMap<String, f64> = params.iter().map(|p| (p.clone(), 1.0)).collect();
    let get = |k: &str| props.get(k).map(fmt_value);
    let mut errs = Vec::new();
    match ftype {
        "contour" => match get("expr") {
            None => errs.push(("type".into(), "contour : `expr` manquante, ex. x^2 + y^2 = 1 ou sin(x)*cos(y)".into())),
            Some(s) => {
                if let Err(e) = level_expr(&s, &vals) {
                    errs.push(("expr".into(), e));
                }
            }
        },
        "region" => {
            let has_where = props.contains_key("where");
            let has_curves = props.contains_key("upper") || props.contains_key("lower");
            if has_where == has_curves {
                errs.push(("type".into(), "region : `where : inégalité` ou bien `upper` / `lower` (deux courbes en x)".into()));
            } else if let Some(s) = get("where") {
                if let Err(e) = where_expr(&s, &vals) {
                    errs.push(("where".into(), e));
                }
            } else {
                for k in ["upper", "lower"] {
                    if let Some(s) = get(k) {
                        if let Err(e) = curve_expr(k, &s, &vals) {
                            errs.push((k.into(), e));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    if matches!(ftype, "contour" | "region") && !props.contains_key("axes") {
        errs.push(("type".into(), format!("{} : `axes : repere` manquant", ftype)));
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle_field(n: usize) -> Field {
        Field::sample(&mut |x, y| x * x + y * y - 1.0, (-2.0, 2.0), (-2.0, 2.0), n, n)
    }

    #[test]
    fn cercle_implicite() {
        let f = circle_field(80);
        let lines = f.isolines(0.0);
        assert_eq!(lines.len(), 1, "une seule courbe fermée");
        let l = &lines[0];
        assert!(l.len() > 100);
        assert!(l.iter().all(|(x, y)| ((x * x + y * y).sqrt() - 1.0).abs() < 0.01));
        // fermée : le premier et le dernier point se rejoignent
        let (a, b) = (l[0], l[l.len() - 1]);
        assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
    }

    #[test]
    fn aire_du_disque() {
        let f = circle_field(200);
        let area: f64 = f
            .inside()
            .iter()
            .map(|p| {
                let n = p.len();
                (0..n).map(|k| p[k].0 * p[(k + 1) % n].1 - p[(k + 1) % n].0 * p[k].1).sum::<f64>() / 2.0
            })
            .sum();
        // Tous dans le même sens (aires positives), total ≈ π
        assert!((area - std::f64::consts::PI).abs() < 0.01, "{}", area);
    }

    #[test]
    fn equations_et_inegalites() {
        assert_eq!(equation_to_level("x^2 + y^2 = 1").as_deref(), Some("(x^2 + y^2 ) - ( 1)"));
        assert!(equation_to_level("x <= 1").is_none());
        assert!(equation_to_level("x == 1").is_none());
        let p = HashMap::new();
        let e = where_expr("and(x^2 + y^2 <= 4, y >= x)", &p).unwrap();
        let at = |x: f64, y: f64| {
            let mut v = HashMap::new();
            v.insert("y".to_string(), y);
            e.eval(x, &v)
        };
        assert!(at(-1.0, 1.0) <= 0.0); // dans le disque, au-dessus de y = x
        assert!(at(1.0, -1.0) > 0.0); // sous y = x
        assert!(at(3.0, 3.0) > 0.0); // hors du disque
        assert!(where_expr("x + y", &p).unwrap_err().contains("inégalité"));
    }

    #[test]
    fn niveaux() {
        let f = Field { nx: 4, ny: 0, xr: (0.0, 1.0), yr: (0.0, 1.0), v: vec![0.0, 1.0, 2.0, 3.0, 4.0] };
        assert_eq!(levels(None, true, Some(&f)).unwrap(), vec![0.0]);
        assert_eq!(levels(Some(&PropValue::Number(3.0)), false, Some(&f)).unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(levels(Some(&PropValue::List(vec!["-1".into(), "2".into()])), false, None).unwrap(), vec![-1.0, 2.0]);
    }
}
