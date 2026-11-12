//! 3D : scènes, caméra, objets 3D, projection triée.
//!
//! Principe :
//! 1. chaque objet 3D est découpé en petits morceaux (`Item`) : faces, segments, points ;
//!    ils sont en coordonnées **de la scène** (celles de `xrange`, `yrange`, `zrange`) ;
//! 2. les animations d'un objet (déplacement, rotation 3D, taille, fondu, dessin progressif)
//!    transforment ses morceaux ;
//! 3. la scène rassemble les morceaux de tous ses objets, les projette avec sa caméra,
//!    les trie du plus loin au plus proche et les transforme en formes 2D (`Shape`).
//!    Le reste du moteur (dessin, fenêtre, exports) ne change pas.
//!
//! Trier des morceaux (plutôt qu'un tampon de profondeur par pixel) garde la transparence
//! et l'anti-crénelage. Les surfaces et les solides sont découpés finement pour que deux
//! objets qui se croisent restent dans le bon ordre presque partout.
//!
//! Conventions : z vers le haut. Caméra : élévation (au-dessus du plan xy) et rotation
//! autour de z, en degrés.

use super::expr::{self, Expr};
use super::geometry::{self, split_call, AxesMap, PathCmd, Refs, Rgba, Scene, Shape};
use super::typeset;
use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use crate::runtime::animation::AnimKind;
use crate::runtime::timeline::FigureState;
use std::collections::HashMap;

pub type V3 = [f64; 3];
type M3 = [[f64; 3]; 3];

// ─────────────────────────────────────────────
//  VECTEURS
// ─────────────────────────────────────────────

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n > 0.0 { mul(a, 1.0 / n) } else { [0.0, 0.0, 1.0] }
}
fn lerp(a: V3, b: V3, e: f64) -> V3 {
    add(a, mul(sub(b, a), e))
}
fn finite(p: V3) -> bool {
    p.iter().all(|v| v.is_finite())
}

const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mat_vec(m: &M3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}
fn mat_mul(a: &M3, b: &M3) -> M3 {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}
/// Rotation de `deg` degrés autour de `axis` (formule de Rodrigues)
fn rotation(axis: V3, deg: f64) -> M3 {
    let [x, y, z] = unit(axis);
    let (s, c) = deg.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}
/// Orientation fixe `rotate : (rx, ry, rz)` : autour de x, puis y, puis z
fn euler(r: V3) -> M3 {
    let m = mat_mul(&rotation([0.0, 1.0, 0.0], r[1]), &rotation([1.0, 0.0, 0.0], r[0]));
    mat_mul(&rotation([0.0, 0.0, 1.0], r[2]), &m)
}
/// Deux vecteurs unitaires perpendiculaires à `d` (et entre eux)
fn basis(d: V3) -> (V3, V3) {
    let d = unit(d);
    let other = if d[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let a = unit(cross(d, other));
    (a, cross(d, a))
}

// ─────────────────────────────────────────────
//  MORCEAUX
// ─────────────────────────────────────────────

/// Un morceau d'objet 3D, en coordonnées de la scène
#[derive(Debug, Clone)]
pub enum Prim {
    /// Face plane (3 points ou plus) ; `shade` : éclairée selon son orientation
    Face { pts: Vec<V3>, color: Rgba, shade: bool },
    /// Segment ; `tube` : dessiné comme un tube éclairé
    Seg { a: V3, b: V3, color: Rgba, width: f32, tube: bool },
    /// Ligne brisée (quelques segments qui se suivent, dessinés d'un seul trait)
    Line { pts: Vec<V3>, color: Rgba, width: f32, tube: bool },
    /// Point (disque toujours face à la caméra), rayon en pixels
    Dot { p: V3, color: Rgba, radius: f32 },
    /// Formule LaTeX accrochée à un point, toujours face à la caméra
    Label { p: V3, tex: String, size: f32, color: Rgba },
}

/// Un morceau, son moment d'apparition pendant `draw` (début, fin entre 0 et 1) et son halo
#[derive(Debug, Clone)]
pub struct Item {
    pub prim: Prim,
    pub order: (f32, f32),
    pub glow: f32,
}

/// Un objet 3D prêt à être placé dans sa scène
#[derive(Debug, Clone, Default)]
pub struct Obj {
    pub scene: String,
    pub items: Vec<Item>,
    /// Chemin suivi par `follow` (courbes, trajectoires)
    pub path: Vec<V3>,
}

impl Prim {
    fn points(&self) -> Vec<V3> {
        match self {
            Prim::Face { pts, .. } => pts.clone(),
            Prim::Seg { a, b, .. } => vec![*a, *b],
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Dot { p, .. } | Prim::Label { p, .. } => vec![*p],
        }
    }

    fn map(&mut self, f: &dyn Fn(V3) -> V3) {
        match self {
            Prim::Face { pts, .. } | Prim::Line { pts, .. } => pts.iter_mut().for_each(|p| *p = f(*p)),
            Prim::Seg { a, b, .. } => {
                *a = f(*a);
                *b = f(*b);
            }
            Prim::Dot { p, .. } | Prim::Label { p, .. } => *p = f(*p),
        }
    }

    fn fade(&mut self, k: f32) {
        match self {
            Prim::Face { color, .. }
            | Prim::Seg { color, .. }
            | Prim::Line { color, .. }
            | Prim::Dot { color, .. }
            | Prim::Label { color, .. } => {
                color.a *= k
            }
        }
    }
}

impl Obj {
    /// Centre de la boîte englobante (coordonnées de la scène)
    pub fn center(&self) -> V3 {
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for it in &self.items {
            for p in it.prim.points() {
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
        }
        if lo[0] > hi[0] { [0.0; 3] } else { mul(add(lo, hi), 0.5) }
    }
}

fn item(prim: Prim, order: (f32, f32), glow: f32) -> Item {
    Item { prim, order, glow }
}

/// Nombre de segments regroupés en une ligne : moins de traits à dessiner, tout en gardant
/// des morceaux assez courts pour se trier correctement avec les faces
const RUN: usize = 6;

/// Regroupe les segments qui se suivent (même apparence, bout à bout) en lignes brisées
pub fn merge_runs(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut count = 0; // segments dans la ligne en cours
    for it in items {
        if let Prim::Seg { a, b, color, width, tube } = &it.prim {
            if let Some(last) = out.last_mut() {
                let same = last.glow == it.glow;
                match &mut last.prim {
                    Prim::Line { pts, color: c, width: w, tube: t }
                        if same && count < RUN && c == color && w == width && t == tube && pts.last() == Some(a) =>
                    {
                        pts.push(*b);
                        last.order.1 = last.order.1.max(it.order.1);
                        count += 1;
                        continue;
                    }
                    _ => {}
                }
            }
            out.push(Item { prim: Prim::Line { pts: vec![*a, *b], color: *color, width: *width, tube: *tube }, order: it.order, glow: it.glow });
            count = 1;
        } else {
            out.push(it);
            count = 0;
        }
    }
    out
}

// ─────────────────────────────────────────────
//  SCÈNE ET CAMÉRA
// ─────────────────────────────────────────────

/// Une scène 3D à l'instant t : comment passer des coordonnées de la scène aux pixels
#[derive(Debug, Clone)]
pub struct Space {
    pub ranges: [(f64, f64); 3],
    /// Coordonnées de la scène → monde : (p - mid) × k
    mid: V3,
    k: V3,
    eye: V3,
    right: V3,
    up: V3,
    fwd: V3,
    perspective: bool,
    /// Pixels par unité du monde (à distance 1 en perspective)
    focal: f64,
    center: (f64, f64),
    /// Direction de la lumière (monde, unitaire) et part de lumière ambiante
    light: V3,
    ambient: f64,
    dist: f64,
}

impl Space {
    fn to_world(&self, p: V3) -> V3 {
        [(p[0] - self.mid[0]) * self.k[0], (p[1] - self.mid[1]) * self.k[1], (p[2] - self.mid[2]) * self.k[2]]
    }

    /// Point de la scène → (x, y) en pixels et profondeur ; None derrière la caméra
    pub fn project(&self, p: V3) -> Option<(f32, f32, f64)> {
        let d = sub(self.to_world(p), self.eye);
        let (x, y, z) = (dot(d, self.right), dot(d, self.up), dot(d, self.fwd));
        if self.perspective {
            if z < self.dist * 0.05 {
                return None;
            }
            Some(((self.center.0 + self.focal * x / z) as f32, (self.center.1 - self.focal * y / z) as f32, z))
        } else {
            Some(((self.center.0 + self.focal * x) as f32, (self.center.1 - self.focal * y) as f32, z))
        }
    }

    /// Plus grande étendue des axes (unités de la scène)
    fn span(&self) -> f64 {
        self.ranges.iter().map(|r| r.1 - r.0).fold(0.0, f64::max)
    }
}

// ─────────────────────────────────────────────
//  LECTURE DES PROPRIÉTÉS
// ─────────────────────────────────────────────

/// Types toujours en 3D ; `arrow`, `field` et `iterate` sont en 3D s'ils ont une scène
pub const TYPES_3D: &[&str] = &["surface", "curve3d", "cube", "box", "sphere", "cylinder", "cone", "plane", "polyhedron"];

/// Méthodes d'optimisation intégrées de `iterate`
pub const METHODS: &[&str] = &["gd", "momentum", "nesterov", "rmsprop", "adam", "newton"];

/// Variables disponibles dans `update` (en plus des variables de l'itération et des paramètres)
const ITERATE_LOCALS: &[&str] = &["n", "f", "dx", "dy", "hxx", "hxy", "hyy"];

fn compile(src: &str, locals: &[&str], refs: &Refs) -> Result<Expr, String> {
    let mut names = refs.param_names();
    names.extend(locals.iter().map(|s| s.to_string()));
    expr::parse_with(src, &names).map_err(|e| format!("'{}' : {}", src, e))
}

/// Variables d'évaluation : paramètres + variables locales
fn vars(refs: &Refs) -> HashMap<String, f64> {
    refs.params.clone()
}

/// Un nombre : 3, ou une expression avec des paramètres
fn eval_num(src: &str, refs: &Refs) -> Result<f64, String> {
    if let Ok(v) = src.trim().parse::<f64>() {
        return Ok(v);
    }
    let e = compile(src, &[], refs)?;
    let v = e.eval(0.0, &refs.params);
    if v.is_finite() { Ok(v) } else { Err(format!("'{}' ne donne pas un nombre", src)) }
}

fn num(f: &ResolvedFigure, key: &str, default: f64, refs: &Refs) -> Result<f64, String> {
    match f.properties.get(key) {
        None => Ok(default),
        Some(PropValue::Number(n)) => Ok(*n),
        Some(v) => eval_num(&fmt_value(v), refs).map_err(|e| format!("{} : {}", key, e)),
    }
}

/// Éléments d'une valeur (a, b, c) ou [a, b] : du texte, à lire comme des expressions
fn parts(v: &PropValue) -> Vec<String> {
    match v {
        PropValue::Tuple(items) | PropValue::List(items) => items.clone(),
        PropValue::Number(n) => vec![n.to_string()],
        other => {
            let s = fmt_value(other);
            let t = s.trim();
            match t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
                Some(inside) => split_top(inside),
                None => vec![t.to_string()],
            }
        }
    }
}

/// Retire une seule paire de parenthèses extérieures : « (a, sin(b)) » → « a, sin(b) »
trait OuterParens {
    fn pipe_outer(&self) -> &str;
}

impl OuterParens for str {
    fn pipe_outer(&self) -> &str {
        let t = self.trim();
        match t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            Some(inner) => inner,
            None => t,
        }
    }
}

/// Découpe aux virgules qui ne sont pas dans des parenthèses
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim().to_string());
    out
}

fn nums(f: &ResolvedFigure, key: &str, n: usize, refs: &Refs) -> Result<Option<Vec<f64>>, String> {
    let Some(v) = f.properties.get(key) else { return Ok(None) };
    let items = parts(v);
    if items.len() != n {
        return Err(format!("{} : {} valeurs attendues, trouvé '{}'", key, n, fmt_value(v)));
    }
    items.iter().map(|s| eval_num(s, refs)).collect::<Result<Vec<_>, _>>().map(Some).map_err(|e| format!("{} : {}", key, e))
}

fn range(f: &ResolvedFigure, key: &str, refs: &Refs) -> Result<Option<(f64, f64)>, String> {
    Ok(nums(f, key, 2, refs)?.map(|v| (v[0], v[1])))
}

/// Nombre ou couple de nombres entiers (résolution, grille, densité)
fn counts(f: &ResolvedFigure, key: &str, default: (usize, usize), refs: &Refs) -> Result<(usize, usize), String> {
    match f.properties.get(key) {
        None => Ok(default),
        Some(v) => {
            let items = parts(v);
            let vals = items.iter().map(|s| eval_num(s, refs)).collect::<Result<Vec<_>, _>>()?;
            let c = |v: f64| v.round().clamp(0.0, 400.0) as usize;
            match vals[..] {
                [a] => Ok((c(a), c(a))),
                [a, b] | [a, b, _] => Ok((c(a), c(b))),
                _ => Err(format!("{} : un nombre ou (n1, n2) attendu", key)),
            }
        }
    }
}

/// Couleur : None = absente, Some(None) = none
fn color(f: &ResolvedFigure, key: &str) -> Result<Option<Option<Rgba>>, String> {
    match f.properties.get(key) {
        None => Ok(None),
        Some(v) => geometry::parse_color(&fmt_value(v)).map(Some).map_err(|e| format!("{} : {}", key, e)),
    }
}

/// Longueur en pixels : 4px, 4 ou 1% (du plus petit côté)
fn px(f: &ResolvedFigure, key: &str, default_px: f32, scene: &Scene) -> Result<f32, String> {
    match f.properties.get(key) {
        None => Ok(default_px * scene.px_scale),
        Some(v) => geometry::length(v, scene, scene.width.min(scene.height)).map_err(|e| format!("{} : {}", key, e)),
    }
}

fn text(f: &ResolvedFigure, key: &str) -> Option<String> {
    f.properties.get(key).map(fmt_value)
}

fn mix(a: Rgba, b: Rgba, e: f32) -> Rgba {
    Rgba { r: a.r + (b.r - a.r) * e, g: a.g + (b.g - a.g) * e, b: a.b + (b.b - a.b) * e, a: a.a + (b.a - a.a) * e }
}

// ─────────────────────────────────────────────
//  PALETTES
// ─────────────────────────────────────────────

pub(crate) const PALETTES: &[(&str, &[&str])] = &[
    ("viridis", &["#440154", "#482878", "#3E4A89", "#31688E", "#26828E", "#1F9E89", "#35B779", "#6DCD59", "#B4DE2C", "#FDE725"]),
    ("plasma", &["#0D0887", "#5B02A3", "#9A179B", "#CB4678", "#EB7852", "#FBB32F", "#F0F921"]),
    ("inferno", &["#000004", "#320A5E", "#781C6D", "#BB3754", "#ED6925", "#FBB61A", "#FCFFA4"]),
    ("magma", &["#000004", "#2C115F", "#721F81", "#B73779", "#F1605D", "#FEB078", "#FCFDBF"]),
    ("coolwarm", &["#3B4CC0", "#7092F2", "#AAC7FD", "#DDDDDD", "#F7B89C", "#E7745B", "#B40426"]),
    ("turbo", &["#30123B", "#4686FB", "#1BE5B5", "#A4FC3C", "#FBB938", "#E4460A", "#7A0403"]),
    ("blues", &["#F7FBFF", "#C6DBEF", "#6BAED6", "#2171B5", "#08306B"]),
    ("gray", &["#000000", "#FFFFFF"]),
];

pub fn palette_names() -> Vec<&'static str> {
    PALETTES.iter().map(|p| p.0).collect()
}

/// Palette : un nom (viridis, ...) ou une liste de couleurs [#000, #FFF]
pub(crate) fn palette(v: &PropValue) -> Result<Vec<Rgba>, String> {
    let colors: Vec<String> = match v {
        PropValue::List(items) => items.clone(),
        other => {
            let name = fmt_value(other);
            match PALETTES.iter().find(|p| p.0 == name) {
                Some(p) => p.1.iter().map(|s| s.to_string()).collect(),
                None => {
                    return Err(format!(
                        "colormap '{}' inconnue ({}, ou une liste de couleurs [#000000, #FFFFFF])",
                        name,
                        palette_names().join(", ")
                    ))
                }
            }
        }
    };
    let stops = colors
        .iter()
        .map(|c| geometry::parse_color(c)?.ok_or_else(|| "colormap : none interdit".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if stops.len() < 2 {
        return Err("colormap : au moins deux couleurs".to_string());
    }
    Ok(stops)
}

pub(crate) fn sample(stops: &[Rgba], v: f64) -> Rgba {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let x = v * (stops.len() - 1) as f64;
    let i = (x.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], (x - i as f64) as f32)
}

// ─────────────────────────────────────────────
//  POINTS 3D ET SURFACES PUBLIÉES
// ─────────────────────────────────────────────

/// Surface publiée pour les références : `f(x, y)` place un point dessus
#[derive(Debug, Clone)]
pub enum SurfaceRef {
    /// z = f(x, y)
    Explicit(Expr),
    /// (x, y, z) en fonction de (u, v)
    Param([Expr; 3]),
}

impl SurfaceRef {
    pub fn at(&self, a: f64, b: f64, params: &HashMap<String, f64>) -> V3 {
        let mut vars = params.clone();
        match self {
            SurfaceRef::Explicit(e) => {
                vars.insert("y".into(), b);
                [a, b, e.eval(a, &vars)]
            }
            SurfaceRef::Param(c) => {
                vars.insert("u".into(), a);
                vars.insert("v".into(), b);
                [c[0].eval(0.0, &vars), c[1].eval(0.0, &vars), c[2].eval(0.0, &vars)]
            }
        }
    }
}

/// Fonction d'un plot 2D publiée pour `iterate` : expression et nom du repère
#[derive(Debug, Clone)]
pub struct PlotRef {
    pub expr: Expr,
    pub axes: String,
}

/// Enregistre la formule d'un plot (pour `iterate` sur une courbe 2D)
pub fn register_plot(f: &ResolvedFigure, refs: &mut Refs) {
    let (Some(src), Some(axes)) = (text(f, "expr"), text(f, "axes")) else { return };
    if let Ok(e) = compile(&src, &[], refs) {
        refs.plots.insert(f.name.clone(), PlotRef { expr: e, axes });
    }
}

/// Un point 3D : espace(x, y, z), surface(a, b), center(objet), ou (x, y, z).
/// Renvoie le point et la scène qu'il désigne (None pour un simple triplet).
pub fn point3(s: &str, refs: &Refs) -> Result<(V3, Option<String>), String> {
    let s = s.trim();
    if s.starts_with('(') {
        let p = split_top(s.pipe_outer());
        if p.len() != 3 {
            return Err(format!("point 3D '{}' : (x, y, z) attendu", s));
        }
        return Ok(([eval_num(&p[0], refs)?, eval_num(&p[1], refs)?, eval_num(&p[2], refs)?], None));
    }
    let (name, args) = split_call(s);
    let vals = || args.iter().map(|a| eval_num(a, refs)).collect::<Result<Vec<f64>, String>>();
    if name == "center" && args.len() == 1 {
        let c = refs.centers3.get(args[0]).ok_or_else(|| format!("center({}) : pas un objet 3D", args[0]))?;
        return Ok((*c, refs.scene_of.get(args[0]).cloned()));
    }
    if refs.spaces.contains_key(name) {
        let v = vals()?;
        if v.len() != 3 {
            return Err(format!("'{}' : {}(x, y, z) attendu", s, name));
        }
        return Ok(([v[0], v[1], v[2]], Some(name.to_string())));
    }
    if let Some(surf) = refs.surfaces.get(name) {
        let v = vals()?;
        if v.len() != 2 {
            return Err(format!("'{}' : {}(a, b) attendu (un point de la surface)", s, name));
        }
        return Ok((surf.at(v[0], v[1], &refs.params), refs.scene_of.get(name).cloned()));
    }
    Err(format!("point 3D '{}' invalide : scene(x, y, z), surface(a, b), center(objet) ou (x, y, z)", s))
}

/// Point en pixels d'une référence 3D (pour les figures 2D : `pos : espace(1, 2, 3)`) ;
/// None si `s` ne désigne pas une scène ou une surface
pub fn point_px(s: &str, refs: &Refs) -> Option<Result<(f32, f32), String>> {
    let (name, args) = split_call(s);
    let is_3d = refs.spaces.contains_key(name)
        || refs.surfaces.contains_key(name)
        || (name == "center" && args.len() == 1 && refs.centers3.contains_key(args[0]));
    if !is_3d {
        return None;
    }
    Some(point3(s, refs).and_then(|(p, sc)| {
        let sc = sc.ok_or_else(|| format!("'{}' : scène inconnue", s))?;
        let space = refs.spaces.get(&sc).ok_or_else(|| format!("'{}' : scène '{}' pas calculée", s, sc))?;
        space.project(p).map(|(x, y, _)| (x, y)).ok_or_else(|| format!("'{}' est derrière la caméra", s))
    }))
}

/// Scène d'une figure : `scene`, celle de `on`, ou celle de son premier point 3D
pub fn scene_of(f: &ResolvedFigure, refs: &Refs) -> Option<String> {
    if let Some(s) = text(f, "scene") {
        return Some(s);
    }
    if let Some(on) = text(f, "on") {
        return refs.scene_of.get(&on).cloned();
    }
    for key in ["from", "center", "to", "points", "start"] {
        let Some(v) = f.properties.get(key) else { continue };
        let first = match v {
            PropValue::List(items) => items.first().cloned().unwrap_or_default(),
            other => fmt_value(other),
        };
        let (name, _) = split_call(&first);
        if refs.spaces.contains_key(name) {
            return Some(name.to_string());
        }
        if let Some(s) = refs.scene_of.get(name) {
            if refs.surfaces.contains_key(name) || name == "center" {
                return Some(s.clone());
            }
        }
        if name == "center" {
            let (_, args) = split_call(&first);
            if let Some(s) = args.first().and_then(|a| refs.scene_of.get(*a)) {
                return Some(s.clone());
            }
        }
    }
    None
}

/// La figure se dessine-t-elle dans une scène 3D ?
pub fn is_3d(f: &ResolvedFigure, refs: &Refs) -> bool {
    TYPES_3D.contains(&f.ftype.as_str())
        || (matches!(f.ftype.as_str(), "arrow" | "field" | "iterate" | "trajectory") && scene_of(f, refs).is_some())
}

// ─────────────────────────────────────────────
//  SCÈNE
// ─────────────────────────────────────────────

/// Construit la scène (caméra) et son repère
pub fn build_space(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<(Space, Obj), String> {
    let r = |key: &str| -> Result<(f64, f64), String> {
        let v = range(f, key, refs)?.unwrap_or((-3.0, 3.0));
        if v.1 > v.0 { Ok(v) } else { Err(format!("{} : [min, max] avec min < max", key)) }
    };
    let ranges = [r("xrange")?, r("yrange")?, r("zrange")?];
    let half = [0, 1, 2].map(|i| (ranges[i].1 - ranges[i].0) / 2.0);
    let mid = [0, 1, 2].map(|i| (ranges[i].0 + ranges[i].1) / 2.0);

    // Échelle : unités réelles (equal, défaut) ou boîte de proportions `box`
    let equal = !matches!(text(f, "equal").as_deref(), Some("false"));
    let k = if equal {
        let m = half.iter().fold(0.0f64, |a, b| a.max(*b));
        [1.0 / m; 3]
    } else {
        let b = nums(f, "box", 3, refs)?.unwrap_or(vec![1.0, 1.0, 0.8]);
        [b[0] / half[0], b[1] / half[1], b[2] / half[2]]
    };
    let radius_world = norm([half[0] * k[0], half[1] * k[1], half[2] * k[2]]).max(1e-9);

    // Caméra : (élévation, rotation), + la souris dans la fenêtre
    let cam = nums(f, "camera", 2, refs)?.unwrap_or(vec![30.0, -60.0]);
    let elev = (cam[0] + scene.orbit.0 as f64).clamp(-89.5, 89.5).to_radians();
    let azim = (cam[1] + scene.orbit.1 as f64).to_radians();
    let target_data = nums(f, "target", 3, refs)?.map(|v| [v[0], v[1], v[2]]).unwrap_or(mid);
    let target = [(target_data[0] - mid[0]) * k[0], (target_data[1] - mid[1]) * k[1], (target_data[2] - mid[2]) * k[2]];
    let fov = num(f, "fov", 30.0, refs)?.clamp(1.0, 150.0).to_radians();
    let dist = radius_world / (fov / 2.0).sin();
    let eye = add(target, mul([elev.cos() * azim.cos(), elev.cos() * azim.sin(), elev.sin()], dist));
    let fwd = unit(sub(target, eye));
    let right = unit(cross(fwd, [0.0, 0.0, 1.0]));
    let up = cross(right, fwd);

    // Zone de l'écran : from / to (défaut : toute la scène)
    let corner = |key: &str, default: (f32, f32)| -> Result<(f32, f32), String> {
        match f.properties.get(key) {
            Some(v) => geometry::point(v, scene, refs),
            None => Ok(default),
        }
    };
    let (x0, y0) = corner("from", (0.0, scene.height))?;
    let (x1, y1) = corner("to", (scene.width, 0.0))?;
    let (vw, vh) = ((x1 - x0).abs() as f64, (y1 - y0).abs() as f64);
    let radius_px = 0.5 * vw.min(vh) * 0.95;
    let zoom = num(f, "zoom", 1.0, refs)? * scene.orbit.2 as f64;
    let perspective = match text(f, "projection").as_deref() {
        None | Some("perspective") => true,
        Some("ortho") => false,
        Some(o) => return Err(format!("projection '{}' invalide (perspective ou ortho)", o)),
    };
    let focal = if perspective { radius_px / (fov / 2.0).tan() } else { radius_px / radius_world } * zoom;

    let light = match nums(f, "light", 3, refs)? {
        Some(v) => unit([v[0], v[1], v[2]]),
        None => unit(add(add(mul(fwd, -1.0), mul(up, 0.7)), mul(right, -0.4))),
    };
    let ambient = num(f, "ambient", 0.35, refs)?.clamp(0.0, 1.0);
    let space = Space {
        ranges,
        mid,
        k,
        eye,
        right,
        up,
        fwd,
        perspective,
        focal,
        center: (((x0 + x1) / 2.0) as f64, ((y0 + y1) / 2.0) as f64),
        light,
        ambient,
        dist,
    };
    let axes = scene_axes(f, &space, scene, refs)?;
    Ok((space, Obj { scene: f.name.clone(), items: axes, path: Vec::new() }))
}

/// Repère de la scène : `axes : lines` (ou true), `box`, `none` ; `floor : n` (grille au sol)
fn scene_axes(f: &ResolvedFigure, sp: &Space, scene: &Scene, refs: &Refs) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let col = color(f, "color")?.flatten().unwrap_or(Rgba::rgb8(0x88, 0x88, 0x88));
    let width = px(f, "stroke", 2.0, scene)?;
    let [rx, ry, rz] = sp.ranges;
    let labels = match f.properties.get("labels") {
        Some(v) => parts(v),
        None => vec!["x".into(), "y".into(), "z".into()],
    };
    let label_size = px(f, "font", 32.0, scene)?;
    let seg = |a: V3, b: V3, w: f32, c: Rgba, o: (f32, f32)| item(Prim::Seg { a, b, color: c, width: w, tube: false }, o, 0.0);
    let mode = text(f, "frame").unwrap_or_else(|| "none".into());
    match mode.as_str() {
        "none" | "false" => {}
        "lines" | "true" => {
            let clamp0 = |r: (f64, f64)| 0.0f64.clamp(r.0, r.1);
            let o = [clamp0(rx), clamp0(ry), clamp0(rz)];
            for (i, r) in [rx, ry, rz].iter().enumerate() {
                let (mut a, mut b) = (o, o);
                a[i] = r.0;
                b[i] = r.1;
                let order = (i as f32 / 3.0, (i + 1) as f32 / 3.0);
                // Découpé en morceaux pour se trier avec les surfaces
                for s in 0..16 {
                    let (p, q) = (lerp(a, b, s as f64 / 16.0), lerp(a, b, (s + 1) as f64 / 16.0));
                    items.push(seg(p, q, width, col, order));
                }
                let head = sp.span() * 0.04;
                let mut dir = [0.0; 3];
                dir[i] = 1.0;
                let tip = add(b, mul(dir, head));
                items.extend(cone_items(b, tip, head * 0.35, col, true, order, 0.0));
                if let Some(l) = labels.get(i).filter(|l| !l.is_empty()) {
                    let p = add(tip, mul(dir, head * 0.8));
                    items.push(item(Prim::Label { p, tex: l.clone(), size: label_size, color: col }, order, 0.0));
                }
            }
        }
        "box" => {
            let c = |i: usize, j: usize, k: usize| [[rx.0, rx.1][i], [ry.0, ry.1][j], [rz.0, rz.1][k]];
            let mut edges = Vec::new();
            for a in 0..2 {
                for b in 0..2 {
                    edges.push((c(0, a, b), c(1, a, b)));
                    edges.push((c(a, 0, b), c(a, 1, b)));
                    edges.push((c(a, b, 0), c(a, b, 1)));
                }
            }
            let n = edges.len() as f32;
            for (e, (a, b)) in edges.into_iter().enumerate() {
                for s in 0..8 {
                    let (p, q) = (lerp(a, b, s as f64 / 8.0), lerp(a, b, (s + 1) as f64 / 8.0));
                    items.push(seg(p, q, width * 0.6, col, (e as f32 / n, (e + 1) as f32 / n)));
                }
            }
            let off = sp.span() * 0.08;
            let spots = [
                [(rx.0 + rx.1) / 2.0, ry.0 - off, rz.0],
                [rx.1 + off, (ry.0 + ry.1) / 2.0, rz.0],
                [rx.0 - off, ry.1 + off, (rz.0 + rz.1) / 2.0],
            ];
            for (i, p) in spots.iter().enumerate() {
                if let Some(l) = labels.get(i).filter(|l| !l.is_empty()) {
                    items.push(item(Prim::Label { p: *p, tex: l.clone(), size: label_size, color: col }, (0.9, 1.0), 0.0));
                }
            }
        }
        o => return Err(format!("axes '{}' invalide (lines, box ou none)", o)),
    }
    let mut items = merge_runs(items);
    // Grille au sol
    let floor = num(f, "floor", 0.0, refs)?.round().max(0.0) as usize;
    if floor > 0 {
        let fc = Rgba { a: col.a * 0.35, ..col };
        for i in 0..=floor {
            let u = i as f64 / floor as f64;
            let x = rx.0 + (rx.1 - rx.0) * u;
            let y = ry.0 + (ry.1 - ry.0) * u;
            for s in 0..floor.max(1) {
                let (a, b) = (s as f64 / floor as f64, (s + 1) as f64 / floor as f64);
                let ya = ry.0 + (ry.1 - ry.0) * a;
                let yb = ry.0 + (ry.1 - ry.0) * b;
                let xa = rx.0 + (rx.1 - rx.0) * a;
                let xb = rx.0 + (rx.1 - rx.0) * b;
                items.push(seg([x, ya, rz.0], [x, yb, rz.0], width * 0.5, fc, (0.0, 0.3)));
                items.push(seg([xa, y, rz.0], [xb, y, rz.0], width * 0.5, fc, (0.0, 0.3)));
            }
        }
    }
    Ok(items)
}

// ─────────────────────────────────────────────
//  OBJETS
// ─────────────────────────────────────────────

/// Apparence commune des objets 3D
struct Look {
    fill: Option<Rgba>,
    line: Option<Rgba>,
    width: f32,
    opacity: f32,
    shade: bool,
    glow: f32,
    tube: Option<f32>,
}

fn look(f: &ResolvedFigure, scene: &Scene, refs: &Refs, fill: Option<Rgba>, line: Option<Rgba>, width: f32) -> Result<Look, String> {
    let opacity = num(f, "opacity", 1.0, refs)?.clamp(0.0, 1.0) as f32;
    let op = |c: Rgba| Rgba { a: c.a * opacity, ..c };
    Ok(Look {
        fill: color(f, "fill")?.unwrap_or(fill).map(op),
        line: color(f, "color")?.unwrap_or(line).map(op),
        width: px(f, "stroke", width, scene)?,
        opacity,
        shade: !matches!(text(f, "shading").as_deref(), Some("none")),
        glow: match f.properties.get("glow") {
            Some(_) => px(f, "glow", 0.0, scene)?,
            None => 0.0,
        },
        tube: match f.properties.get("tube") {
            Some(_) => Some(px(f, "tube", 6.0, scene)?),
            None => None,
        },
    })
}

const BLUE: Rgba = Rgba { r: 0.231, g: 0.510, b: 0.965, a: 1.0 };

/// Construit un objet 3D (sans ses animations)
pub fn build_object(f: &ResolvedFigure, sp: &Space, scene_name: &str, scene: &Scene, refs: &Refs) -> Result<(Obj, Option<SurfaceRef>), String> {
    let mut surf = None;
    let mut path = Vec::new();
    let items = match f.ftype.as_str() {
        "surface" => {
            // Le maillage (en coordonnées de la scène) ne dépend pas de la caméra : gardé d'une
            // image à l'autre tant que la surface, les paramètres et les bornes ne changent pas
            let extra = format!("{:?} {} {} {}", sp.ranges, scene.width, scene.height, scene.px_scale);
            let key = super::memo::key(&f.properties, &refs.params, &extra);
            let (items, s) = super::memo::get_or(&format!("3d:{}", f.name), key, || surface(f, sp, scene, refs))?;
            surf = Some(s);
            items
        }
        "curve3d" => curve3d(f, scene, refs, &mut path)?,
        "iterate" => {
            let pts = iterate_points(f, sp, refs)?;
            let it = trail(f, scene, refs, &pts, true)?;
            path = pts;
            it
        }
        "arrow" => {
            let (a, _) = point3(&text(f, "from").ok_or("propriété `from` manquante")?, refs)?;
            let (b, _) = point3(&text(f, "to").ok_or("propriété `to` manquante")?, refs)?;
            let lk = look(f, scene, refs, None, Some(Rgba::BLACK), 3.0)?;
            let head = num(f, "head", norm(sub(b, a)) * 0.2, refs)?;
            path = vec![a, b];
            arrow3(a, b, head, lk.line.unwrap_or(Rgba::BLACK), lk.width, lk.tube, lk.shade, (0.0, 1.0), lk.glow)
        }
        "field" => field3(f, sp, scene, refs)?,
        "trajectory" => {
            let (cs, _) = super::ode::curves(f, &refs.params, 3)?;
            let lk = look(f, scene, refs, None, Some(Rgba::BLACK), 2.0)?;
            let line = lk.line.unwrap_or(Rgba::BLACK);
            let colors = trajectory_colors(f, cs.len(), line)?;
            let r = px(f, "head", 0.0, scene)?;
            let mut items = Vec::new();
            for (c, col) in cs.iter().zip(&colors) {
                let pts = super::ode::thin(c, 3000);
                if pts.len() >= 2 && lk.line.is_some() {
                    items.extend(polyline(&pts, *col, lk.tube.unwrap_or(lk.width), lk.tube.is_some(), lk.glow));
                }
                if let (true, Some(p)) = (r > 0.0, pts.last()) {
                    items.push(item(Prim::Dot { p: *p, color: lk.fill.unwrap_or(*col), radius: r }, (1.0, 1.0), lk.glow));
                }
            }
            // Chemin pour follow : la première trajectoire
            path = cs.first().cloned().unwrap_or_default();
            items
        }
        "cube" | "box" | "sphere" | "cylinder" | "cone" | "plane" | "polyhedron" => solid(f, scene, refs)?,
        other => return Err(format!("type '{}' : pas un objet 3D", other)),
    };
    Ok((Obj { scene: scene_name.to_string(), items, path }, surf))
}

/// Maillage d'une surface paramétrée : faces (couleur par sommet) et lignes de grille
struct Mesh<'a> {
    pos: &'a mut dyn FnMut(f64, f64) -> V3,
    ur: (f64, f64),
    vr: (f64, f64),
    res: (usize, usize),
    grid: (usize, usize),
}

/// Coupe un polygone aux plans z = lo et z = hi (Sutherland–Hodgman)
fn clip_poly(pts: &[V3], lo: f64, hi: f64) -> Vec<V3> {
    let mut poly = pts.to_vec();
    for (bound, keep_below) in [(hi, true), (lo, false)] {
        let inside = |p: &V3| if keep_below { p[2] <= bound } else { p[2] >= bound };
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let cut = |a: V3, b: V3| lerp(a, b, (bound - a[2]) / (b[2] - a[2]));
            match (inside(&a), inside(&b)) {
                (true, true) => out.push(b),
                (true, false) => out.push(cut(a, b)),
                (false, true) => {
                    out.push(cut(a, b));
                    out.push(b);
                }
                (false, false) => {}
            }
        }
        poly = out;
        if poly.len() < 3 {
            return Vec::new();
        }
    }
    poly
}

/// Coupe un segment aux plans z = lo et z = hi ; None s'il est entièrement dehors
fn clip_seg(a: V3, b: V3, lo: f64, hi: f64) -> Option<(V3, V3)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let dz = b[2] - a[2];
    for (bound, below) in [(hi, true), (lo, false)] {
        let inside_a = if below { a[2] <= bound } else { a[2] >= bound };
        if dz.abs() < 1e-15 {
            if !inside_a {
                return None;
            }
            continue;
        }
        let t = (bound - a[2]) / dz;
        let entering = if below { dz < 0.0 } else { dz > 0.0 };
        if entering { t0 = t0.max(t) } else { t1 = t1.min(t) }
    }
    if t0 < t1 { Some((lerp(a, b, t0), lerp(a, b, t1))) } else { None }
}

/// Grille de points du maillage
fn mesh_points(m: &mut Mesh) -> Vec<Vec<V3>> {
    let (nu, nv) = (m.res.0.max(1), m.res.1.max(1));
    (0..=nu)
        .map(|i| {
            let u = m.ur.0 + (m.ur.1 - m.ur.0) * i as f64 / nu as f64;
            (0..=nv).map(|j| (m.pos)(u, m.vr.0 + (m.vr.1 - m.vr.0) * j as f64 / nv as f64)).collect()
        })
        .collect()
}

/// Faces et lignes de grille d'un maillage. `face_color` : couleur d'une face selon ses 4 coins
fn mesh_items(
    m: &mut Mesh,
    face_color: &dyn Fn(&[V3]) -> Option<Rgba>,
    line: Option<Rgba>,
    width: f32,
    shade: bool,
    glow: f32,
    clip: Option<(f64, f64)>,
) -> Vec<Item> {
    let pts = mesh_points(m);
    let seg = |a: V3, b: V3| -> Option<(V3, V3)> {
        match clip {
            Some((lo, hi)) => clip_seg(a, b, lo, hi),
            None => Some((a, b)),
        }
    };
    let (nu, nv) = (m.res.0.max(1), m.res.1.max(1));
    let mut items = Vec::new();
    let ord = |i: usize| (i as f32 / nu as f32, (i + 1) as f32 / nu as f32);
    for i in 0..nu {
        for j in 0..nv {
            let quad = vec![pts[i][j], pts[i + 1][j], pts[i + 1][j + 1], pts[i][j + 1]];
            if !quad.iter().all(|p| finite(*p)) {
                continue;
            }
            if let Some(c) = face_color(&quad) {
                let pts = match clip {
                    Some((lo, hi)) => clip_poly(&quad, lo, hi),
                    None => quad,
                };
                if pts.len() >= 3 {
                    items.push(item(Prim::Face { pts, color: c, shade }, ord(i), glow));
                }
            }
        }
    }
    let Some(line) = line else { return items };
    let (gu, gv) = m.grid;
    // Lignes à u constant (gu lignes), échantillonnées le long de v
    for g in 0..gu {
        let u = if gu == 1 { m.ur.0 } else { m.ur.0 + (m.ur.1 - m.ur.0) * g as f64 / (gu - 1) as f64 };
        let i = (((u - m.ur.0) / (m.ur.1 - m.ur.0)) * nu as f64).round() as usize;
        let row: Vec<V3> = (0..=nv).map(|j| (m.pos)(u, m.vr.0 + (m.vr.1 - m.vr.0) * j as f64 / nv as f64)).collect();
        for w in row.windows(2).filter(|w| finite(w[0]) && finite(w[1])) {
            if let Some((a, b)) = seg(w[0], w[1]) {
                items.push(item(Prim::Seg { a, b, color: line, width, tube: false }, ord(i.min(nu - 1)), glow));
            }
        }
    }
    // Lignes à v constant, échantillonnées le long de u
    for g in 0..gv {
        let v = if gv == 1 { m.vr.0 } else { m.vr.0 + (m.vr.1 - m.vr.0) * g as f64 / (gv - 1) as f64 };
        let col: Vec<V3> = (0..=nu).map(|i| (m.pos)(m.ur.0 + (m.ur.1 - m.ur.0) * i as f64 / nu as f64, v)).collect();
        for (i, w) in col.windows(2).enumerate().filter(|(_, w)| finite(w[0]) && finite(w[1])) {
            if let Some((a, b)) = seg(w[0], w[1]) {
                items.push(item(Prim::Seg { a, b, color: line, width, tube: false }, ord(i), glow));
            }
        }
    }
    items
}

/// surface : z = f(x, y), ou paramétrée (x, y, z) en fonction de (u, v)
fn surface(f: &ResolvedFigure, sp: &Space, scene: &Scene, refs: &Refs) -> Result<(Vec<Item>, SurfaceRef), String> {
    let (sref, ur, vr, parametric) = surface_def(f, sp, refs)?;
    let params = refs.params.clone();
    let mut pos = |a: f64, b: f64| sref.at(a, b, &params);
    let mut items = surface_mesh(f, sp, scene, refs, &mut pos, ur, vr, parametric)?;
    if f.properties.contains_key("levels") {
        if parametric {
            return Err("levels : seulement pour une surface z = f(x, y)".to_string());
        }
        items.extend(surface_levels(f, sp, scene, refs, &sref, ur, vr)?);
    }
    Ok((items, sref))
}

/// Lignes de niveau d'une surface z = f(x, y) : `levels : 10` ou `[0.5, 1, 2]`,
/// tracées sur la surface, au sol (`levels_on : floor`) ou les deux (`both`)
fn surface_levels(f: &ResolvedFigure, sp: &Space, scene: &Scene, refs: &Refs, sref: &SurfaceRef, ur: (f64, f64), vr: (f64, f64)) -> Result<Vec<Item>, String> {
    use super::contour::{levels, Field};
    let params = refs.params.clone();
    let res = counts(f, "resolution", (48, 48), refs)?;
    let (nx, ny) = ((res.0 * 3).clamp(30, 240), (res.1 * 3).clamp(30, 240));
    let field = Field::sample(&mut |x, y| sref.at(x, y, &params)[2], ur, vr, nx, ny);
    let zr = range(f, "zrange", refs)?.unwrap_or(sp.ranges[2]);
    // Niveaux dans zrange seulement (au-delà, la surface est découpée)
    let lv: Vec<f64> = levels(f.properties.get("levels"), false, Some(&field))?
        .into_iter()
        .filter(|c| *c >= zr.0 && *c <= zr.1)
        .collect();
    let (on_surface, on_floor) = match text(f, "levels_on").as_deref() {
        None | Some("surface") => (true, false),
        Some("floor") => (false, true),
        Some("both") => (true, true),
        Some(o) => return Err(format!("levels_on '{}' invalide (surface / floor / both)", o)),
    };
    let pal = match f.properties.get("colormap") {
        Some(v) => Some(palette(v)?),
        None => None,
    };
    let fixed = color(f, "level_color")?.flatten();
    let width = px(f, "level_stroke", 2.0, scene)?;
    let glow = match f.properties.get("glow") {
        Some(_) => px(f, "glow", 0.0, scene)?,
        None => 0.0,
    };
    let mut items = Vec::new();
    for (k, c) in lv.iter().enumerate() {
        let col = match (fixed, &pal) {
            (Some(c), _) => c,
            (None, Some(stops)) => mix(sample(stops, if lv.len() > 1 { k as f64 / (lv.len() - 1) as f64 } else { 0.5 }), Rgba::WHITE, 0.25),
            (None, None) => Rgba::WHITE,
        };
        for line in field.isolines(*c) {
            if on_surface {
                // Légèrement au-dessus : entre deux sommets du maillage, une face plane peut passer
                // un peu au-dessus de la vraie surface et cacher la ligne
                let lift = 0.008 * (zr.1 - zr.0);
                let pts: Vec<V3> = line.iter().map(|&(x, y)| [x, y, *c + lift]).collect();
                items.extend(polyline(&pts, col, width, false, glow));
            }
            if on_floor {
                let pts: Vec<V3> = line.iter().map(|&(x, y)| [x, y, sp.ranges[2].0]).collect();
                items.extend(polyline(&pts, col, width, false, glow));
            }
        }
    }
    Ok(items)
}

/// transform(to=b) entre deux surfaces : chaque point glisse de a vers b (domaines mis en
/// correspondance), puis la surface est découpée à zrange. Apparence de a.
pub fn morph_surfaces(fa: &ResolvedFigure, fb: &ResolvedFigure, e: f64, sp: &Space, scene: &Scene, refs: &Refs) -> Result<Vec<Item>, String> {
    let (sa, ua, va, pa) = surface_def(fa, sp, refs)?;
    let (sb, ub, vb, _) = surface_def(fb, sp, refs)?;
    let params = refs.params.clone();
    let at = |r: (f64, f64), k: f64| r.0 + (r.1 - r.0) * k;
    let mut pos = |s: f64, t: f64| lerp(sa.at(at(ua, s), at(va, t), &params), sb.at(at(ub, s), at(vb, t), &params), e);
    surface_mesh(fa, sp, scene, refs, &mut pos, (0.0, 1.0), (0.0, 1.0), pa)
}

/// Définition d'une surface : fonction, domaine (u ou x, v ou y), paramétrée ou non
fn surface_def(f: &ResolvedFigure, sp: &Space, refs: &Refs) -> Result<(SurfaceRef, (f64, f64), (f64, f64), bool), String> {
    let src = text(f, "expr").ok_or("propriété `expr` manquante")?;
    let parametric = src.trim_start().starts_with('(');
    let (sref, ur, vr) = if parametric {
        let c = split_top(src.trim().pipe_outer());
        if c.len() != 3 {
            return Err("expr paramétrée : (x(u, v), y(u, v), z(u, v)) attendu".to_string());
        }
        let e = [compile(&c[0], &["u", "v"], refs)?, compile(&c[1], &["u", "v"], refs)?, compile(&c[2], &["u", "v"], refs)?];
        let ur = range(f, "urange", refs)?.ok_or("surface paramétrée : `urange : [a, b]` manquant")?;
        let vr = range(f, "vrange", refs)?.ok_or("surface paramétrée : `vrange : [a, b]` manquant")?;
        (SurfaceRef::Param(e), ur, vr)
    } else {
        let e = compile(&src, &["y"], refs)?;
        let ur = range(f, "xrange", refs)?.unwrap_or(sp.ranges[0]);
        let vr = range(f, "yrange", refs)?.unwrap_or(sp.ranges[1]);
        (SurfaceRef::Explicit(e), ur, vr)
    };
    Ok((sref, ur, vr, parametric))
}

/// Maillage d'une surface avec son apparence : couleur ou palette, grille, découpe à zrange
#[allow(clippy::too_many_arguments)]
fn surface_mesh(
    f: &ResolvedFigure,
    sp: &Space,
    scene: &Scene,
    refs: &Refs,
    pos: &mut dyn FnMut(f64, f64) -> V3,
    ur: (f64, f64),
    vr: (f64, f64),
    parametric: bool,
) -> Result<Vec<Item>, String> {
    let res = counts(f, "resolution", if parametric { (48, 32) } else { (48, 48) }, refs)?;
    let grid = counts(f, "grid", (0, 0), refs)?;
    let lk = look(f, scene, refs, Some(BLUE), None, 1.0)?;
    let base = lk.fill;
    let line = match color(f, "color")? {
        Some(c) => c.map(|c| Rgba { a: c.a * lk.opacity, ..c }),
        None if grid.0 + grid.1 > 0 => base.map(|b| mix(b, Rgba::WHITE, 0.45)),
        None => None,
    };

    // Couleur selon une valeur (z par défaut) et une palette
    let pal = match f.properties.get("colormap") {
        Some(v) => Some(palette(v)?),
        None => None,
    };
    let color_by = match text(f, "color_by") {
        Some(s) => Some(compile(&s, &["y", "z", "u", "v"], refs)?),
        None => None,
    };
    let mut vars = vars(refs);
    let value = |p: V3, vars: &mut HashMap<String, f64>| match &color_by {
        Some(e) => {
            vars.insert("y".into(), p[1]);
            vars.insert("z".into(), p[2]);
            e.eval(p[0], vars)
        }
        None => p[2],
    };
    let mut m = Mesh { pos, ur, vr, res, grid };
    let crange = match range(f, "crange", refs)? {
        Some(r) => r,
        None => {
            // Couleur selon z : bornée à la partie visible (zrange), sinon une surface qui
            // monte très haut hors du cadre aurait presque une seule couleur
            let visible_z = if color_by.is_none() && !matches!(text(f, "clip").as_deref(), Some("false")) {
                Some(range(f, "zrange", refs)?.unwrap_or(sp.ranges[2]))
            } else {
                None
            };
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for row in mesh_points(&mut m) {
                for p in row.into_iter().filter(|p| finite(*p)) {
                    let v = value(p, &mut vars);
                    if v.is_finite() {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
            }
            if let Some(z) = visible_z {
                lo = lo.max(z.0);
                hi = hi.min(z.1);
            }
            if lo < hi { (lo, hi) } else { (lo, lo + 1.0) }
        }
    };
    let vars_cell = std::cell::RefCell::new(vars);
    let face_color = |q: &[V3]| -> Option<Rgba> {
        match &pal {
            Some(stops) => {
                let mut vars = vars_cell.borrow_mut();
                let v = q.iter().map(|p| value(*p, &mut vars)).sum::<f64>() / q.len() as f64;
                let c = sample(stops, (v - crange.0) / (crange.1 - crange.0));
                Some(Rgba { a: c.a * lk.opacity, ..c })
            }
            None => base,
        }
    };
    // Coupée à zrange (celui de la surface, sinon celui de la scène) ; clip : false pour tout garder
    let clip = match text(f, "clip").as_deref() {
        Some("false") => None,
        _ => Some(range(f, "zrange", refs)?.unwrap_or(sp.ranges[2])),
    };
    Ok(mesh_items(&mut m, &face_color, line, lk.width, lk.shade, lk.glow, clip))
}

/// Segments d'une ligne brisée, avec leur ordre d'apparition selon la longueur parcourue
fn polyline(pts: &[V3], color: Rgba, width: f32, tube: bool, glow: f32) -> Vec<Item> {
    let lens: Vec<f64> = pts.windows(2).map(|w| norm(sub(w[1], w[0]))).collect();
    let total: f64 = lens.iter().sum::<f64>().max(1e-12);
    let mut acc = 0.0;
    let mut items = Vec::new();
    for (w, l) in pts.windows(2).zip(&lens) {
        let o = ((acc / total) as f32, ((acc + l) / total) as f32);
        acc += l;
        if finite(w[0]) && finite(w[1]) {
            items.push(item(Prim::Seg { a: w[0], b: w[1], color, width, tube }, o, glow));
        }
    }
    // Courbes et trajectoires : segments regroupés (le quadrillage d'une surface, lui, reste
    // découpé case par case pour se trier avec ses faces)
    merge_runs(items)
}

/// curve3d : (x(t), y(t), z(t)) avec `t : [a, b]`, ou `points : [...]`
fn curve3d(f: &ResolvedFigure, scene: &Scene, refs: &Refs, path: &mut Vec<V3>) -> Result<Vec<Item>, String> {
    let pts: Vec<V3> = if let Some(PropValue::List(list)) = f.properties.get("points") {
        list.iter().map(|s| point3(s, refs).map(|p| p.0)).collect::<Result<_, _>>()?
    } else {
        let src = text(f, "expr").ok_or("curve3d : `expr : (x(t), y(t), z(t))` ou `points` attendu")?;
        let c = split_top(src.trim().pipe_outer());
        if c.len() != 3 {
            return Err("curve3d : expr = (x(t), y(t), z(t)) attendu".to_string());
        }
        let e = c.iter().map(|s| compile(s, &["t"], refs)).collect::<Result<Vec<_>, _>>()?;
        let tr = range(f, "t", refs)?.unwrap_or((0.0, 1.0));
        let n = num(f, "samples", 200.0, refs)?.clamp(2.0, 20000.0) as usize;
        let mut vars = vars(refs);
        (0..=n)
            .map(|i| {
                let t = tr.0 + (tr.1 - tr.0) * i as f64 / n as f64;
                vars.insert("t".into(), t);
                [e[0].eval(0.0, &vars), e[1].eval(0.0, &vars), e[2].eval(0.0, &vars)]
            })
            .collect()
    };
    *path = pts.clone();
    let lk = look(f, scene, refs, None, Some(Rgba::BLACK), 3.0)?;
    let c = lk.line.unwrap_or(Rgba::BLACK);
    let mut items = polyline(&pts, c, lk.tube.unwrap_or(lk.width), lk.tube.is_some(), lk.glow);
    let r = px(f, "markers", 0.0, scene)?;
    if r > 0.0 {
        let mc = lk.fill.unwrap_or(c);
        let n = pts.len().max(2) - 1;
        for (i, p) in pts.iter().enumerate() {
            let o = (i as f32 / n as f32, i as f32 / n as f32);
            items.push(item(Prim::Dot { p: *p, color: mc, radius: r }, o, lk.glow));
        }
    }
    Ok(items)
}

/// Faces d'un cône (pointe en `tip`, base de rayon `r` centrée en `base`)
fn cone_items(base: V3, tip: V3, r: f64, color: Rgba, shade: bool, order: (f32, f32), glow: f32) -> Vec<Item> {
    let (a, b) = basis(sub(tip, base));
    let n = 14;
    let ring: Vec<V3> = (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            add(base, add(mul(a, r * t.cos()), mul(b, r * t.sin())))
        })
        .collect();
    let mut items: Vec<Item> = (0..n)
        .map(|i| item(Prim::Face { pts: vec![ring[i], ring[(i + 1) % n], tip], color, shade }, order, glow))
        .collect();
    items.push(item(Prim::Face { pts: ring, color, shade }, order, glow));
    items
}

/// Flèche 3D : tige (segments) et pointe (cône)
fn arrow3(a: V3, b: V3, head: f64, color: Rgba, width: f32, tube: Option<f32>, shade: bool, order: (f32, f32), glow: f32) -> Vec<Item> {
    let len = norm(sub(b, a));
    if len <= 0.0 {
        return Vec::new();
    }
    let head = head.clamp(0.0, len);
    let base = lerp(b, a, head / len);
    let pieces = 8;
    let mut items = Vec::new();
    for i in 0..pieces {
        let (p, q) = (lerp(a, base, i as f64 / pieces as f64), lerp(a, base, (i + 1) as f64 / pieces as f64));
        let o = (order.0 + (order.1 - order.0) * 0.8 * i as f32 / pieces as f32, order.0 + (order.1 - order.0) * 0.8 * (i + 1) as f32 / pieces as f32);
        items.push(item(Prim::Seg { a: p, b: q, color, width: tube.unwrap_or(width), tube: tube.is_some() }, o, glow));
    }
    let mut items = merge_runs(items);
    let head_order = (order.0 + (order.1 - order.0) * 0.8, order.1);
    items.extend(cone_items(base, b, head * 0.38, color, shade, head_order, glow));
    items
}

/// Champ de vecteurs 3D : (fx, fy, fz) en fonction de (x, y, z)
fn field3(f: &ResolvedFigure, sp: &Space, scene: &Scene, refs: &Refs) -> Result<Vec<Item>, String> {
    let src = text(f, "expr").ok_or("field : `expr : (fx, fy, fz)` manquant")?;
    let c = split_top(src.trim().pipe_outer());
    if c.len() != 3 {
        return Err("field en 3D : expr = (fx, fy, fz) attendu".to_string());
    }
    let e = c.iter().map(|s| compile(s, &["y", "z"], refs)).collect::<Result<Vec<_>, _>>()?;
    let n = counts(f, "density", (5, 5), refs)?;
    let nz = match f.properties.get("density").map(parts) {
        Some(p) if p.len() == 3 => eval_num(&p[2], refs)?.round().max(1.0) as usize,
        _ => n.0,
    };
    let rs = [
        range(f, "xrange", refs)?.unwrap_or(sp.ranges[0]),
        range(f, "yrange", refs)?.unwrap_or(sp.ranges[1]),
        range(f, "zrange", refs)?.unwrap_or(sp.ranges[2]),
    ];
    let counts3 = [n.0.max(1), n.1.max(1), nz.max(1)];
    let at = |i: usize, k: usize| {
        if counts3[i] == 1 { (rs[i].0 + rs[i].1) / 2.0 } else { rs[i].0 + (rs[i].1 - rs[i].0) * k as f64 / (counts3[i] - 1) as f64 }
    };
    let cell = (0..3).map(|i| (rs[i].1 - rs[i].0) / counts3[i].max(2) as f64).fold(f64::MAX, f64::min);
    let mut vars = vars(refs);
    let mut vecs = Vec::new();
    for i in 0..counts3[0] {
        for j in 0..counts3[1] {
            for k in 0..counts3[2] {
                let p = [at(0, i), at(1, j), at(2, k)];
                vars.insert("y".into(), p[1]);
                vars.insert("z".into(), p[2]);
                let v = [e[0].eval(p[0], &vars), e[1].eval(p[0], &vars), e[2].eval(p[0], &vars)];
                if finite(v) {
                    vecs.push((p, v));
                }
            }
        }
    }
    let lk = look(f, scene, refs, None, Some(Rgba::BLACK), 2.0)?;
    let max = vecs.iter().map(|(_, v)| norm(*v)).fold(0.0, f64::max).max(1e-12);
    let normalize = matches!(text(f, "normalize").as_deref(), Some("true"));
    let scale = num(f, "length", 0.9, refs)? * cell;
    let pal = match f.properties.get("colormap") {
        Some(v) => Some(palette(v)?),
        None => None,
    };
    let count = vecs.len().max(1) as f32;
    let mut items = Vec::new();
    for (idx, (p, v)) in vecs.iter().enumerate() {
        let m = norm(*v);
        if m <= 0.0 {
            continue;
        }
        let len = if normalize { scale } else { scale * m / max };
        let q = add(*p, mul(unit(*v), len));
        let c = match &pal {
            Some(s) => {
                let c = sample(s, m / max);
                Rgba { a: c.a * lk.opacity, ..c }
            }
            None => lk.line.unwrap_or(Rgba::BLACK),
        };
        let o = (idx as f32 / count, (idx + 1) as f32 / count);
        items.extend(arrow3(*p, q, len * 0.3, c, lk.width, lk.tube, lk.shade, o, lk.glow));
    }
    Ok(items)
}

/// Solides : cube, box, sphere, cylinder, cone, plane, polyhedron
fn solid(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<Vec<Item>, String> {
    let center = match text(f, "center") {
        Some(s) => point3(&s, refs)?.0,
        None => [0.0; 3],
    };
    let orient = euler(nums(f, "rotate", 3, refs)?.map(|v| [v[0], v[1], v[2]]).unwrap_or([0.0; 3]));
    let place = |p: V3| add(center, mat_vec(&orient, p));
    let lk = look(f, scene, refs, Some(BLUE), None, 2.0)?;
    let edge = lk.line;
    let grid = counts(f, "grid", (0, 0), refs)?;
    let res = num(f, "resolution", 0.0, refs)?.round() as usize;
    let mut items = Vec::new();
    // Face plane découpée en n × n (pour bien se trier avec les autres objets)
    let mut flat = |corners: [V3; 4], n: usize, order: (f32, f32)| {
        let Some(fill) = lk.fill else { return };
        for i in 0..n {
            for j in 0..n {
                let at = |a: f64, b: f64| lerp(lerp(corners[0], corners[1], a), lerp(corners[3], corners[2], a), b);
                let (a0, a1, b0, b1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64, j as f64 / n as f64, (j + 1) as f64 / n as f64);
                let pts = vec![at(a0, b0), at(a1, b0), at(a1, b1), at(a0, b1)].into_iter().map(place).collect();
                items.push(item(Prim::Face { pts, color: fill, shade: lk.shade }, order, lk.glow));
            }
        }
    };
    let mut edges: Vec<(V3, V3)> = Vec::new();
    match f.ftype.as_str() {
        "cube" | "box" => {
            let s = if f.ftype == "cube" {
                let a = num(f, "size", 1.0, refs)?;
                [a, a, a]
            } else {
                nums(f, "size", 3, refs)?.map(|v| [v[0], v[1], v[2]]).unwrap_or([1.0, 1.0, 1.0])
            };
            let h = mul(s, 0.5);
            let v = |i: usize| [if i & 1 == 0 { -h[0] } else { h[0] }, if i & 2 == 0 { -h[1] } else { h[1] }, if i & 4 == 0 { -h[2] } else { h[2] }];
            let faces = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];
            let n = if res > 0 { res } else { 4 };
            for (k, fc) in faces.iter().enumerate() {
                flat([v(fc[0]), v(fc[1]), v(fc[2]), v(fc[3])], n, (k as f32 / 6.0, (k + 1) as f32 / 6.0));
            }
            for a in 0..8 {
                for b in [1, 2, 4] {
                    if a & b == 0 {
                        edges.push((v(a), v(a | b)));
                    }
                }
            }
        }
        "plane" => {
            let s = nums(f, "size", 2, refs)?.map(|v| (v[0], v[1])).unwrap_or((2.0, 2.0));
            let (a, b) = (s.0 / 2.0, s.1 / 2.0);
            let c = [[-a, -b, 0.0], [a, -b, 0.0], [a, b, 0.0], [-a, b, 0.0]];
            flat(c, if res > 0 { res } else { 6 }, (0.0, 1.0));
            for i in 0..4 {
                edges.push((c[i], c[(i + 1) % 4]));
            }
            // Grille sur le plan
            if grid.0 > 1 {
                for g in 1..grid.0 - 1 {
                    let u = -a + 2.0 * a * g as f64 / (grid.0 - 1) as f64;
                    edges.push(([u, -b, 0.0], [u, b, 0.0]));
                }
            }
            if grid.1 > 1 {
                for g in 1..grid.1 - 1 {
                    let v = -b + 2.0 * b * g as f64 / (grid.1 - 1) as f64;
                    edges.push(([-a, v, 0.0], [a, v, 0.0]));
                }
            }
        }
        "sphere" | "cylinder" | "cone" => {
            let r = num(f, "radius", 1.0, refs)?;
            let h = num(f, "height", 2.0, refs)?;
            let tau = std::f64::consts::TAU;
            let kind = f.ftype.clone();
            let mut pos = move |u: f64, v: f64| -> V3 {
                match kind.as_str() {
                    "sphere" => [r * v.sin() * u.cos(), r * v.sin() * u.sin(), r * v.cos()],
                    "cylinder" => [r * u.cos(), r * u.sin(), -h / 2.0 + h * v],
                    _ => [r * (1.0 - v) * u.cos(), r * (1.0 - v) * u.sin(), -h / 2.0 + h * v],
                }
            };
            let (vr, nv) = if f.ftype == "sphere" { ((0.0, std::f64::consts::PI), 16) } else { ((0.0, 1.0), 6) };
            let n = if res > 0 { res } else { 32 };
            let mut m = Mesh { pos: &mut pos, ur: (0.0, tau), vr, res: (n, if res > 0 { res / 2 } else { nv }), grid };
            let fill = lk.fill;
            let local = mesh_items(&mut m, &|_| fill, edge, lk.width, lk.shade, lk.glow, None);
            for mut it in local {
                it.prim.map(&place);
                items.push(it);
            }
            // Disques des bases
            if f.ftype != "sphere" {
                if let Some(fill) = lk.fill {
                    let ring = |z: f64| -> Vec<V3> { (0..n).map(|i| place([r * (tau * i as f64 / n as f64).cos(), r * (tau * i as f64 / n as f64).sin(), z])).collect() };
                    let bottom = ring(-h / 2.0);
                    for i in 0..n {
                        items.push(item(Prim::Face { pts: vec![place([0.0, 0.0, -h / 2.0]), bottom[i], bottom[(i + 1) % n]], color: fill, shade: lk.shade }, (0.0, 1.0), lk.glow));
                    }
                    if f.ftype == "cylinder" {
                        let top = ring(h / 2.0);
                        for i in 0..n {
                            items.push(item(Prim::Face { pts: vec![place([0.0, 0.0, h / 2.0]), top[i], top[(i + 1) % n]], color: fill, shade: lk.shade }, (0.0, 1.0), lk.glow));
                        }
                    }
                }
            }
        }
        "polyhedron" => {
            let pts: Vec<V3> = match f.properties.get("points") {
                Some(PropValue::List(list)) => list.iter().map(|s| point3(s, refs).map(|p| p.0)).collect::<Result<_, _>>()?,
                _ => return Err("polyhedron : `points : [(x, y, z), ...]` manquant".to_string()),
            };
            let faces: Vec<Vec<usize>> = match f.properties.get("faces") {
                Some(PropValue::List(list)) => list
                    .iter()
                    .map(|s| {
                        split_top(s.trim().pipe_outer())
                            .iter()
                            .map(|i| i.trim().parse::<usize>().ok().filter(|i| *i < pts.len()))
                            .collect::<Option<Vec<usize>>>()
                            .filter(|v| v.len() >= 3)
                            .ok_or_else(|| format!("polyhedron : face '{}' invalide (indices de points, à partir de 0)", s))
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err("polyhedron : `faces : [(0, 1, 2), ...]` manquant".to_string()),
            };
            let nf = faces.len().max(1) as f32;
            for (k, fc) in faces.iter().enumerate() {
                if let Some(fill) = lk.fill {
                    items.push(item(Prim::Face { pts: fc.iter().map(|&i| place(pts[i])).collect(), color: fill, shade: lk.shade }, (k as f32 / nf, (k + 1) as f32 / nf), lk.glow));
                }
                for w in 0..fc.len() {
                    let (a, b) = (fc[w], fc[(w + 1) % fc.len()]);
                    if a < b || !faces.iter().any(|g| g.windows(2).any(|p| p[0] == b && p[1] == a) || (g.last() == Some(&b) && g.first() == Some(&a))) {
                        edges.push((pts[a], pts[b]));
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    if let Some(c) = edge {
        let n = edges.len().max(1) as f32;
        for (k, (a, b)) in edges.into_iter().enumerate() {
            for s in 0..4 {
                let (p, q) = (lerp(a, b, s as f64 / 4.0), lerp(a, b, (s + 1) as f64 / 4.0));
                items.push(item(Prim::Seg { a: place(p), b: place(q), color: c, width: lk.width, tube: false }, (k as f32 / n, (k + 1) as f32 / n), lk.glow));
            }
        }
    }
    Ok(items)
}

// ─────────────────────────────────────────────
//  ITERATE : RÉCURRENCES ET OPTIMISEURS
// ─────────────────────────────────────────────

/// La fonction sur laquelle on itère (surface ou courbe), si elle existe
struct Landscape<'a> {
    f: Box<dyn Fn(&[f64]) -> f64 + 'a>,
    dims: usize,
}

impl Landscape<'_> {
    fn grad(&self, x: &[f64]) -> Vec<f64> {
        (0..self.dims)
            .map(|i| {
                let h = 1e-5 * (1.0 + x[i].abs());
                let (mut a, mut b) = (x.to_vec(), x.to_vec());
                a[i] += h;
                b[i] -= h;
                ((self.f)(&a) - (self.f)(&b)) / (2.0 * h)
            })
            .collect()
    }

    fn hessian(&self, x: &[f64]) -> Vec<Vec<f64>> {
        (0..self.dims)
            .map(|i| {
                let h = 1e-4 * (1.0 + x[i].abs());
                let (mut a, mut b) = (x.to_vec(), x.to_vec());
                a[i] += h;
                b[i] -= h;
                let (ga, gb) = (self.grad(&a), self.grad(&b));
                (0..self.dims).map(|j| (ga[j] - gb[j]) / (2.0 * h)).collect()
            })
            .collect()
    }
}

/// La fonction de `on` : surface explicite (2 variables) ou plot (1 variable)
fn landscape<'a>(f: &ResolvedFigure, refs: &'a Refs) -> Result<Option<Landscape<'a>>, String> {
    let Some(on) = text(f, "on") else { return Ok(None) };
    if let Some(s) = refs.surfaces.get(&on) {
        let SurfaceRef::Explicit(e) = s else {
            return Err(format!("iterate : '{}' est une surface paramétrée ; il faut z = f(x, y)", on));
        };
        let params = &refs.params;
        return Ok(Some(Landscape {
            f: Box::new(move |p: &[f64]| {
                let mut v = params.clone();
                v.insert("y".into(), p[1]);
                e.eval(p[0], &v)
            }),
            dims: 2,
        }));
    }
    if let Some(p) = refs.plots.get(&on) {
        let params = &refs.params;
        let e = &p.expr;
        return Ok(Some(Landscape { f: Box::new(move |x: &[f64]| e.eval(x[0], params)), dims: 1 }));
    }
    Err(format!("iterate : on={} doit être une surface (z = f(x, y)) ou un plot", on))
}

/// Suite des états de l'itération (chaque état = valeurs des variables)
fn iterate_states(f: &ResolvedFigure, refs: &Refs, land: Option<&Landscape>) -> Result<Vec<Vec<f64>>, String> {
    let steps = num(f, "steps", 30.0, refs)?.clamp(0.0, 100_000.0) as usize;
    let mut states = Vec::new();
    let ok = |x: &[f64]| x.iter().all(|v| v.is_finite() && v.abs() < 1e9);

    if let Some(update) = f.properties.get("update") {
        // Récurrence générale : vars, init, update
        let names: Vec<String> = f.properties.get("vars").map(parts).ok_or("iterate : `vars : (x, y, ...)` manquant")?;
        let init = f.properties.get("init").map(parts).ok_or("iterate : `init : (...)` manquant")?;
        let upd = parts(update);
        if init.len() != names.len() || upd.len() != names.len() {
            return Err(format!("iterate : vars, init et update doivent avoir autant d'éléments ({})", names.len()));
        }
        let mut locals: Vec<&str> = names.iter().map(|s| s.as_str()).filter(|s| *s != "x").collect();
        locals.extend(ITERATE_LOCALS);
        let exprs = upd.iter().map(|s| compile(s, &locals, refs)).collect::<Result<Vec<_>, _>>()?;
        let mut x: Vec<f64> = init.iter().map(|s| eval_num(s, refs)).collect::<Result<_, _>>()?;
        let mut v = vars(refs);
        states.push(x.clone());
        for n in 0..steps {
            for (name, val) in names.iter().zip(&x) {
                v.insert(name.clone(), *val);
            }
            v.insert("n".into(), n as f64);
            let xv = names.iter().position(|s| s == "x").map(|i| x[i]).unwrap_or(0.0);
            if let Some(l) = land {
                let p: Vec<f64> = x.iter().take(l.dims).copied().collect();
                if p.len() == l.dims {
                    let g = l.grad(&p);
                    let h = l.hessian(&p);
                    v.insert("f".into(), (l.f)(&p));
                    v.insert("dx".into(), g[0]);
                    v.insert("dy".into(), *g.get(1).unwrap_or(&0.0));
                    v.insert("hxx".into(), h[0][0]);
                    v.insert("hxy".into(), if l.dims > 1 { h[0][1] } else { 0.0 });
                    v.insert("hyy".into(), if l.dims > 1 { h[1][1] } else { 0.0 });
                }
            }
            let next: Vec<f64> = exprs.iter().map(|e| e.eval(xv, &v)).collect();
            if !ok(&next) {
                break;
            }
            x = next;
            states.push(x.clone());
        }
        return Ok(states);
    }

    // Méthodes intégrées : il faut une fonction
    let land = land.ok_or("iterate : `on : surface` (ou un plot) pour une méthode d'optimisation, sinon vars / init / update")?;
    let method = text(f, "method").unwrap_or_else(|| "gd".into());
    if !METHODS.contains(&method.as_str()) {
        return Err(format!("iterate : méthode '{}' inconnue ({}), ou vars / init / update", method, METHODS.join(", ")));
    }
    let start = f.properties.get("start").map(parts).ok_or("iterate : `start : (x, y)` manquant")?;
    let mut x: Vec<f64> = start.iter().map(|s| eval_num(s, refs)).collect::<Result<_, _>>()?;
    if x.len() != land.dims {
        return Err(format!("iterate : start attend {} valeur(s)", land.dims));
    }
    let rate = num(f, "rate", if method == "newton" { 1.0 } else { 0.1 }, refs)?;
    let beta = num(f, "beta", 0.9, refs)?;
    let beta2 = num(f, "beta2", 0.999, refs)?;
    let eps = num(f, "eps", 1e-8, refs)?;
    let d = land.dims;
    let (mut vel, mut m, mut s) = (vec![0.0; d], vec![0.0; d], vec![0.0; d]);
    states.push(x.clone());
    for n in 1..=steps {
        let next: Vec<f64> = match method.as_str() {
            "gd" => {
                let g = land.grad(&x);
                (0..d).map(|i| x[i] - rate * g[i]).collect()
            }
            "momentum" => {
                let g = land.grad(&x);
                (0..d).for_each(|i| vel[i] = beta * vel[i] - rate * g[i]);
                (0..d).map(|i| x[i] + vel[i]).collect()
            }
            "nesterov" => {
                let ahead: Vec<f64> = (0..d).map(|i| x[i] + beta * vel[i]).collect();
                let g = land.grad(&ahead);
                (0..d).for_each(|i| vel[i] = beta * vel[i] - rate * g[i]);
                (0..d).map(|i| x[i] + vel[i]).collect()
            }
            "rmsprop" => {
                let g = land.grad(&x);
                (0..d).for_each(|i| s[i] = beta * s[i] + (1.0 - beta) * g[i] * g[i]);
                (0..d).map(|i| x[i] - rate * g[i] / (s[i].sqrt() + eps)).collect()
            }
            "adam" => {
                let g = land.grad(&x);
                (0..d).for_each(|i| {
                    m[i] = beta * m[i] + (1.0 - beta) * g[i];
                    s[i] = beta2 * s[i] + (1.0 - beta2) * g[i] * g[i];
                });
                let (c1, c2) = (1.0 - beta.powi(n as i32), 1.0 - beta2.powi(n as i32));
                (0..d).map(|i| x[i] - rate * (m[i] / c1) / ((s[i] / c2).sqrt() + eps)).collect()
            }
            _ => {
                // Newton : pas = H⁻¹ g, avec un amortissement si H n'est pas définie positive
                let g = land.grad(&x);
                let h = land.hessian(&x);
                let step: Vec<f64> = if d == 1 {
                    let a = if h[0][0] > 1e-9 { h[0][0] } else { h[0][0].abs() + 1.0 };
                    vec![g[0] / a]
                } else {
                    let mut lambda = 0.0;
                    loop {
                        let (a, b, c) = (h[0][0] + lambda, (h[0][1] + h[1][0]) / 2.0, h[1][1] + lambda);
                        let det = a * c - b * b;
                        if a > 1e-9 && det > 1e-12 {
                            break vec![(c * g[0] - b * g[1]) / det, (a * g[1] - b * g[0]) / det];
                        }
                        lambda = if lambda == 0.0 { 1e-3 } else { lambda * 10.0 };
                        if lambda > 1e9 {
                            break g.clone();
                        }
                    }
                };
                (0..d).map(|i| x[i] - rate * step[i]).collect()
            }
        };
        if !ok(&next) {
            break;
        }
        x = next;
        states.push(x.clone());
    }
    Ok(states)
}

/// Points 3D de la trajectoire : posés sur la surface (`on`), ou (x, y, z) des trois premières variables
fn iterate_points(f: &ResolvedFigure, sp: &Space, refs: &Refs) -> Result<Vec<V3>, String> {
    let land = landscape(f, refs)?;
    let states = iterate_states(f, refs, land.as_ref())?;
    let lift = num(f, "lift", (sp.ranges[2].1 - sp.ranges[2].0) * 0.01, refs)?;
    let on_surface = land.as_ref().filter(|l| l.dims == 2);
    let drape = matches!(text(f, "drape").as_deref(), Some("true"));
    let mut pts: Vec<V3> = Vec::new();
    for (i, s) in states.iter().enumerate() {
        let p = match on_surface {
            Some(l) => [s[0], s[1], (l.f)(&s[..2]) + lift],
            None => [s[0], *s.get(1).unwrap_or(&0.0), *s.get(2).unwrap_or(&0.0)],
        };
        // drape : entre deux pas, le chemin suit la surface au lieu d'aller en ligne droite
        if let (true, Some(l), Some(prev)) = (drape, on_surface, i.checked_sub(1).map(|j| &states[j])) {
            for k in 1..12 {
                let e = k as f64 / 12.0;
                let (x, y) = (prev[0] + (s[0] - prev[0]) * e, prev[1] + (s[1] - prev[1]) * e);
                pts.push([x, y, (l.f)(&[x, y]) + lift]);
            }
        }
        pts.push(p);
    }
    Ok(pts)
}

/// Dessin d'une trajectoire : segments (ou tube) et points à chaque pas
fn trail(f: &ResolvedFigure, scene: &Scene, refs: &Refs, pts: &[V3], dots: bool) -> Result<Vec<Item>, String> {
    let lk = look(f, scene, refs, None, Some(Rgba::rgb8(0xF5, 0xC5, 0x18)), 2.0)?;
    let mut items = Vec::new();
    if let Some(c) = lk.line {
        items = polyline(pts, c, lk.tube.unwrap_or(lk.width), lk.tube.is_some(), lk.glow);
    }
    let r = px(f, "markers", 4.0, scene)?;
    if dots && r > 0.0 && !matches!(text(f, "drape").as_deref(), Some("true")) {
        let c = lk.fill.or(lk.line).unwrap_or(Rgba::BLACK);
        let n = pts.len().max(2) - 1;
        for (i, p) in pts.iter().enumerate() {
            let o = (i as f32 / n as f32, i as f32 / n as f32);
            items.push(item(Prim::Dot { p: *p, color: c, radius: r }, o, lk.glow));
        }
    }
    Ok(items)
}

// ─────────────────────────────────────────────
//  VERSIONS 2D : arrow, field, iterate dans un repère
// ─────────────────────────────────────────────

/// arrow, field et iterate quand ils ne sont pas dans une scène 3D
/// Couleur de chaque trajectoire : `colormap` (selon k), sinon `color`
fn trajectory_colors(f: &ResolvedFigure, n: usize, line: Rgba) -> Result<Vec<Rgba>, String> {
    Ok(match f.properties.get("colormap") {
        Some(v) => {
            let stops = palette(v)?;
            (0..n).map(|k| Rgba { a: line.a, ..sample(&stops, if n > 1 { k as f64 / (n - 1) as f64 } else { 0.5 }) }).collect()
        }
        None => vec![line; n],
    })
}

pub fn shapes_2d(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<Vec<Shape>, String> {
    let lk = look(f, scene, refs, None, Some(Rgba::BLACK), 2.0)?;
    let line = lk.line.unwrap_or(Rgba::BLACK);
    let width = lk.width;
    let arrow2 = |a: (f32, f32), b: (f32, f32), c: Rgba, w: f32| -> Vec<Shape> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.5 {
            return Vec::new();
        }
        let (ux, uy) = (dx / len, dy / len);
        let head = geometry::arrow_length(w, scene).min(len * 0.45);
        let end = (b.0 - ux * head * 0.8, b.1 - uy * head * 0.8);
        vec![
            Shape { glow: 0.0, path: vec![PathCmd::MoveTo(a.0, a.1), PathCmd::LineTo(end.0, end.1)], fill: None, stroke: Some((c, w)), image: None },
            geometry::arrow_head(b.0, b.1, ux, uy, head, c),
        ]
    };
    let axes_of = |name: &str| -> Result<AxesMap, String> {
        geometry::resolve_geom(name, refs)?.axes.clone().ok_or_else(|| format!("'{}' n'est pas un repère axes", name))
    };
    let mut shapes = match f.ftype.as_str() {
        "arrow" => {
            let a = geometry::point(geometry::require(f, "from")?, scene, refs)?;
            let b = geometry::point(geometry::require(f, "to")?, scene, refs)?;
            arrow2(a, b, line, width)
        }
        "field" => {
            let m = axes_of(&text(f, "axes").ok_or("field : `axes : repere` ou `scene : ...` manquant")?)?;
            let src = text(f, "expr").ok_or("field : `expr : (fx, fy)` manquant")?;
            let c = split_top(src.trim().pipe_outer());
            if c.len() != 2 {
                return Err("field en 2D : expr = (fx, fy) attendu".to_string());
            }
            let e = [compile(&c[0], &["y"], refs)?, compile(&c[1], &["y"], refs)?];
            let (nx, ny) = counts(f, "density", (12, 12), refs)?;
            let xr = range(f, "xrange", refs)?.unwrap_or(m.xrange);
            let yr = range(f, "yrange", refs)?.unwrap_or(m.yrange);
            let at = |r: (f64, f64), n: usize, k: usize| if n <= 1 { (r.0 + r.1) / 2.0 } else { r.0 + (r.1 - r.0) * (k as f64 + 0.5) / n as f64 };
            let mut vars = vars(refs);
            let mut vecs = Vec::new();
            for i in 0..nx.max(1) {
                for j in 0..ny.max(1) {
                    let (x, y) = (at(xr, nx, i), at(yr, ny, j));
                    vars.insert("y".into(), y);
                    let v = (e[0].eval(x, &vars), e[1].eval(x, &vars));
                    if v.0.is_finite() && v.1.is_finite() {
                        vecs.push(((x, y), v));
                    }
                }
            }
            let max = vecs.iter().map(|(_, v)| (v.0 * v.0 + v.1 * v.1).sqrt()).fold(0.0, f64::max).max(1e-12);
            let cell = ((xr.1 - xr.0) / nx.max(1) as f64).min((yr.1 - yr.0) / ny.max(1) as f64);
            let normalize = matches!(text(f, "normalize").as_deref(), Some("true"));
            let scale = num(f, "length", 0.85, refs)? * cell;
            let pal = match f.properties.get("colormap") {
                Some(v) => Some(palette(v)?),
                None => None,
            };
            let mut out = Vec::new();
            for ((x, y), v) in vecs {
                let mag = (v.0 * v.0 + v.1 * v.1).sqrt();
                if mag <= 0.0 {
                    continue;
                }
                let len = if normalize { scale } else { scale * mag / max };
                let (ux, uy) = (v.0 / mag, v.1 / mag);
                let (x0, y0) = (x - ux * len / 2.0, y - uy * len / 2.0);
                let c = pal.as_ref().map(|s| sample(s, mag / max)).map(|c| Rgba { a: c.a * lk.opacity, ..c }).unwrap_or(line);
                out.extend(arrow2(m.to_px(x0, y0), m.to_px(x0 + ux * len, y0 + uy * len), c, width));
            }
            out
        }
        "iterate" => {
            let land = landscape(f, refs)?;
            let axes = match (text(f, "axes"), text(f, "on")) {
                (Some(a), _) => a,
                (None, Some(on)) => refs.plots.get(&on).map(|p| p.axes.clone()).ok_or("iterate : `axes` manquant")?,
                _ => return Err("iterate : `axes : repere`, `on : courbe` ou `scene : ...` manquant".to_string()),
            };
            let m = axes_of(&axes)?;
            let states = iterate_states(f, refs, land.as_ref())?;
            let pts: Vec<(f32, f32)> = states
                .iter()
                .map(|s| match &land {
                    Some(l) if l.dims == 1 => m.to_px(s[0], (l.f)(&s[..1])),
                    _ => m.to_px(s[0], *s.get(1).unwrap_or(&0.0)),
                })
                .collect();
            let mut out = Vec::new();
            if let (Some(c), true) = (lk.line, pts.len() >= 2) {
                let mut path = vec![PathCmd::MoveTo(pts[0].0, pts[0].1)];
                path.extend(pts[1..].iter().map(|p| PathCmd::LineTo(p.0, p.1)));
                out.push(Shape { glow: 0.0, path, fill: None, stroke: Some((c, lk.tube.unwrap_or(width))), image: None });
            }
            let r = px(f, "markers", 4.0, scene)?;
            if r > 0.0 {
                let c = lk.fill.or(lk.line).unwrap_or(Rgba::BLACK);
                out.extend(pts.iter().map(|p| Shape { glow: 0.0, path: geometry::circle_path(p.0, p.1, r), fill: Some(c), stroke: None, image: None }));
            }
            out
        }
        "trajectory" => {
            let m = axes_of(&text(f, "axes").ok_or("trajectory : `axes : repere` ou `scene : espace` manquant")?)?;
            let (cs, _) = super::ode::curves(f, &refs.params, 2)?;
            let colors = trajectory_colors(f, cs.len(), line)?;
            let r = px(f, "head", 0.0, scene)?;
            let mut out = Vec::new();
            for (c, col) in cs.iter().zip(&colors) {
                let pts: Vec<(f32, f32)> = super::ode::thin(c, 4000).iter().map(|p| m.to_px(p[0], p[1])).collect();
                if pts.len() >= 2 && lk.line.is_some() {
                    let mut path = vec![PathCmd::MoveTo(pts[0].0, pts[0].1)];
                    path.extend(pts[1..].iter().map(|p| PathCmd::LineTo(p.0, p.1)));
                    out.push(Shape { glow: 0.0, path, fill: None, stroke: Some((*col, width)), image: None });
                }
                if let (true, Some(p)) = (r > 0.0, pts.last()) {
                    let fc = lk.fill.unwrap_or(*col);
                    out.push(Shape { glow: 0.0, path: geometry::circle_path(p.0, p.1, r), fill: Some(fc), stroke: None, image: None });
                }
            }
            out
        }
        other => return Err(format!("type '{}' pas en 2D", other)),
    };
    for s in &mut shapes {
        s.glow = lk.glow;
    }
    Ok(shapes)
}

// ─────────────────────────────────────────────
//  ANIMATIONS
// ─────────────────────────────────────────────

/// Effet des animations d'un objet 3D à l'instant t
pub struct Effect3 {
    c0: V3,
    center: V3,
    rot: M3,
    scale: f64,
    pub opacity: f32,
    pub reveal: Option<f32>,
}

/// Calcule l'effet des animations commencées d'une figure (ou d'un groupe) 3D, autour de `c0`
pub fn effect(st: &FigureState, t: f64, c0: V3, span: f64, refs: &Refs) -> Result<Effect3, String> {
    let mut fx = Effect3 { c0, center: c0, rot: IDENTITY, scale: 1.0, opacity: 1.0, reveal: None };
    let (mut size, mut appear) = (1.0f64, 1.0f64);
    let mut slide = [0.0; 3];
    let dir3 = |d: crate::runtime::animation::Dir| -> V3 {
        use crate::runtime::animation::Dir;
        match d {
            Dir::Left => [-1.0, 0.0, 0.0],
            Dir::Right => [1.0, 0.0, 0.0],
            Dir::Up => [0.0, 0.0, 1.0],
            Dir::Down => [0.0, 0.0, -1.0],
        }
    };
    for a in &st.anims {
        let e = a.progress(t);
        if a.kind.is_entry_or_exit() && a.start < st.last_start {
            continue;
        }
        match &a.kind {
            AnimKind::Move { to } => {
                let (target, _) = point3(to, refs)?;
                fx.center = lerp(fx.center, target, e);
            }
            AnimKind::MoveBy { dir, distance } => fx.center = add(fx.center, mul(dir3(*dir), distance / 100.0 * span * e)),
            AnimKind::Scale(f) => size += (f - size) * e,
            AnimKind::Rotate(deg) => fx.rot = mat_mul(&rotation([0.0, 0.0, 1.0], deg * e), &fx.rot),
            AnimKind::Rotate3d { axis, angle } => fx.rot = mat_mul(&rotation(*axis, angle * e), &fx.rot),
            AnimKind::RotateAround { .. } => return Err("rotate(around=...) n'existe qu'en 2D ; en 3D : rotate3d(angle, axis=..., durée)".to_string()),
            AnimKind::Follow { path, .. } => {
                let pts = refs.paths3.get(path).ok_or_else(|| format!("follow : '{}' n'est pas un chemin 3D (curve3d, iterate, arrow)", path))?;
                if let Some(p) = along(pts, e) {
                    fx.center = p;
                }
            }
            AnimKind::FadeIn => fx.opacity = e as f32,
            AnimKind::FadeOut => fx.opacity = 1.0 - e as f32,
            AnimKind::Grow => appear = e,
            AnimKind::Shrink => appear = 1.0 - e,
            AnimKind::SlideIn { from } => {
                slide = mul(dir3(*from), span * 0.12 * (1.0 - e));
                fx.opacity = e as f32;
            }
            AnimKind::SlideOut { to } => {
                slide = mul(dir3(*to), span * 0.12 * e);
                fx.opacity = 1.0 - e as f32;
            }
            AnimKind::Draw | AnimKind::Write => fx.reveal = Some(e as f32),
            AnimKind::Color { .. } | AnimKind::Fill { .. } | AnimKind::Transform { .. } | AnimKind::To(_) | AnimKind::Swap { .. } => {}
        }
    }
    fx.scale = size * appear;
    fx.center = add(fx.center, slide);
    Ok(fx)
}

/// Point à la fraction `e` de la longueur d'une ligne brisée
fn along(pts: &[V3], e: f64) -> Option<V3> {
    if pts.len() < 2 {
        return pts.first().copied();
    }
    let lens: Vec<f64> = pts.windows(2).map(|w| norm(sub(w[1], w[0]))).collect();
    let mut target = lens.iter().sum::<f64>() * e.clamp(0.0, 1.0);
    for (w, l) in pts.windows(2).zip(&lens) {
        if target <= *l && *l > 0.0 {
            return Some(lerp(w[0], w[1], target / l));
        }
        target -= l;
    }
    pts.last().copied()
}

impl Effect3 {
    /// Seulement le fondu et le dessin progressif (pas de déplacement)
    pub fn look_only(fx: &Effect3) -> Effect3 {
        Effect3 { c0: fx.c0, center: fx.c0, rot: IDENTITY, scale: 1.0, opacity: fx.opacity, reveal: fx.reveal }
    }

    pub fn map(&self, p: V3) -> V3 {
        add(self.center, mat_vec(&self.rot, mul(sub(p, self.c0), self.scale)))
    }

    /// Applique l'effet aux morceaux (géométrie, opacité, dessin progressif)
    pub fn apply(&self, items: &mut Vec<Item>) {
        let identity = self.rot == IDENTITY && self.scale == 1.0 && self.center == self.c0;
        if !identity {
            for it in items.iter_mut() {
                it.prim.map(&|p| self.map(p));
                if self.scale != 1.0 {
                    let k = self.scale.max(0.0) as f32;
                    match &mut it.prim {
                        Prim::Dot { radius, .. } => *radius *= k,
                        Prim::Seg { width, .. } | Prim::Line { width, .. } => *width *= k,
                        Prim::Label { size, .. } => *size *= k,
                        Prim::Face { .. } => {}
                    }
                }
            }
        }
        if self.opacity < 1.0 {
            items.iter_mut().for_each(|it| it.prim.fade(self.opacity));
        }
        if let Some(e) = self.reveal {
            reveal(items, e);
        }
    }
}

/// Dessin progressif : chaque morceau apparaît entre ses instants (début, fin) ;
/// un segment en cours se trace, une face en cours apparaît en fondu
fn reveal(items: &mut Vec<Item>, e: f32) {
    items.retain(|it| e > it.order.0 || (e >= 1.0 && it.order.0 >= 1.0));
    for it in items.iter_mut() {
        let (s, end) = it.order;
        if e >= end {
            continue;
        }
        let k = ((e - s) / (end - s).max(1e-6)).clamp(0.0, 1.0);
        match &mut it.prim {
            Prim::Seg { a, b, .. } => *b = lerp(*a, *b, k as f64),
            Prim::Line { pts, .. } => {
                // On garde le début de la ligne, jusqu'à la fraction k de sa longueur
                let total: f64 = pts.windows(2).map(|w| norm(sub(w[1], w[0]))).sum();
                let mut left = total * k as f64;
                let mut kept = vec![pts[0]];
                for w in pts.windows(2) {
                    let l = norm(sub(w[1], w[0]));
                    if left >= l {
                        kept.push(w[1]);
                        left -= l;
                    } else {
                        if l > 0.0 {
                            kept.push(lerp(w[0], w[1], left / l));
                        }
                        break;
                    }
                }
                *pts = kept;
            }
            Prim::Face { color, .. } | Prim::Label { color, .. } => color.a *= k,
            Prim::Dot { .. } => {}
        }
    }
}

/// transform(to=b) entre objets 3D : même découpage → les points glissent ; sinon fondu enchaîné
pub fn morph(a: &[Item], b: &[Item], e: f32) -> Vec<Item> {
    let same = a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| std::mem::discriminant(&x.prim) == std::mem::discriminant(&y.prim) && x.prim.points().len() == y.prim.points().len());
    if !same {
        let mut out: Vec<Item> = a.to_vec();
        out.iter_mut().for_each(|it| it.prim.fade(1.0 - e));
        out.extend(b.iter().cloned().map(|mut it| {
            it.prim.fade(e);
            it
        }));
        return out;
    }
    a.iter()
        .zip(b)
        .map(|(x, y)| {
            let mut it = x.clone();
            let (pa, pb) = (x.prim.points(), y.prim.points());
            let pts: Vec<V3> = pa.iter().zip(&pb).map(|(p, q)| lerp(*p, *q, e as f64)).collect();
            match (&mut it.prim, &y.prim) {
                (Prim::Face { pts: p, color, .. }, Prim::Face { color: c2, .. }) => {
                    *p = pts.clone();
                    *color = mix(*color, *c2, e);
                }
                (Prim::Line { pts: p, color, width, .. }, Prim::Line { color: c2, width: w2, .. }) => {
                    *p = pts.clone();
                    *color = mix(*color, *c2, e);
                    *width += (w2 - *width) * e;
                }
                (Prim::Seg { a, b, color, width, .. }, Prim::Seg { color: c2, width: w2, .. }) => {
                    *a = pts[0];
                    *b = pts[1];
                    *color = mix(*color, *c2, e);
                    *width += (w2 - *width) * e;
                }
                (Prim::Dot { p, color, radius }, Prim::Dot { color: c2, radius: r2, .. }) => {
                    *p = pts[0];
                    *color = mix(*color, *c2, e);
                    *radius += (r2 - *radius) * e;
                }
                (Prim::Label { p, color, .. }, Prim::Label { color: c2, .. }) => {
                    *p = pts[0];
                    *color = mix(*color, *c2, e);
                }
                _ => {}
            }
            it
        })
        .collect()
}

// ─────────────────────────────────────────────
//  PROJECTION ET TRI
// ─────────────────────────────────────────────

/// Projette les morceaux d'une scène, les trie du plus loin au plus proche et donne les formes 2D
pub fn compose(sp: &Space, items: &[&Item]) -> Vec<Shape> {
    // Lignes, points et étiquettes passent devant les faces voisines (une trajectoire posée
    // sur une surface reste visible)
    let bias = sp.dist * 0.012;
    let mut drawn: Vec<(f64, usize, Vec<Shape>)> = Vec::new();
    for (idx, it) in items.iter().enumerate() {
        let pts = it.prim.points();
        let Some(proj) = pts.iter().map(|p| sp.project(*p)).collect::<Option<Vec<_>>>() else { continue };
        let depth = proj.iter().map(|p| p.2).sum::<f64>() / proj.len() as f64;
        let mut shapes = Vec::new();
        let depth = match &it.prim {
            Prim::Face { color, shade, .. } => {
                let mut c = *color;
                if *shade {
                    // Normale de Newell (robuste pour les polygones presque dégénérés)
                    let w: Vec<V3> = pts.iter().map(|p| sp.to_world(*p)).collect();
                    let mut n = [0.0; 3];
                    for i in 0..w.len() {
                        let (a, b) = (w[i], w[(i + 1) % w.len()]);
                        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
                        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
                        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
                    }
                    let light = sp.ambient + (1.0 - sp.ambient) * dot(unit(n), sp.light).abs();
                    let l = light as f32;
                    c = Rgba { r: c.r * l, g: c.g * l, b: c.b * l, a: c.a };
                }
                let mut path = vec![PathCmd::MoveTo(proj[0].0, proj[0].1)];
                path.extend(proj[1..].iter().map(|p| PathCmd::LineTo(p.0, p.1)));
                path.push(PathCmd::Close);
                // Faces opaques : un fin trait de la même couleur cache les jointures
                let seam = if c.a >= 0.99 { Some((c, 0.8)) } else { None };
                shapes.push(Shape { glow: it.glow, path, fill: Some(c), stroke: seam, image: None });
                depth
            }
            Prim::Seg { color, width, tube, .. } | Prim::Line { color, width, tube, .. } => {
                let mut path = vec![PathCmd::MoveTo(proj[0].0, proj[0].1)];
                path.extend(proj[1..].iter().map(|p| PathCmd::LineTo(p.0, p.1)));
                if *tube {
                    // Tube : bord sombre, corps, reflet ; seul le corps porte le halo
                    let dark = Rgba { r: color.r * 0.55, g: color.g * 0.55, b: color.b * 0.55, a: color.a };
                    shapes.push(Shape { glow: 0.0, path: path.clone(), fill: None, stroke: Some((dark, *width)), image: None });
                    shapes.push(Shape { glow: it.glow, path: path.clone(), fill: None, stroke: Some((*color, *width * 0.7)), image: None });
                    shapes.push(Shape { glow: 0.0, path, fill: None, stroke: Some((mix(*color, Rgba::WHITE, 0.45), *width * 0.25)), image: None });
                } else {
                    shapes.push(Shape { glow: it.glow, path, fill: None, stroke: Some((*color, *width)), image: None });
                }
                depth - bias
            }
            Prim::Dot { color, radius, .. } => {
                if *radius > 0.0 {
                    shapes.push(Shape { glow: it.glow, path: geometry::circle_path(proj[0].0, proj[0].1, *radius), fill: Some(*color), stroke: None, image: None });
                }
                depth - bias * 1.5
            }
            Prim::Label { tex, size, color, .. } => {
                let content = typeset::Content::Math(tex.clone());
                if let Ok(mut s) = typeset::shapes(&content, proj[0].0, proj[0].1, *size, typeset::Align::Center, *color) {
                    s.iter_mut().for_each(|s| s.glow = it.glow);
                    shapes.extend(s);
                }
                depth - bias * 2.0
            }
        };
        drawn.push((depth, idx, shapes));
    }
    drawn.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    drawn.into_iter().flat_map(|d| d.2).collect()
}

/// Centre et boîte en pixels d'un objet (pour `center(objet)` et `offset(objet, d)` en 2D)
pub fn screen_geom(sp: &Space, obj: &Obj, center: V3) -> geometry::Geom {
    let c = sp.project(center).map(|p| (p.0, p.1)).unwrap_or((0.0, 0.0));
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for it in &obj.items {
        for p in it.prim.points() {
            if let Some((x, y, _)) = sp.project(p) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    let bbox = if x0 > x1 { (c.0, c.1, c.0, c.1) } else { (x0, y0, x1, y1) };
    let outline = obj.path.iter().filter_map(|p| sp.project(*p)).map(|p| (p.0, p.1)).collect();
    geometry::Geom { center: c, angle: 0.0, bbox, label: None, axes: None, outline, glyphs: None, named: Vec::new() }
}

/// Plus grande étendue des axes d'une scène (pour move(up, 20%, ...))
pub fn span_of(sp: &Space) -> f64 {
    sp.span()
}

// ─────────────────────────────────────────────
//  VÉRIFICATION (validateur)
// ─────────────────────────────────────────────

/// Vérifie les formules et les propriétés propres à la 3D d'une figure.
/// Renvoie (propriété, message) pour chaque erreur.
pub fn check(ftype: &str, props: &HashMap<String, PropValue>, params: &[String]) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    let refs = Refs::with_params(params.iter().map(|p| (p.clone(), 1.0)).collect());
    let get = |k: &str| props.get(k).map(fmt_value);
    let expr_check = |key: &str, src: &str, locals: &[&str], errors: &mut Vec<(String, String)>| {
        if let Err(e) = compile(src, locals, &refs) {
            errors.push((key.to_string(), e));
        }
    };
    let tuple = |src: &str| split_top(src.trim().pipe_outer());
    match ftype {
        "surface" => match get("expr") {
            None => errors.push(("type".into(), "surface : `expr` manquant (z = f(x, y), ou (x, y, z) en fonction de u, v)".into())),
            Some(src) if src.trim_start().starts_with('(') => {
                let c = tuple(&src);
                if c.len() != 3 {
                    errors.push(("expr".into(), "surface paramétrée : (x(u, v), y(u, v), z(u, v)) attendu".into()));
                }
                for s in &c {
                    expr_check("expr", s, &["u", "v"], &mut errors);
                }
                for k in ["urange", "vrange"] {
                    if !props.contains_key(k) {
                        errors.push(("expr".into(), format!("surface paramétrée : `{} : [a, b]` manquant", k)));
                    }
                }
            }
            Some(src) => expr_check("expr", &src, &["y"], &mut errors),
        },
        "curve3d" => match get("expr") {
            Some(src) => {
                let c = tuple(&src);
                if c.len() != 3 {
                    errors.push(("expr".into(), "curve3d : (x(t), y(t), z(t)) attendu".into()));
                }
                for s in &c {
                    expr_check("expr", s, &["t"], &mut errors);
                }
            }
            None if !props.contains_key("points") => {
                errors.push(("type".into(), "curve3d : `expr : (x(t), y(t), z(t))` ou `points` manquant".into()))
            }
            None => {}
        },
        "field" => match get("expr") {
            Some(src) => {
                let c = tuple(&src);
                if c.len() != 2 && c.len() != 3 {
                    errors.push(("expr".into(), "field : (fx, fy) ou (fx, fy, fz) attendu".into()));
                }
                for s in &c {
                    expr_check("expr", s, &["y", "z"], &mut errors);
                }
            }
            None => errors.push(("type".into(), "field : `expr : (fx, fy)` manquant".into())),
        },
        "iterate" => {
            if let Some(update) = props.get("update") {
                let names = props.get("vars").map(parts).unwrap_or_default();
                let init = props.get("init").map(parts).unwrap_or_default();
                let upd = parts(update);
                if names.is_empty() || init.len() != names.len() || upd.len() != names.len() {
                    errors.push(("update".into(), "iterate : vars, init et update doivent avoir autant d'éléments".into()));
                }
                let mut locals: Vec<&str> = names.iter().map(|s| s.as_str()).filter(|s| *s != "x").collect();
                locals.extend(ITERATE_LOCALS);
                for s in &upd {
                    expr_check("update", s, &locals, &mut errors);
                }
                for s in &init {
                    expr_check("init", s, &[], &mut errors);
                }
            } else {
                let method = get("method").unwrap_or_else(|| "gd".into());
                if !METHODS.contains(&method.as_str()) {
                    errors.push(("method".into(), format!("iterate : méthode '{}' inconnue ({}), ou vars / init / update", method, METHODS.join(", "))));
                }
                if !props.contains_key("on") {
                    errors.push(("type".into(), "iterate : `on : surface` (ou un plot) pour une méthode d'optimisation, sinon vars / init / update".into()));
                }
                if !props.contains_key("start") {
                    errors.push(("type".into(), "iterate : `start : (x, y)` manquant".into()));
                }
            }
        }
        "polyhedron" => {
            for k in ["points", "faces"] {
                if !props.contains_key(k) {
                    errors.push(("type".into(), format!("polyhedron : `{}` manquant", k)));
                }
            }
        }
        "arrow" => {
            for k in ["from", "to"] {
                if !props.contains_key(k) {
                    errors.push(("type".into(), format!("arrow : `{}` manquant", k)));
                }
            }
        }
        "scene3d" => {
            if let Some(p) = get("projection").filter(|p| p != "perspective" && p != "ortho") {
                errors.push(("projection".into(), format!("projection '{}' invalide (perspective ou ortho)", p)));
            }
            if let Some(a) = get("frame").filter(|a| !["lines", "true", "box", "none", "false"].contains(&a.as_str())) {
                errors.push(("frame".into(), format!("frame '{}' invalide (lines, box ou none)", a)));
            }
        }
        _ => {}
    }
    if let Some(v) = props.get("colormap") {
        if let Err(e) = palette(v) {
            errors.push(("colormap".into(), e));
        }
    }
    if let Some(src) = get("color_by") {
        expr_check("color_by", &src, &["y", "z", "u", "v"], &mut errors);
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(elev: f64, azim: f64, persp: bool) -> Space {
        let scene = Scene { width: 1000.0, height: 1000.0, px_scale: 1.0, background: Rgba::WHITE, orbit: (0.0, 0.0, 1.0) };
        let mut props = HashMap::new();
        props.insert("camera".to_string(), PropValue::Tuple(vec![elev.to_string(), azim.to_string()]));
        props.insert("projection".to_string(), PropValue::Single(if persp { "perspective" } else { "ortho" }.into()));
        let f = ResolvedFigure { name: "s".into(), index: 0, ftype: "scene3d".into(), layer: 0.0, line: 1, properties: props, prop_lines: HashMap::new() };
        build_space(&f, &scene, &Refs::new()).unwrap().0
    }

    #[test]
    fn camera_vue_de_dessus_et_de_face() {
        // Vue de face (élévation 0, rotation -90 : on regarde vers +y) : x vers la droite, z vers le haut
        let s = space(0.0, -90.0, false);
        let o = s.project([0.0, 0.0, 0.0]).unwrap();
        let x = s.project([1.0, 0.0, 0.0]).unwrap();
        let z = s.project([0.0, 0.0, 1.0]).unwrap();
        assert!((o.0 - 500.0).abs() < 1e-3 && (o.1 - 500.0).abs() < 1e-3);
        assert!(x.0 > o.0 + 10.0 && (x.1 - o.1).abs() < 1e-3);
        assert!(z.1 < o.1 - 10.0 && (z.0 - o.0).abs() < 1e-3);
        // Le point le plus proche de la caméra (y négatif) est moins profond
        assert!(s.project([0.0, -1.0, 0.0]).unwrap().2 < o.2);
        // Vue de dessus : z ne bouge plus à l'écran
        let s = space(89.5, -90.0, false);
        let (a, b) = (s.project([0.0, 0.0, 0.0]).unwrap(), s.project([0.0, 0.0, 2.0]).unwrap());
        assert!((a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 5.0);
    }

    #[test]
    fn perspective_les_objets_lointains_sont_plus_petits() {
        let s = space(0.0, -90.0, true);
        let w = |y: f64| s.project([1.0, y, 0.0]).unwrap().0 - s.project([0.0, y, 0.0]).unwrap().0;
        assert!(w(-1.0) > w(0.0) && w(0.0) > w(1.0));
    }

    #[test]
    fn tri_du_plus_loin_au_plus_proche() {
        let s = space(0.0, -90.0, false);
        let face = |y: f64, c: Rgba| item(Prim::Face { pts: vec![[-1.0, y, -1.0], [1.0, y, -1.0], [1.0, y, 1.0], [-1.0, y, 1.0]], color: c, shade: false }, (0.0, 1.0), 0.0);
        let red = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let (near, far) = (face(-1.0, red), face(1.0, Rgba::BLACK));
        let shapes = compose(&s, &[&near, &far]);
        // la face lointaine (noire) est dessinée d'abord
        assert_eq!(shapes[0].fill, Some(Rgba::BLACK));
        assert_eq!(shapes[1].fill, Some(red));
    }

    #[test]
    fn rotation_3d() {
        let r = rotation([0.0, 0.0, 1.0], 90.0);
        let p = mat_vec(&r, [1.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-12 && (p[1] - 1.0).abs() < 1e-12);
        let r = rotation([1.0, 1.0, 1.0], 120.0); // permute les axes
        let p = mat_vec(&r, [1.0, 0.0, 0.0]);
        assert!((p[1] - 1.0).abs() < 1e-9);
    }

    fn fig(ftype: &str, props: &[(&str, PropValue)]) -> ResolvedFigure {
        let properties = props.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        ResolvedFigure { name: "it".into(), index: 0, ftype: ftype.into(), layer: 0.0, line: 1, properties, prop_lines: HashMap::new() }
    }

    fn bowl() -> Refs {
        let mut refs = Refs::new();
        refs.surfaces.insert("bol".into(), SurfaceRef::Explicit(expr::parse_with("x^2 + 3*y^2", &["y".to_string()]).unwrap()));
        refs
    }

    #[test]
    fn optimiseurs_convergent_vers_le_minimum() {
        let refs = bowl();
        for method in METHODS {
            let f = fig(
                "iterate",
                &[
                    ("on", PropValue::Single("bol".into())),
                    ("method", PropValue::Single(method.to_string())),
                    ("start", PropValue::Tuple(vec!["2".into(), "1".into()])),
                    ("rate", PropValue::Number(if *method == "newton" { 1.0 } else if *method == "adam" || *method == "rmsprop" { 0.05 } else { 0.05 })),
                    ("steps", PropValue::Number(400.0)),
                ],
            );
            let land = landscape(&f, &refs).unwrap();
            let states = iterate_states(&f, &refs, land.as_ref()).unwrap();
            let last = states.last().unwrap();
            assert!(last[0].abs() < 0.05 && last[1].abs() < 0.05, "{} : {:?}", method, last);
            assert_eq!(states[0], vec![2.0, 1.0]);
        }
    }

    #[test]
    fn newton_en_un_pas_sur_une_quadratique() {
        let refs = bowl();
        let f = fig(
            "iterate",
            &[
                ("on", PropValue::Single("bol".into())),
                ("method", PropValue::Single("newton".into())),
                ("start", PropValue::Tuple(vec!["2".into(), "1".into()])),
                ("steps", PropValue::Number(1.0)),
            ],
        );
        let land = landscape(&f, &refs).unwrap();
        let s = iterate_states(&f, &refs, land.as_ref()).unwrap();
        assert!(s[1][0].abs() < 1e-3 && s[1][1].abs() < 1e-3, "{:?}", s);
    }

    #[test]
    fn recurrence_generale() {
        // Descente de gradient écrite à la main = méthode gd
        let refs = bowl();
        let general = fig(
            "iterate",
            &[
                ("on", PropValue::Single("bol".into())),
                ("vars", PropValue::Tuple(vec!["x".into(), "y".into()])),
                ("init", PropValue::Tuple(vec!["2".into(), "1".into()])),
                ("update", PropValue::Tuple(vec!["x - 0.1*dx".into(), "y - 0.1*dy".into()])),
                ("steps", PropValue::Number(5.0)),
            ],
        );
        let builtin = fig(
            "iterate",
            &[
                ("on", PropValue::Single("bol".into())),
                ("start", PropValue::Tuple(vec!["2".into(), "1".into()])),
                ("rate", PropValue::Number(0.1)),
                ("steps", PropValue::Number(5.0)),
            ],
        );
        let land = landscape(&general, &refs).unwrap();
        let a = iterate_states(&general, &refs, land.as_ref()).unwrap();
        let b = iterate_states(&builtin, &refs, land.as_ref()).unwrap();
        for (p, q) in a.iter().zip(&b) {
            assert!((p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6, "{:?} {:?}", p, q);
        }
        // Sans fonction : une suite libre (ici une rotation)
        let free = fig(
            "iterate",
            &[
                ("vars", PropValue::Tuple(vec!["x".into(), "y".into(), "z".into()])),
                ("init", PropValue::Tuple(vec!["1".into(), "0".into(), "0".into()])),
                ("update", PropValue::Tuple(vec!["x*cos(1) - y*sin(1)".into(), "x*sin(1) + y*cos(1)".into(), "z + 0.1*n".into()])),
                ("steps", PropValue::Number(3.0)),
            ],
        );
        let s = iterate_states(&free, &Refs::new(), None).unwrap();
        assert_eq!(s.len(), 4);
        assert!(((s[3][0] * s[3][0] + s[3][1] * s[3][1]) - 1.0).abs() < 1e-9);
        assert!((s[3][2] - 0.3).abs() < 1e-12);
    }

    #[test]
    fn segments_regroupes() {
        let pts: Vec<V3> = (0..=13).map(|i| [i as f64, 0.0, 0.0]).collect();
        let items = polyline(&pts, Rgba::BLACK, 1.0, false, 0.0);
        // 13 segments → lignes de 6, 6 et 1, bout à bout
        assert_eq!(items.len(), 3);
        let Prim::Line { pts: first, .. } = &items[0].prim else { panic!() };
        assert_eq!(first.len(), 7);
        assert_eq!(items[0].order.0, 0.0);
        assert!((items[2].order.1 - 1.0).abs() < 1e-6);
        // Le dessin progressif coupe une ligne au bon endroit
        let mut half = items.clone();
        reveal(&mut half, 0.25);
        let Prim::Line { pts: cut, .. } = &half[0].prim else { panic!() };
        assert!((cut.last().unwrap()[0] - 3.25).abs() < 1e-3, "{:?}", cut.last());
    }

    #[test]
    fn dessin_progressif() {
        let mut items = polyline(&[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]], Rgba::BLACK, 1.0, false, 0.0);
        reveal(&mut items, 0.5);
        let Prim::Line { pts, .. } = &items[0].prim else { panic!() };
        assert!((pts.last().unwrap()[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn parentheses_exterieures() {
        assert_eq!(split_top("(cos(u), 0.5*sin(v))".pipe_outer()), ["cos(u)", "0.5*sin(v)"]);
        assert_eq!("sin(v)".pipe_outer(), "sin(v)");
    }

    #[test]
    fn verification_des_formules() {
        let mut p = HashMap::new();
        p.insert("expr".to_string(), PropValue::Single("sin(k*x) + y^2".into()));
        assert!(check("surface", &p, &["k".to_string()]).is_empty());
        assert!(!check("surface", &p, &[]).is_empty()); // k inconnu
        p.insert("expr".to_string(), PropValue::Single("(cos(u), sin(u), v)".into()));
        assert!(check("surface", &p, &[]).iter().any(|e| e.1.contains("urange")));
        let mut it = HashMap::new();
        it.insert("method".to_string(), PropValue::Single("sgd".into()));
        assert!(check("iterate", &it, &[]).iter().any(|e| e.1.contains("méthode 'sgd'")));
    }
}
