//! Conversion d'une figure Didact en formes à dessiner, en pixels.
//!
//! Conventions (SPEC §7) :
//! - positions en % de la scène, origine en bas à gauche, y vers le haut ;
//! - en pixels, l'origine est en haut à gauche et y descend : on convertit ici.
//! - angles en degrés, positif = sens trigonométrique.
//!
//! Unités (SPEC §7.5) :
//! - positions : x en % de la largeur, y en % de la hauteur (placement) ;
//! - tailles (`radius`, `size`, `width`/`height` d'un rect) : en % du plus petit côté
//!   de la scène, pour que les formes ne se déforment jamais ;
//! - `px` : pixels d'une scène de référence (taille de `window`, sinon 1080 px de haut),
//!   mis à l'échelle avec la fenêtre pour que le rendu reste identique.

use crate::ast::{Config, PropValue, WindowConfig};
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;

// ─────────────────────────────────────────────
//  TYPES DE BASE
// ─────────────────────────────────────────────

/// Couleur, composantes entre 0 et 1
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    fn rgb8(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    fn with_opacity(self, opacity: f32) -> Rgba {
        Rgba { a: self.a * opacity, ..self }
    }
}

/// Une commande de tracé, en pixels
#[derive(Debug, Clone, PartialEq)]
pub enum PathCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// Courbe de Bézier cubique : point de contrôle 1, point de contrôle 2, arrivée
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Une forme : un chemin, éventuellement rempli et/ou tracé
#[derive(Debug, Clone)]
pub struct Shape {
    pub path: Vec<PathCmd>,
    pub fill: Option<Rgba>,
    /// Couleur et épaisseur du trait (en pixels)
    pub stroke: Option<(Rgba, f32)>,
}

/// Géométrie calculée d'une figure à l'instant t, utilisée par les références
/// dynamiques des figures définies plus bas (`center(fig)`, `angle(fig)`, `offset(fig, d)`).
#[derive(Debug, Clone, PartialEq)]
pub struct Geom {
    /// Centre (pixels)
    pub center: (f32, f32),
    /// Angle en degrés, sens trigonométrique : direction pour une line, rotation sinon
    pub angle: f32,
    /// Boîte englobante : (x min, y min, x max, y max) en pixels
    pub bbox: (f32, f32, f32, f32),
    /// Point d'accroche d'une étiquette et direction vers l'extérieur
    /// (milieu d'un arc) ; None → coin haut-droit de la boîte
    pub label: Option<((f32, f32), (f32, f32))>,
}

/// Géométries des figures déjà calculées, par nom
pub type Refs = HashMap<String, Geom>;

/// La zone de dessin
#[derive(Debug, Clone)]
pub struct Scene {
    /// Taille en pixels
    pub width: f32,
    pub height: f32,
    /// Facteur appliqué aux valeurs en `px` (taille réelle / taille de référence)
    pub px_scale: f32,
    pub background: Rgba,
}

impl Scene {
    pub fn new(config: &Config, width: u32, height: u32) -> Scene {
        let reference_height = match config.window {
            WindowConfig::Size(_, h) if h > 0 => h as f32,
            _ => 1080.0,
        };
        Scene {
            width: width as f32,
            height: height as f32,
            px_scale: height as f32 / reference_height,
            background: parse_color(&config.background).ok().flatten().unwrap_or(Rgba::WHITE),
        }
    }

    /// Taille par défaut pour un rendu hors fenêtre (PNG, et plus tard MP4)
    pub fn default_size(config: &Config) -> (u32, u32) {
        match config.window {
            WindowConfig::Size(w, h) if w > 0 && h > 0 => (w, h),
            WindowConfig::Ratio(a, b) if a > 0 && b > 0 => (1920, (1920 * b / a).max(1)),
            _ => (1920, 1080),
        }
    }

    fn min_side(&self) -> f32 {
        self.width.min(self.height)
    }
}

// ─────────────────────────────────────────────
//  CONVERSION FIGURE → FORMES
// ─────────────────────────────────────────────

/// Valeurs par défaut quand une propriété n'est pas écrite (à valider, cf. SPEC §15)
const DEFAULT_STROKE_PX: f32 = 2.0;

/// Style commun à toutes les figures
struct Look {
    color: Option<Rgba>,
    fill: Option<Rgba>,
    stroke: f32,
}

pub fn figure_shapes(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<Vec<Shape>, String> {
    let opacity = match f.properties.get("opacity") {
        Some(PropValue::Number(n)) => (*n as f32).clamp(0.0, 1.0),
        Some(v) => return Err(format!("opacity invalide '{}'", fmt_value(v))),
        None => 1.0,
    };
    let look = Look {
        color: match f.properties.get("color") {
            Some(v) => parse_color(&fmt_value(v))?.map(|c| c.with_opacity(opacity)),
            None => Some(Rgba::BLACK.with_opacity(opacity)),
        },
        fill: match f.properties.get("fill") {
            Some(v) => parse_color(&fmt_value(v))?.map(|c| c.with_opacity(opacity)),
            None => None,
        },
        stroke: match f.properties.get("stroke") {
            Some(v) => length(v, scene, scene.min_side())?,
            None => DEFAULT_STROKE_PX * scene.px_scale,
        },
    };

    match f.ftype.as_str() {
        "line" => line(f, scene, refs, &look),
        "circle" => circle(f, scene, refs, &look),
        "rect" => rect(f, scene, refs, &look),
        "polygon" => polygon(f, scene, &look),
        "arc" => arc(f, scene, refs, &look),
        "star" => star(f, scene, refs, &look),
        other => Err(format!("type '{}' pas encore dessiné", other)),
    }
}

fn line(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (x1, y1) = point(require(f, "from")?, scene, refs)?;
    let (x2, y2) = point(require(f, "to")?, scene, refs)?;
    let (start_arrow, end_arrow) = arrows(f)?;
    let color = look.color.unwrap_or(Rgba::BLACK);

    let mut shapes = Vec::new();
    let (mut ax, mut ay, mut bx, mut by) = (x1, y1, x2, y2);
    let head = arrow_length(look.stroke, scene);

    // Le trait s'arrête à la base de la flèche pour ne pas dépasser de la pointe
    if end_arrow {
        let (dx, dy) = unit(x2 - x1, y2 - y1);
        shapes.push(arrow_head(x2, y2, dx, dy, head, color));
        bx -= dx * head * 0.8;
        by -= dy * head * 0.8;
    }
    if start_arrow {
        let (dx, dy) = unit(x1 - x2, y1 - y2);
        shapes.push(arrow_head(x1, y1, dx, dy, head, color));
        ax -= dx * head * 0.8;
        ay -= dy * head * 0.8;
    }

    shapes.insert(
        0,
        Shape {
            path: vec![PathCmd::MoveTo(ax, ay), PathCmd::LineTo(bx, by)],
            fill: None,
            stroke: look.color.map(|c| (c, look.stroke)),
        },
    );
    Ok(shapes)
}

fn circle(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (cx, cy) = point(require(f, "center")?, scene, refs)?;
    let r = length(require(f, "radius")?, scene, scene.min_side())?;
    Ok(vec![Shape {
        path: circle_path(cx, cy, r),
        fill: look.fill,
        stroke: look.color.map(|c| (c, look.stroke)),
    }])
}

fn rect(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    // En % du plus petit côté : width = height donne toujours un carré
    let w = length(require(f, "width")?, scene, scene.min_side())?;
    let h = length(require(f, "height")?, scene, scene.min_side())?;

    // Sans anchor, pos est le coin bas-gauche ; anchor : center → pos est le centre
    let (left, top) = match f.properties.get("anchor").map(fmt_value).as_deref() {
        None => (x, y - h),
        Some("center") => (x - w / 2.0, y - h / 2.0),
        Some(other) => return Err(format!("anchor '{}' pas encore supporté (seulement center)", other)),
    };

    Ok(vec![Shape {
        path: vec![
            PathCmd::MoveTo(left, top),
            PathCmd::LineTo(left + w, top),
            PathCmd::LineTo(left + w, top + h),
            PathCmd::LineTo(left, top + h),
            PathCmd::Close,
        ],
        fill: look.fill,
        stroke: look.color.map(|c| (c, look.stroke)),
    }])
}

fn polygon(f: &ResolvedFigure, scene: &Scene, look: &Look) -> Result<Vec<Shape>, String> {
    let items = match require(f, "points")? {
        PropValue::List(items) => items,
        v => return Err(format!("points doit être une liste de points, trouvé '{}'", fmt_value(v))),
    };
    if items.len() < 3 {
        return Err("un polygon doit avoir au moins 3 points".to_string());
    }
    let mut path = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let (x, y) = point_str(item, scene)?;
        path.push(if i == 0 { PathCmd::MoveTo(x, y) } else { PathCmd::LineTo(x, y) });
    }
    path.push(PathCmd::Close);
    Ok(vec![Shape { path, fill: look.fill, stroke: look.color.map(|c| (c, look.stroke)) }])
}

fn arc(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (cx, cy) = point(require(f, "center")?, scene, refs)?;
    let r = length(require(f, "radius")?, scene, scene.min_side())?;
    let a0 = angle(require(f, "from_angle")?, refs)?;
    let a1 = angle(require(f, "to_angle")?, refs)?;
    let (start_arrow, end_arrow) = arrows(f)?;
    let color = look.color.unwrap_or(Rgba::BLACK);

    // Point de l'arc à l'angle a (degrés, sens trigo) ; y pixel vers le bas → -sin
    let at = |a: f32| {
        let t = a.to_radians();
        (cx + r * t.cos(), cy - r * t.sin())
    };
    // Tangente dans le sens de parcours
    let sign = if a1 >= a0 { 1.0 } else { -1.0 };
    let tangent = |a: f32| {
        let t = a.to_radians();
        unit(-t.sin() * sign, -t.cos() * sign)
    };

    // Approximation par segments courts (1 segment tous les 2°)
    let steps = ((a1 - a0).abs() / 2.0).ceil().max(1.0) as usize;
    let mut path = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        let (x, y) = at(a);
        path.push(if i == 0 { PathCmd::MoveTo(x, y) } else { PathCmd::LineTo(x, y) });
    }

    let mut shapes = vec![Shape { path, fill: None, stroke: look.color.map(|c| (c, look.stroke)) }];
    let head = arrow_length(look.stroke, scene);
    if end_arrow {
        let (x, y) = at(a1);
        let (dx, dy) = tangent(a1);
        shapes.push(arrow_head(x, y, dx, dy, head, color));
    }
    if start_arrow {
        let (x, y) = at(a0);
        let (dx, dy) = tangent(a0);
        shapes.push(arrow_head(x, y, -dx, -dy, head, color));
    }
    Ok(shapes)
}

fn star(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (cx, cy) = point(require(f, "pos")?, scene, refs)?;
    let outer = length(require(f, "size")?, scene, scene.min_side())?;
    let n = match f.properties.get("points") {
        Some(PropValue::Number(n)) if *n >= 3.0 => *n as usize,
        None => 5,
        Some(v) => return Err(format!("points d'une star doit être un entier ≥ 3, trouvé '{}'", fmt_value(v))),
    };
    // Rapport rayon intérieur / extérieur d'une étoile régulière à 5 branches
    let inner = outer * 0.382;

    let mut path = Vec::with_capacity(2 * n + 1);
    for i in 0..2 * n {
        let r = if i % 2 == 0 { outer } else { inner };
        // Première pointe vers le haut
        let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / n as f32;
        let (x, y) = (cx + r * a.cos(), cy - r * a.sin());
        path.push(if i == 0 { PathCmd::MoveTo(x, y) } else { PathCmd::LineTo(x, y) });
    }
    path.push(PathCmd::Close);
    Ok(vec![Shape { path, fill: look.fill, stroke: look.color.map(|c| (c, look.stroke)) }])
}

// ─────────────────────────────────────────────
//  FLÈCHES ET CERCLES
// ─────────────────────────────────────────────

fn arrows(f: &ResolvedFigure) -> Result<(bool, bool), String> {
    match f.properties.get("arrow").map(fmt_value).as_deref() {
        None | Some("none") => Ok((false, false)),
        Some("end") => Ok((false, true)),
        Some("start") => Ok((true, false)),
        Some("both") => Ok((true, true)),
        Some(other) => Err(format!("arrow '{}' invalide (none / start / end / both)", other)),
    }
}

fn arrow_length(stroke: f32, scene: &Scene) -> f32 {
    (stroke * 4.0).max(14.0 * scene.px_scale)
}

/// Triangle plein dont la pointe est en (x, y), orienté selon (dx, dy) unitaire
fn arrow_head(x: f32, y: f32, dx: f32, dy: f32, len: f32, color: Rgba) -> Shape {
    let half = len * 0.45;
    let (bx, by) = (x - dx * len, y - dy * len); // base de la flèche
    let (nx, ny) = (-dy, dx); // normale
    Shape {
        path: vec![
            PathCmd::MoveTo(x, y),
            PathCmd::LineTo(bx + nx * half, by + ny * half),
            PathCmd::LineTo(bx - nx * half, by - ny * half),
            PathCmd::Close,
        ],
        fill: Some(color),
        stroke: None,
    }
}

/// Cercle exact à 0,03 % près avec 4 courbes de Bézier
fn circle_path(cx: f32, cy: f32, r: f32) -> Vec<PathCmd> {
    let k = 0.552_284_8 * r;
    vec![
        PathCmd::MoveTo(cx + r, cy),
        PathCmd::CubicTo(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
        PathCmd::CubicTo(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
        PathCmd::CubicTo(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
        PathCmd::CubicTo(cx + k, cy - r, cx + r, cy - k, cx + r, cy),
        PathCmd::Close,
    ]
}

fn unit(dx: f32, dy: f32) -> (f32, f32) {
    let n = (dx * dx + dy * dy).sqrt();
    if n == 0.0 { (1.0, 0.0) } else { (dx / n, dy / n) }
}

// ─────────────────────────────────────────────
//  LECTURE DES VALEURS
// ─────────────────────────────────────────────

fn require<'a>(f: &'a ResolvedFigure, key: &str) -> Result<&'a PropValue, String> {
    f.properties
        .get(key)
        .ok_or_else(|| format!("propriété `{}` manquante", key))
}

/// Position (x, y) en pixels : tuple (20%, 50%) ou référence dynamique center(fig), offset(fig, d)
fn point(v: &PropValue, scene: &Scene, refs: &Refs) -> Result<(f32, f32), String> {
    match v {
        PropValue::Tuple(items) if items.len() == 2 => {
            Ok((coord(&items[0], scene, true)?, coord(&items[1], scene, false)?))
        }
        PropValue::Single(s) if s.contains('(') => ref_point(s, scene, refs),
        _ => Err(format!("position invalide '{}', attendu (x%, y%)", fmt_value(v))),
    }
}

/// Position écrite en texte : "(80%,50%)" ou "center(b)" (paramètre `to=` de move)
pub fn resolve_point(s: &str, scene: &Scene, refs: &Refs) -> Result<(f32, f32), String> {
    if s.trim_start().starts_with('(') {
        point_str(s, scene)
    } else {
        ref_point(s, scene, refs)
    }
}

/// Récupère la géométrie d'une figure référencée
fn geom<'a>(name: &str, refs: &'a Refs) -> Result<&'a Geom, String> {
    refs.get(name)
        .ok_or_else(|| format!("la figure référencée '{}' n'a pas pu être calculée", name))
}

/// center(fig) ou offset(fig, d)
fn ref_point(s: &str, scene: &Scene, refs: &Refs) -> Result<(f32, f32), String> {
    let (fname, args) = split_call(s);
    match (fname, args.as_slice()) {
        ("center", [name]) => Ok(geom(name, refs)?.center),
        ("offset", [name, d]) => {
            let g = geom(name, refs)?;
            let d = length_str(d, scene)?;
            Ok(match g.label {
                // Milieu d'un arc : on s'écarte vers l'extérieur
                Some(((x, y), (dx, dy))) => (x + dx * d, y + dy * d),
                // Sinon : en haut à droite de la figure
                None => (g.bbox.2 + d, g.bbox.1 - d),
            })
        }
        ("center" | "offset", _) => Err(format!("'{}' : nombre d'arguments invalide", s)),
        ("auto_tangent", _) => Err(format!("'{}' : pas encore supporté (nécessite plot, Phase 3)", s)),
        // mes_axes(3, 4) : position dans un repère
        _ => Err(format!("'{}' : repère axes pas encore supporté (Phase 3)", s)),
    }
}

/// "offset(arc_theta,1%)" → ("offset", ["arc_theta", "1%"])
fn split_call(s: &str) -> (&str, Vec<&str>) {
    match s.split_once('(') {
        Some((name, rest)) => (
            name.trim(),
            rest.trim_end_matches(')').split(',').map(|a| a.trim()).filter(|a| !a.is_empty()).collect(),
        ),
        None => (s.trim(), Vec::new()),
    }
}

/// Longueur écrite en texte : "1%" (du plus petit côté) ou "5px"
fn length_str(s: &str, scene: &Scene) -> Result<f32, String> {
    if s.ends_with('%') {
        length(&PropValue::Percent(s.to_string()), scene, scene.min_side())
    } else {
        length(&PropValue::Single(s.to_string()), scene, 0.0)
    }
}

/// Position à partir d'une chaîne "(20%,50%)" (élément d'une liste de points)
fn point_str(s: &str, scene: &Scene) -> Result<(f32, f32), String> {
    let inside = s.trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = inside.split(',').map(|p| p.trim()).collect();
    if parts.len() != 2 {
        return Err(format!("point invalide '{}', attendu (x%, y%)", s));
    }
    Ok((coord(parts[0], scene, true)?, coord(parts[1], scene, false)?))
}

/// Une coordonnée : "20%" (de la largeur ou de la hauteur) ou un nombre de px.
/// y est converti : origine en bas (Didact) → origine en haut (pixels).
fn coord(s: &str, scene: &Scene, is_x: bool) -> Result<f32, String> {
    let base = if is_x { scene.width } else { scene.height };
    let v = if let Some(p) = s.strip_suffix('%') {
        p.parse::<f32>().map_err(|_| format!("pourcentage invalide '{}'", s))? / 100.0 * base
    } else {
        let px = s.strip_suffix("px").unwrap_or(s);
        px.parse::<f32>().map_err(|_| format!("coordonnée invalide '{}'", s))? * scene.px_scale
    };
    Ok(if is_x { v } else { scene.height - v })
}

/// Une longueur en pixels : "10%" (de `base`), "2px", ou un nombre (px)
fn length(v: &PropValue, scene: &Scene, base: f32) -> Result<f32, String> {
    match v {
        PropValue::Percent(p) => {
            let n: f32 = p.trim_end_matches('%').parse().map_err(|_| format!("pourcentage invalide '{}'", p))?;
            Ok(n / 100.0 * base)
        }
        PropValue::Number(n) => Ok(*n as f32 * scene.px_scale),
        PropValue::Single(s) if s.ends_with("px") => {
            let n: f32 = s.trim_end_matches("px").parse().map_err(|_| format!("longueur invalide '{}'", s))?;
            Ok(n * scene.px_scale)
        }
        _ => Err(format!("longueur '{}' pas supportée (utiliser % ou px)", fmt_value(v))),
    }
}

/// Angle en degrés : 90, "90deg" ou angle(fig)
fn angle(v: &PropValue, refs: &Refs) -> Result<f32, String> {
    match v {
        PropValue::Number(n) => Ok(*n as f32),
        PropValue::Single(s) if s.ends_with("deg") => s
            .trim_end_matches("deg")
            .parse()
            .map_err(|_| format!("angle invalide '{}'", s)),
        PropValue::Single(s) if s.contains('(') => match split_call(s) {
            ("angle", args) if args.len() == 1 => Ok(geom(args[0], refs)?.angle),
            _ => Err(format!("angle invalide '{}' (attendu un nombre ou angle(fig))", s)),
        },
        _ => Err(format!("angle invalide '{}'", fmt_value(v))),
    }
}

/// Couleur : nom (red), hexadécimal (#F00, #FF0000, #FF000080) ou none.
/// `Ok(None)` = none (pas de couleur).
pub fn parse_color(s: &str) -> Result<Option<Rgba>, String> {
    let s = s.trim();
    if s == "none" {
        return Ok(None);
    }
    if let Some(hex) = s.strip_prefix('#') {
        let digit = |i: usize, n: usize| u8::from_str_radix(&hex[i..i + n], 16).ok();
        let c = match hex.len() {
            3 => (|| Some(Rgba::rgb8(digit(0, 1)? * 17, digit(1, 1)? * 17, digit(2, 1)? * 17)))(),
            6 => (|| Some(Rgba::rgb8(digit(0, 2)?, digit(2, 2)?, digit(4, 2)?)))(),
            8 => (|| {
                let c = Rgba::rgb8(digit(0, 2)?, digit(2, 2)?, digit(4, 2)?);
                Some(Rgba { a: digit(6, 2)? as f32 / 255.0, ..c })
            })(),
            _ => None,
        };
        return c.map(Some).ok_or_else(|| format!("couleur hexadécimale invalide '{}'", s));
    }
    let (r, g, b) = match s {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (220, 38, 38),
        "green" => (22, 163, 74),
        "blue" => (37, 99, 235),
        "yellow" => (250, 204, 21),
        "orange" => (249, 115, 22),
        "purple" => (147, 51, 234),
        "pink" => (236, 72, 153),
        "brown" => (146, 64, 14),
        "gray" | "grey" => (115, 115, 115),
        "gold" => (234, 179, 8),
        "cyan" => (6, 182, 212),
        "navy" => (30, 58, 138),
        _ => return Err(format!("couleur inconnue '{}'", s)),
    };
    Ok(Some(Rgba::rgb8(r, g, b)))
}

// ─────────────────────────────────────────────
//  TESTS
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene { width: 1000.0, height: 500.0, px_scale: 1.0, background: Rgba::WHITE }
    }

    #[test]
    fn coordonnees_origine_en_bas() {
        let s = scene();
        assert_eq!(coord("0%", &s, false).unwrap(), 500.0); // bas de la scène
        assert_eq!(coord("100%", &s, false).unwrap(), 0.0); // haut
        assert_eq!(coord("25%", &s, true).unwrap(), 250.0);
    }

    #[test]
    fn longueurs() {
        let s = scene();
        assert_eq!(length(&PropValue::Percent("10%".into()), &s, s.min_side()).unwrap(), 50.0);
        assert_eq!(length(&PropValue::Single("4px".into()), &s, 0.0).unwrap(), 4.0);
        assert!(length(&PropValue::Single("3cm".into()), &s, 0.0).is_err());
    }

    #[test]
    fn couleurs() {
        assert_eq!(parse_color("none").unwrap(), None);
        assert_eq!(parse_color("#FF0000").unwrap(), Some(Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }));
        assert_eq!(parse_color("#fff").unwrap(), Some(Rgba::WHITE));
        assert_eq!(parse_color("#00000080").unwrap().unwrap().a, 128.0 / 255.0);
        assert!(parse_color("rouge").is_err());
        assert!(parse_color("#12").is_err());
    }

    #[test]
    fn rect_carre_quel_que_soit_le_ratio() {
        use std::collections::HashMap;
        let mut properties = HashMap::new();
        properties.insert("type".to_string(), PropValue::Single("rect".into()));
        properties.insert("pos".to_string(), PropValue::Tuple(vec!["10%".into(), "10%".into()]));
        properties.insert("width".to_string(), PropValue::Percent("20%".into()));
        properties.insert("height".to_string(), PropValue::Percent("20%".into()));
        let f = ResolvedFigure {
            name: "r".into(),
            index: 0,
            ftype: "rect".into(),
            layer: 0.0,
            line: 1,
            properties,
            prop_lines: HashMap::new(),
        };
        let shapes = figure_shapes(&f, &scene(), &Refs::new()).unwrap(); // scène 1000 × 500
        let xs: Vec<f32> = shapes[0].path.iter().filter_map(|c| match c { PathCmd::LineTo(x, _) | PathCmd::MoveTo(x, _) => Some(*x), _ => None }).collect();
        let ys: Vec<f32> = shapes[0].path.iter().filter_map(|c| match c { PathCmd::LineTo(_, y) | PathCmd::MoveTo(_, y) => Some(*y), _ => None }).collect();
        let w = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        let h = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
        assert_eq!((w, h), (100.0, 100.0)); // 20 % de 500 dans les deux sens
    }

    #[test]
    fn angles() {
        let refs = Refs::new();
        assert_eq!(angle(&PropValue::Number(90.0), &refs).unwrap(), 90.0);
        assert_eq!(angle(&PropValue::Single("45deg".into()), &refs).unwrap(), 45.0);
    }
}
