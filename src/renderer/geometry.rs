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

    pub fn rgb8(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    pub fn with_opacity(self, opacity: f32) -> Rgba {
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
    /// Rayon du halo lumineux en pixels (0 : pas de halo)
    pub glow: f32,
    /// Image (couleur par pixel) posée sur la boîte du chemin ; `fill` en donne l'opacité
    pub image: Option<std::sync::Arc<Raster>>,
}

/// Image calculée pixel par pixel : RGBA prémultiplié, ligne par ligne, de haut en bas
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
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
    /// Pour une figure `axes` : conversion coordonnées du repère → pixels
    pub axes: Option<AxesMap>,
    /// Contour de la figure en points (pixels), pour follow et auto_tangent
    pub outline: Vec<(f32, f32)>,
}

/// Conversion entre les coordonnées d'un repère `axes` et les pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxesMap {
    /// Position en pixels du point (0, 0) du repère (peut être hors de la boîte)
    pub ox: f32,
    pub oy: f32,
    /// Pixels par unité, en x et en y (égaux sauf `equal : false`)
    pub ux: f32,
    pub uy: f32,
    pub xrange: (f64, f64),
    pub yrange: (f64, f64),
    /// Pas des graduations en x et en y (`step`, sinon choisi selon la fenêtre)
    pub step: (f64, f64),
}

impl AxesMap {
    pub fn to_px(&self, x: f64, y: f64) -> (f32, f32) {
        (self.ox + x as f32 * self.ux, self.oy - y as f32 * self.uy)
    }
}

/// Ce que les figures peuvent référencer : géométries des figures déjà calculées (par nom)
/// et valeur des paramètres à l'instant t
#[derive(Debug, Clone, Default)]
pub struct Refs {
    geoms: HashMap<String, Geom>,
    pub params: HashMap<String, f64>,
    /// Scènes 3D (caméra à l'instant t)
    pub spaces: HashMap<String, super::three::Space>,
    /// Surfaces 3D : `f(a, b)` place un point dessus
    pub surfaces: HashMap<String, super::three::SurfaceRef>,
    /// Formules des plots 2D (pour iterate)
    pub plots: HashMap<String, super::three::PlotRef>,
    /// Chemins 3D suivis par follow (coordonnées de la scène)
    pub paths3: HashMap<String, Vec<super::three::V3>>,
    /// Centres des objets 3D (coordonnées de la scène)
    pub centers3: HashMap<String, super::three::V3>,
    /// Scène de chaque objet 3D
    pub scene_of: HashMap<String, String>,
}

impl Refs {
    pub fn new() -> Refs {
        Refs::default()
    }

    pub fn with_params(params: HashMap<String, f64>) -> Refs {
        Refs { params, ..Refs::default() }
    }

    pub fn param_names(&self) -> Vec<String> {
        self.params.keys().cloned().collect()
    }
}

impl std::ops::Deref for Refs {
    type Target = HashMap<String, Geom>;
    fn deref(&self) -> &Self::Target {
        &self.geoms
    }
}

impl std::ops::DerefMut for Refs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.geoms
    }
}

/// La zone de dessin
#[derive(Debug, Clone)]
pub struct Scene {
    /// Taille en pixels
    pub width: f32,
    pub height: f32,
    /// Facteur appliqué aux valeurs en `px` (taille réelle / taille de référence)
    pub px_scale: f32,
    pub background: Rgba,
    /// Caméra tournée à la souris dans la fenêtre : (élévation, rotation) en degrés, et facteur de zoom.
    /// Ajoutée à la caméra de chaque scène 3D ; (0, 0, 1) partout ailleurs.
    pub orbit: (f32, f32, f32),
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
            orbit: (0.0, 0.0, 1.0),
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

    pub(crate) fn min_side(&self) -> f32 {
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

    let shapes = figure_shapes_inner(f, scene, refs, &look, opacity)?;
    // `anchor` : quel point de la figure se pose sur `pos` (textes, tableaux, graphes)
    if ANCHORED.contains(&f.ftype.as_str()) {
        if let Some(a) = f.properties.get("anchor") {
            let (fx, fy) = anchor_frac(&fmt_value(a))?;
            let (x, y) = point(require(f, "pos")?, scene, refs)?;
            return Ok(anchor_shapes(shapes, x, y, fx, fy));
        }
    }
    Ok(shapes)
}

/// Types placés par `pos` dont `anchor` déplace toute la figure (le rect le gère lui-même)
const ANCHORED: &[&str] = &["text", "equation", "latex", "raw.latex", "table", "array", "graph"];

/// Les 9 points d'ancrage : fraction (horizontale, verticale) depuis le coin haut-gauche
pub(crate) fn anchor_frac(name: &str) -> Result<(f32, f32), String> {
    Ok(match name.trim() {
        "topleft" => (0.0, 0.0),
        "top" => (0.5, 0.0),
        "topright" => (1.0, 0.0),
        "left" => (0.0, 0.5),
        "center" => (0.5, 0.5),
        "right" => (1.0, 0.5),
        "bottomleft" => (0.0, 1.0),
        "bottom" => (0.5, 1.0),
        "bottomright" => (1.0, 1.0),
        other => {
            return Err(format!(
                "anchor '{}' invalide (topleft, top, topright, left, center, right, bottomleft, bottom, bottomright)",
                other
            ))
        }
    })
}

/// Déplace des formes pour que le point (fx, fy) de leur boîte soit en (x, y)
fn anchor_shapes(mut shapes: Vec<Shape>, x: f32, y: f32, fx: f32, fy: f32) -> Vec<Shape> {
    let mut b = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for s in &shapes {
        for c in &s.path {
            let mut add = |px: f32, py: f32| b = (b.0.min(px), b.1.min(py), b.2.max(px), b.3.max(py));
            match *c {
                PathCmd::MoveTo(px, py) | PathCmd::LineTo(px, py) => add(px, py),
                PathCmd::CubicTo(_, _, _, _, px, py) => add(px, py),
                PathCmd::Close => {}
            }
        }
    }
    if b.0 > b.2 {
        return shapes;
    }
    let (dx, dy) = (x - (b.0 + fx * (b.2 - b.0)), y - (b.1 + fy * (b.3 - b.1)));
    for s in &mut shapes {
        for c in &mut s.path {
            match c {
                PathCmd::MoveTo(px, py) | PathCmd::LineTo(px, py) => {
                    *px += dx;
                    *py += dy;
                }
                PathCmd::CubicTo(a, b2, c2, d, px, py) => {
                    *a += dx;
                    *b2 += dy;
                    *c2 += dx;
                    *d += dy;
                    *px += dx;
                    *py += dy;
                }
                PathCmd::Close => {}
            }
        }
    }
    shapes
}

fn figure_shapes_inner(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look, opacity: f32) -> Result<Vec<Shape>, String> {
    match f.ftype.as_str() {
        "line" => line(f, scene, refs, &look),
        "circle" => circle(f, scene, refs, &look),
        "rect" => rect(f, scene, refs, &look),
        "polygon" => polygon(f, scene, refs, &look),
        "arc" => arc(f, scene, refs, &look),
        "star" => star(f, scene, refs, &look),
        "text" => text(f, scene, refs, &look),
        "equation" => equation(f, scene, refs, &look),
        "latex" | "raw.latex" => latex(f, scene, refs, &look),
        "table" => table(f, scene, refs, &look),
        "axes" => axes(f, scene, refs, &look),
        "plot" => plot(f, scene, refs, &look),
        "grid" => grid(f, scene, refs, &look),
        "image" => super::image::shapes(f, scene, refs, opacity),
        "graph" | "array" => Ok(super::discrete::build(f, scene, refs)?.0),
        "pixels" => {
            let axes = f.properties.get("axes").map(fmt_value).and_then(|a| refs.get(&a).and_then(|g| g.axes));
            let extra = format!("{:?} {} {} {} {} {}", axes, scene.width, scene.height, scene.px_scale, opacity, super::pixels::geometry_only());
            let key = super::memo::key(&f.properties, &refs.params, &extra);
            super::memo::get_or(&format!("2d:{}", f.name), key, || super::pixels::shapes(f, scene, refs, opacity))
        }
        "contour" | "region" => {
            let color = if f.properties.contains_key("color") { look.color } else { None };
            // Calcul gardé d'une image à l'autre tant que rien dont il dépend ne change
            let axes = f.properties.get("axes").map(fmt_value).and_then(|a| refs.get(&a).and_then(|g| g.axes));
            let extra = format!("{} {:?} {} {} {}", f.ftype, axes, scene.width, scene.height, scene.px_scale);
            let key = super::memo::key(&f.properties, &refs.params, &extra);
            super::memo::get_or(&format!("2d:{}", f.name), key, || super::contour::shapes(f, refs, color, look.fill, look.stroke, opacity))
        }
        // Tirés par formule (count) : calcul gardé tant que rien ne change
        "scatter" | "histogram" if f.properties.contains_key("count") => {
            let axes = f.properties.get("axes").map(fmt_value).and_then(|a| refs.get(&a).and_then(|g| g.axes));
            let extra = format!("{} {:?} {} {} {}", f.ftype, axes, scene.width, scene.height, scene.px_scale);
            let key = super::memo::key(&f.properties, &refs.params, &extra);
            super::memo::get_or(&format!("2d:{}", f.name), key, || {
                if f.ftype == "scatter" { scatter(f, scene, refs, &look) } else { histogram(f, scene, refs, &look) }
            })
        }
        "scatter" => scatter(f, scene, refs, &look),
        "histogram" => histogram(f, scene, refs, &look),
        "arrow" | "field" | "iterate" | "trajectory" => super::three::shapes_2d(f, scene, refs),
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
        Shape { glow: 0.0,
            path: vec![PathCmd::MoveTo(ax, ay), PathCmd::LineTo(bx, by)],
            fill: None,
            stroke: look.color.map(|c| (c, look.stroke)),
            image: None,
        },
    );
    Ok(shapes)
}

fn circle(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (cx, cy) = point(require(f, "center")?, scene, refs)?;
    // radius : r (cercle) ou (rx, ry) (ellipse)
    let path = match require(f, "radius")? {
        PropValue::Tuple(v) if v.len() == 2 => {
            let (rx, ry) = (length_str(&v[0], scene)?, length_str(&v[1], scene)?);
            ellipse_path(cx, cy, rx, ry)
        }
        PropValue::Tuple(_) => return Err("radius : un nombre, ou (rx, ry) pour une ellipse".to_string()),
        v => circle_path(cx, cy, length(v, scene, scene.min_side())?),
    };
    Ok(vec![Shape { glow: 0.0,
        path,
        fill: look.fill,
        stroke: look.color.map(|c| (c, look.stroke)),
        image: None,
    }])
}

fn rect(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    // En % du plus petit côté : width = height donne toujours un carré
    let w = length(require(f, "width")?, scene, scene.min_side())?;
    let h = length(require(f, "height")?, scene, scene.min_side())?;

    // pos est le centre, sauf `anchor` (bottomleft : pos est le coin bas-gauche, ...)
    let (fx, fy) = match f.properties.get("anchor") {
        Some(a) => anchor_frac(&fmt_value(a))?,
        None => (0.5, 0.5),
    };
    let (left, top) = (x - fx * w, y - fy * h);
    // Coins arrondis : corner_radius en % du plus petit côté de l'écran, ou en px
    let r = match f.properties.get("corner_radius") {
        Some(v) => length(v, scene, scene.min_side())?,
        None => 0.0,
    };

    Ok(vec![Shape { glow: 0.0,
        path: rounded_rect_path(left, top, w, h, r),
        fill: look.fill,
        stroke: look.color.map(|c| (c, look.stroke)),
        image: None,
    }])
}

fn polygon(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let items = match require(f, "points")? {
        PropValue::List(items) => items,
        v => return Err(format!("points doit être une liste de points, trouvé '{}'", fmt_value(v))),
    };
    // closed : false → ligne brisée (pas de segment de retour, pas de remplissage)
    let closed = flag(f, "closed", true)?;
    let min = if closed { 3 } else { 2 };
    if items.len() < min {
        return Err(format!("un polygon {}doit avoir au moins {} points", if closed { "" } else { "ouvert " }, min));
    }
    let mut path = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let (x, y) = resolve_point(item, scene, refs)?;
        path.push(if i == 0 { PathCmd::MoveTo(x, y) } else { PathCmd::LineTo(x, y) });
    }
    if closed {
        path.push(PathCmd::Close);
    }
    let fill = if closed { look.fill } else { None };
    Ok(vec![Shape { glow: 0.0, path, fill, stroke: look.color.map(|c| (c, look.stroke)), image: None }])
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

    let mut shapes = vec![Shape { glow: 0.0, path, fill: None, stroke: look.color.map(|c| (c, look.stroke)), image: None }];
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
    // Rayon intérieur / extérieur : `inner` (défaut 0.382, étoile régulière à 5 branches)
    let ratio = match f.properties.get("inner") {
        Some(PropValue::Number(n)) if *n > 0.0 && *n < 1.0 => *n as f32,
        None => 0.382,
        Some(v) => return Err(format!("inner : rapport rayon intérieur / extérieur entre 0 et 1 (ex : 0.5), trouvé '{}'", fmt_value(v))),
    };
    let inner = outer * ratio;

    let mut path = Vec::with_capacity(2 * n + 1);
    for i in 0..2 * n {
        let r = if i % 2 == 0 { outer } else { inner };
        // Première pointe vers le haut
        let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / n as f32;
        let (x, y) = (cx + r * a.cos(), cy - r * a.sin());
        path.push(if i == 0 { PathCmd::MoveTo(x, y) } else { PathCmd::LineTo(x, y) });
    }
    path.push(PathCmd::Close);
    Ok(vec![Shape { glow: 0.0, path, fill: look.fill, stroke: look.color.map(|c| (c, look.stroke)), image: None }])
}

/// Formes des points d'un scatter
const MARKERS: &[&str] = &["circle", "square", "diamond", "triangle", "cross", "plus"];

/// Un marqueur de « rayon » r centré en (x, y) ; même surface apparente à peu près pour tous
fn marker_shape(marker: &str, x: f32, y: f32, r: f32, color: Option<Rgba>) -> Shape {
    let poly = |pts: &[(f32, f32)]| {
        let mut p: Vec<PathCmd> = pts
            .iter()
            .enumerate()
            .map(|(i, &(dx, dy))| if i == 0 { PathCmd::MoveTo(x + dx, y + dy) } else { PathCmd::LineTo(x + dx, y + dy) })
            .collect();
        p.push(PathCmd::Close);
        p
    };
    let filled = |path| Shape { glow: 0.0, path, fill: color, stroke: None, image: None };
    // Croix : deux traits, épaisseur proportionnelle à la taille
    let lines = |segs: [(f32, f32, f32, f32); 2]| Shape {
        glow: 0.0,
        path: segs
            .iter()
            .flat_map(|&(a, b, c, d)| [PathCmd::MoveTo(x + a, y + b), PathCmd::LineTo(x + c, y + d)])
            .collect(),
        fill: None,
        stroke: color.map(|c| (c, r * 0.45)),
        image: None,
    };
    match marker {
        "square" => {
            let a = r * 0.886; // même aire que le disque
            filled(poly(&[(-a, -a), (a, -a), (a, a), (-a, a)]))
        }
        "diamond" => {
            let a = r * 1.25;
            filled(poly(&[(0.0, -a), (a, 0.0), (0.0, a), (-a, 0.0)]))
        }
        "triangle" => {
            // pointe vers le haut (y pixel vers le bas)
            let a = r * 1.35;
            filled(poly(&[(0.0, -a), (a * 0.866, a * 0.5), (-a * 0.866, a * 0.5)]))
        }
        "cross" => {
            let a = r * 0.9;
            lines([(-a, -a, a, a), (-a, a, a, -a)])
        }
        "plus" => {
            let a = r * 1.1;
            lines([(-a, 0.0, a, 0.0), (0.0, -a, 0.0, a)])
        }
        _ => filled(circle_path(x, y, r)),
    }
}

/// Taille de police par défaut (px de la scène de référence)
const DEFAULT_FONT_PX: f32 = 32.0;

fn text(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let content = match require(f, "content")? {
        PropValue::Single(s) | PropValue::Block(s) => s.clone(),
        v => fmt_value(v),
    };
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    let size = match f.properties.get("font") {
        Some(v) => length(v, scene, scene.min_side())?,
        None => DEFAULT_FONT_PX * scene.px_scale,
    };
    let align = match f.properties.get("align").map(fmt_value).as_deref() {
        None | Some("center") => super::text::Align::Center,
        Some("left") => super::text::Align::Left,
        Some("right") => super::text::Align::Right,
        Some(other) => return Err(format!("align '{}' invalide (left / center / right)", other)),
    };
    let color = look.color.unwrap_or(Rgba::BLACK);
    super::text::text_shapes(&content, x, y, size, align, color)
}

/// Texte brut d'une propriété (chaîne, bloc `|`, ou valeur simple)
fn raw_text(v: &PropValue) -> String {
    match v {
        PropValue::Single(s) | PropValue::Block(s) => s.clone(),
        v => fmt_value(v),
    }
}

/// Taille de composition : `size`, sinon `font`, sinon la valeur par défaut × `factor`
fn font_size(f: &ResolvedFigure, scene: &Scene, factor: f32) -> Result<f32, String> {
    match f.properties.get("size").or(f.properties.get("font")) {
        Some(v) => length(v, scene, scene.min_side()),
        None => Ok(DEFAULT_FONT_PX * factor * scene.px_scale),
    }
}

/// equation : une formule LaTeX (mode mathématique)
fn equation(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let tex = raw_text(require(f, "expr")?);
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    let size = font_size(f, scene, 1.25)?;
    let color = look.color.unwrap_or(Rgba::BLACK);
    let content = super::typeset::Content::Math(tex);
    let align = match f.properties.get("align").map(fmt_value).as_deref() {
        None | Some("center") => super::typeset::Align::Center,
        Some("left") => super::typeset::Align::Left,
        Some("right") => super::typeset::Align::Right,
        Some(other) => return Err(format!("align '{}' invalide (left / center / right)", other)),
    };
    super::typeset::shapes(&content, x, y, size, align, color)
}

/// latex / raw.latex : du LaTeX en mode texte (`\begin{align}`, paragraphes, ...)
fn latex(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let key = if f.ftype == "raw.latex" { "code" } else { "content" };
    let tex = raw_text(require(f, key)?);
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    let size = font_size(f, scene, 1.0)?;
    let color = look.color.unwrap_or(Rgba::BLACK);
    super::typeset::shapes(&super::typeset::Content::Latex(tex), x, y, size, super::typeset::Align::Center, color)
}

/// table : en-tête (liste) + lignes (bloc, cases séparées par |)
fn table(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let header = match f.properties.get("header") {
        Some(PropValue::List(items)) => items.clone(),
        Some(v) => return Err(format!("header doit être une liste, trouvé '{}'", fmt_value(v))),
        None => Vec::new(),
    };
    let rows: Vec<Vec<String>> = raw_text(require(f, "rows")?)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('|').map(|c| c.trim().to_string()).collect())
        .collect();
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    let size = font_size(f, scene, 1.0)?;
    let color = look.color.unwrap_or(Rgba::BLACK);
    let content = super::typeset::Content::Table { header, rows };
    super::typeset::shapes(&content, x, y, size, super::typeset::Align::Center, color)
}

// ─────────────────────────────────────────────
//  REPÈRES ET GRAPHES
// ─────────────────────────────────────────────

/// Intervalle [a, b] écrit comme une liste
pub(crate) fn range(f: &ResolvedFigure, key: &str) -> Result<(f64, f64), String> {
    match require(f, key)? {
        PropValue::List(items) if items.len() == 2 => {
            let a: f64 = items[0].parse().map_err(|_| format!("{} : nombre invalide '{}'", key, items[0]))?;
            let b: f64 = items[1].parse().map_err(|_| format!("{} : nombre invalide '{}'", key, items[1]))?;
            if b <= a {
                return Err(format!("{} : [{}, {}] doit aller du plus petit au plus grand", key, a, b));
            }
            Ok((a, b))
        }
        v => Err(format!("{} doit être [min, max], trouvé '{}'", key, fmt_value(v))),
    }
}

fn flag(f: &ResolvedFigure, key: &str, default: bool) -> Result<bool, String> {
    match f.properties.get(key).map(fmt_value).as_deref() {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(other) => Err(format!("{} doit être true ou false, trouvé '{}'", key, other)),
    }
}

/// Repère : la boîte from/to à l'écran, les intervalles xrange/yrange.
/// Échelle égale en x et en y par défaut : le repère est centré dans la boîte.
pub fn axes_map(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<AxesMap, String> {
    let (x1, y1) = point(require(f, "from")?, scene, refs)?;
    let (x2, y2) = point(require(f, "to")?, scene, refs)?;
    let (xrange, yrange) = view(f)?;
    let (left, right) = (x1.min(x2), x1.max(x2));
    let (top, bottom) = (y1.min(y2), y1.max(y2));
    let (box_w, box_h) = (right - left, bottom - top);
    let (span_x, span_y) = ((xrange.1 - xrange.0) as f32, (yrange.1 - yrange.0) as f32);

    let (mut ux, mut uy) = (box_w / span_x, box_h / span_y);
    if flag(f, "equal", true)? {
        let u = ux.min(uy);
        ux = u;
        uy = u;
    }
    // Zone réellement utilisée, centrée dans la boîte
    let (w, h) = (span_x * ux, span_y * uy);
    let area_left = left + (box_w - w) / 2.0;
    let area_bottom = bottom - (box_h - h) / 2.0;
    let step = tick_steps(f, xrange, yrange)?;
    Ok(AxesMap {
        ox: area_left - xrange.0 as f32 * ux,
        oy: area_bottom + yrange.0 as f32 * uy,
        ux,
        uy,
        xrange,
        yrange,
        step,
    })
}

/// Pas des graduations d'un repère : `step : 0.5`, `step : pi/2`, `step : (pi/2, 0.5)`,
/// `auto` (défaut : 1, 2 ou 5 × 10^k selon la fenêtre, suit le zoom)
fn tick_steps(f: &ResolvedFigure, xr: (f64, f64), yr: (f64, f64)) -> Result<(f64, f64), String> {
    let one = |s: &str, span: f64| -> Result<f64, String> {
        let s = s.trim();
        if s == "auto" {
            return Ok(nice_step(span));
        }
        let e = super::expr::parse_with(s, &[]).map_err(|e| format!("step '{}' : {}", s, e))?;
        let v = e.eval(0.0, &HashMap::new());
        if v.is_finite() && v > 0.0 {
            Ok(v)
        } else {
            Err(format!("step '{}' : un pas positif (ex : 0.5, pi/2, auto)", s))
        }
    };
    let (sx, sy) = (xr.1 - xr.0, yr.1 - yr.0);
    match f.properties.get("step") {
        None => Ok((nice_step(sx), nice_step(sy))),
        Some(PropValue::Number(n)) => Ok((one(&n.to_string(), sx)?, one(&n.to_string(), sy)?)),
        Some(PropValue::Tuple(v)) if v.len() == 2 => Ok((one(&v[0], sx)?, one(&v[1], sy)?)),
        Some(v) => {
            let s = fmt_value(v);
            Ok((one(&s, sx)?, one(&s, sy)?))
        }
    }
}

/// Une graduation : en multiples de π si le pas en est un (π/2, π/3, ...), sinon un nombre
fn fmt_tick(v: f64, step: f64) -> String {
    let pi = std::f64::consts::PI;
    if let Some(q) = (1..=12).find(|q| {
        let n = step / pi * *q as f64;
        n.round() >= 1.0 && (n - n.round()).abs() < 1e-9
    }) {
        // v = (p/q) π
        let p = (v / pi * q as f64).round() as i64;
        let g = gcd(p.unsigned_abs(), q as u64) as i64;
        let (p, q) = (p / g.max(1), q as i64 / g.max(1));
        let sign = if p < 0 { "−" } else { "" };
        let num = match p.abs() {
            0 => return "0".into(),
            1 => "π".to_string(),
            n => format!("{}π", n),
        };
        return if q == 1 { format!("{}{}", sign, num) } else { format!("{}{}/{}", sign, num, q) };
    }
    fmt_number(v, decimals_for(step).max(4))
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Fenêtre d'un repère : xrange × yrange, recentrée sur `center : (x, y)` et agrandie
/// `zoom` fois (caméra 2D ; avec des paramètres, on zoome et on se déplace en les animant)
fn view(f: &ResolvedFigure) -> Result<((f64, f64), (f64, f64)), String> {
    let (xr, yr) = (range(f, "xrange")?, range(f, "yrange")?);
    let zoom = match f.properties.get("zoom") {
        None => 1.0,
        Some(PropValue::Number(z)) if *z > 0.0 && z.is_finite() => *z,
        Some(v) => return Err(format!("zoom invalide '{}' : un nombre > 0 (2 = deux fois plus près)", fmt_value(v))),
    };
    let (cx, cy) = match f.properties.get("center") {
        None => ((xr.0 + xr.1) / 2.0, (yr.0 + yr.1) / 2.0),
        Some(PropValue::Tuple(v)) if v.len() == 2 => {
            let n = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("center invalide '{}' : (x, y) dans le repère", s));
            (n(&v[0])?, n(&v[1])?)
        }
        Some(v) => return Err(format!("center invalide '{}' : (x, y) dans le repère", fmt_value(v))),
    };
    let (hx, hy) = ((xr.1 - xr.0) / 2.0 / zoom, (yr.1 - yr.0) / 2.0 / zoom);
    Ok(((cx - hx, cx + hx), (cy - hy, cy + hy)))
}

/// Pas de graduation « joli » (1, 2 ou 5 × 10^k) pour environ 8 graduations
fn nice_step(span: f64) -> f64 {
    let raw = span / 8.0;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let nice = if norm < 1.5 { 1.0 } else if norm < 3.5 { 2.0 } else if norm < 7.5 { 5.0 } else { 10.0 };
    nice * mag
}

/// Valeurs des graduations dans [a, b], tous les `step`
fn ticks_in(a: f64, b: f64, step: f64) -> Vec<f64> {
    let first = (a / step - 1e-9).ceil() as i64;
    let last = (b / step + 1e-9).floor() as i64;
    if last - first > 1000 {
        return Vec::new();
    }
    // k × pas (et non une somme de pas) : pas d'erreur qui s'accumule quand on zoome loin de 0
    (first..=last).map(|k| if k == 0 { 0.0 } else { k as f64 * step }).collect()
}

/// Nombre de décimales qui suffit pour écrire les multiples de `step`
fn decimals_for(step: f64) -> usize {
    (0..=12).find(|d| {
        let v = step * 10f64.powi(*d as i32);
        (v - v.round()).abs() < 1e-6 * v.abs().max(1.0)
    }).unwrap_or(12)
}

/// 2.0 → "2", 0.5 → "0.5", -1.25 → "−1.25" ; `decimals` : au plus
fn fmt_number(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    let s = if s == "-0" { "0" } else { s };
    s.replace('-', "−")
}

fn axes(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let m = axes_map(f, scene, refs)?;
    let color = look.color.unwrap_or(Rgba::BLACK);
    let (xa, xb) = m.xrange;
    let (ya, yb) = m.yrange;
    // Les axes passent par 0 si 0 est dans l'intervalle, sinon par le bord
    let x0 = 0f64.clamp(xa, xb);
    let y0 = 0f64.clamp(ya, yb);
    let font = 22.0 * scene.px_scale;
    let tick = 6.0 * scene.px_scale;
    let mut shapes = Vec::new();

    // Grille
    if flag(f, "grid", false)? {
        let grid = Rgba { r: 0.85, g: 0.87, b: 0.9, a: color.a };
        let mut path = Vec::new();
        for x in ticks_in(xa, xb, m.step.0) {
            let (a, b) = (m.to_px(x, ya), m.to_px(x, yb));
            path.extend([PathCmd::MoveTo(a.0, a.1), PathCmd::LineTo(b.0, b.1)]);
        }
        for y in ticks_in(ya, yb, m.step.1) {
            let (a, b) = (m.to_px(xa, y), m.to_px(xb, y));
            path.extend([PathCmd::MoveTo(a.0, a.1), PathCmd::LineTo(b.0, b.1)]);
        }
        shapes.push(Shape { glow: 0.0, path, fill: None, stroke: Some((grid, 1.0 * scene.px_scale)), image: None });
    }

    // Axes fléchés
    let head = arrow_length(look.stroke, scene);
    for (from, to) in [(m.to_px(xa, y0), m.to_px(xb, y0)), (m.to_px(x0, ya), m.to_px(x0, yb))] {
        let (dx, dy) = unit(to.0 - from.0, to.1 - from.1);
        shapes.push(Shape { glow: 0.0,
            path: vec![PathCmd::MoveTo(from.0, from.1), PathCmd::LineTo(to.0 - dx * head * 0.8, to.1 - dy * head * 0.8)],
            fill: None,
            stroke: Some((color, look.stroke)),
            image: None,
        });
        shapes.push(arrow_head(to.0, to.1, dx, dy, head, color));
    }

    // Graduations et nombres (sauf au croisement des axes)
    if flag(f, "ticks", true)? {
        let mut path = Vec::new();
        for x in ticks_in(xa, xb, m.step.0) {
            if x == x0 || (x - xb).abs() < m.step.0 * 1e-6 {
                continue;
            }
            let (px, py) = m.to_px(x, y0);
            path.extend([PathCmd::MoveTo(px, py - tick), PathCmd::LineTo(px, py + tick)]);
            shapes.extend(super::text::text_shapes(&fmt_tick(x, m.step.0), px, py + tick + font * 0.8, font, super::text::Align::Center, color)?);
        }
        for y in ticks_in(ya, yb, m.step.1) {
            if y == y0 || (y - yb).abs() < m.step.1 * 1e-6 {
                continue;
            }
            let (px, py) = m.to_px(x0, y);
            path.extend([PathCmd::MoveTo(px - tick, py), PathCmd::LineTo(px + tick, py)]);
            shapes.extend(super::text::text_shapes(&fmt_tick(y, m.step.1), px - tick - font * 0.4, py, font, super::text::Align::Right, color)?);
        }
        shapes.push(Shape { glow: 0.0, path, fill: None, stroke: Some((color, look.stroke * 0.75)), image: None });
    }

    // Noms des axes
    let label = |key: &str| f.properties.get(key).map(fmt_value).filter(|s| !s.is_empty());
    if let Some(xl) = label("xlabel") {
        let (px, py) = m.to_px(xb, y0);
        shapes.extend(super::text::text_shapes(&xl, px, py + font * 1.4, font * 1.2, super::text::Align::Right, color)?);
    }
    if let Some(yl) = label("ylabel") {
        let (px, py) = m.to_px(x0, yb);
        shapes.extend(super::text::text_shapes(&yl, px + font * 0.6, py + font * 0.5, font * 1.2, super::text::Align::Left, color)?);
    }
    Ok(shapes)
}

/// Repère référencé par `axes : nom`
fn axes_of<'a>(f: &ResolvedFigure, refs: &'a Refs) -> Result<&'a AxesMap, String> {
    let name = fmt_value(require(f, "axes")?);
    geom(&name, refs)?
        .axes
        .as_ref()
        .ok_or_else(|| format!("'{}' n'est pas un repère axes", name))
}

fn plot(f: &ResolvedFigure, _scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let m = axes_of(f, refs)?;
    let src = fmt_value(require(f, "expr")?);
    let e = super::expr::parse_with(&src, &refs.param_names())?;
    let (xa, xb) = m.xrange;
    let (ya, yb) = m.yrange;
    let f_has_samples = f.properties.contains_key("samples");
    let samples = match f.properties.get("samples") {
        Some(PropValue::Number(n)) if *n >= 2.0 => *n as usize,
        Some(v) => return Err(format!("samples doit être un entier ≥ 2, trouvé '{}'", fmt_value(v))),
        // Par défaut : un point tous les 2 pixels environ
        None => (((xb - xa) as f32 * m.ux / 2.0) as usize).clamp(100, 2000),
    };

    // Échantillonnage régulier, affiné là où la courbe s'écarte de plus d'un demi-pixel
    // d'un segment droit (oscillations rapides, zoom), sauf si `samples` est donné
    let f = |x: f64| e.eval(x, &refs.params);
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(samples + 1);
    let adaptive = !f_has_samples;
    for i in 0..=samples {
        let x = xa + (xb - xa) * i as f64 / samples as f64;
        let y = f(x);
        if adaptive && i > 0 {
            // Au plus 64 points ajoutés par intervalle : répartis sur toute la courbe
            let mut budget = 64usize;
            let (x0, y0) = *pts.last().unwrap();
            refine(&f, (x0, y0), (x, y), m, 0, &mut budget, &mut pts);
        }
        pts.push((x, y));
    }

    // Tracé, en coupant la courbe hors de yrange et là où elle n'est pas définie
    let mut path = Vec::new();
    let mut prev: Option<(f64, f64)> = None;
    for (x, y) in pts {
        let cur = if y.is_finite() { Some((x, y)) } else { None };
        if let (Some(a), Some(b)) = (prev, cur) {
            if let Some((p, q)) = clip_y(a, b, ya, yb) {
                let (p, q) = (m.to_px(p.0, p.1), m.to_px(q.0, q.1));
                let continues = matches!(path.last(), Some(PathCmd::LineTo(x, y)) | Some(PathCmd::MoveTo(x, y)) if (*x - p.0).abs() < 0.01 && (*y - p.1).abs() < 0.01);
                if !continues {
                    path.push(PathCmd::MoveTo(p.0, p.1));
                }
                path.push(PathCmd::LineTo(q.0, q.1));
            }
        }
        prev = cur;
    }
    Ok(vec![Shape { glow: 0.0, path, fill: None, stroke: look.color.map(|c| (c, look.stroke)), image: None }])
}

/// grid : quadrillage d'un repère (droites x = k·pas et y = k·pas), à déformer avec matrix / map / cmap.
/// `step : 1` ou `step : (1, 0.5)` ; `xrange` / `yrange` : étendue (par défaut celle du repère).
fn grid(f: &ResolvedFigure, _scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let m = axes_of(f, refs)?;
    let (xa, xb) = if f.properties.contains_key("xrange") { range(f, "xrange")? } else { m.xrange };
    let (ya, yb) = if f.properties.contains_key("yrange") { range(f, "yrange")? } else { m.yrange };
    let (sx, sy) = match f.properties.get("step") {
        None => (1.0, 1.0),
        // Comme les graduations : suit le zoom
        Some(PropValue::Single(a)) if a == "auto" => m.step,
        Some(PropValue::Number(n)) => (*n, *n),
        Some(PropValue::Tuple(v)) if v.len() == 2 => {
            let p = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("step invalide '{}'", s));
            (p(&v[0])?, p(&v[1])?)
        }
        Some(v) => return Err(format!("step invalide '{}' (ex : 1, (1, 0.5) ou auto)", fmt_value(v))),
    };
    if !(sx > 0.0 && sy > 0.0) {
        return Err("step doit être positif".to_string());
    }
    let lines = |a: f64, b: f64, s: f64| {
        let first = (a / s - 1e-9).ceil() as i64;
        let last = (b / s + 1e-9).floor() as i64;
        (first..=last).map(move |k| k as f64 * s)
    };
    if lines(xa, xb, sx).count() + lines(ya, yb, sy).count() > 2000 {
        return Err("grid : trop de lignes, augmenter `step`".to_string());
    }
    let mut path = Vec::new();
    for x in lines(xa, xb, sx) {
        let (p, q) = (m.to_px(x, ya), m.to_px(x, yb));
        path.push(PathCmd::MoveTo(p.0, p.1));
        path.push(PathCmd::LineTo(q.0, q.1));
    }
    for y in lines(ya, yb, sy) {
        let (p, q) = (m.to_px(xa, y), m.to_px(xb, y));
        path.push(PathCmd::MoveTo(p.0, p.1));
        path.push(PathCmd::LineTo(q.0, q.1));
    }
    Ok(vec![Shape { glow: 0.0, path, fill: None, stroke: look.color.map(|c| (c, look.stroke)), image: None }])
}

/// Ajoute entre a et b (exclus) les points nécessaires pour que la courbe ne s'écarte pas
/// de plus d'un demi-pixel des segments tracés
fn refine(
    f: &dyn Fn(f64) -> f64,
    a: (f64, f64),
    b: (f64, f64),
    m: &AxesMap,
    depth: u32,
    budget: &mut usize,
    out: &mut Vec<(f64, f64)>,
) {
    if depth >= 12 || *budget == 0 {
        return;
    }
    let xm = (a.0 + b.0) / 2.0;
    let ym = f(xm);
    // Une extrémité non définie : on ne cherche pas la frontière
    if !(a.1.is_finite() && b.1.is_finite() && ym.is_finite()) {
        return;
    }
    // Hors de yrange des deux côtés et au milieu : rien de visible
    let (lo, hi) = m.yrange;
    if (a.1 > hi && b.1 > hi && ym > hi) || (a.1 < lo && b.1 < lo && ym < lo) {
        return;
    }
    let dev = ((ym - (a.1 + b.1) / 2.0) * m.uy as f64).abs();
    if dev <= 0.5 {
        return;
    }
    *budget -= 1;
    refine(f, a, (xm, ym), m, depth + 1, budget, out);
    out.push((xm, ym));
    refine(f, (xm, ym), b, m, depth + 1, budget, out);
}

/// Partie du segment [a, b] comprise entre y = lo et y = hi (None si entièrement dehors)
fn clip_y(a: (f64, f64), b: (f64, f64), lo: f64, hi: f64) -> Option<((f64, f64), (f64, f64))> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let dy = b.1 - a.1;
    for (p, q) in [(-dy, a.1 - lo), (dy, hi - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 { t0 = t0.max(r) } else { t1 = t1.min(r) }
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + dy * t);
    Some((at(t0), at(t1)))
}

/// Valeurs tirées par formule : `count` valeurs de `key` (fonction de k = 1, 2, ..., count)
fn sampled(f: &ResolvedFigure, refs: &Refs, keys: &[&str]) -> Result<Vec<Vec<f64>>, String> {
    let count = match f.properties.get("count") {
        Some(PropValue::Number(n)) if *n >= 0.0 && *n <= 1e6 => n.round() as usize,
        Some(v) => return Err(format!("count : nombre de tirages entre 0 et 1000000, trouvé '{}'", fmt_value(v))),
        None => return Err("`count` manquant : nombre de tirages".to_string()),
    };
    let mut names = refs.param_names();
    names.push("k".into());
    let slots = vec!["k".to_string()];
    let fs = keys
        .iter()
        .map(|k| {
            let src = f.properties.get(*k).map(raw_text).ok_or_else(|| format!("`{}` manquant (formule en k)", k))?;
            super::expr::parse_with(&src, &names)
                .and_then(|e| super::expr::compile(&e, &slots, &refs.params))
                .map_err(|e| format!("{} '{}' : {}", k, src, e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((1..=count).map(|k| fs.iter().map(|g| g(&[k as f64])).collect()).collect())
}

fn scatter(f: &ResolvedFigure, scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let m = axes_of(f, refs)?;
    // Points tirés par formule : count, x, y (fonctions de k), et un filtre `where` facultatif
    let generated: Vec<String>;
    let items = if f.properties.contains_key("count") {
        let pts = sampled(f, refs, &["x", "y"])?;
        let keep = match f.properties.get("where") {
            Some(v) => {
                let src = raw_text(v);
                let mut names = refs.param_names();
                names.push("y".into());
                let e = super::expr::parse_with(&src, &names).map_err(|e| format!("where '{}' : {}", src, e))?;
                let c = super::expr::compile(&e, &["x".to_string(), "y".to_string()], &refs.params)?;
                Some(c)
            }
            None => None,
        };
        generated = pts
            .into_iter()
            .filter(|p| keep.as_ref().is_none_or(|c| c(&[p[0], p[1]]) != 0.0))
            .map(|p| format!("({}, {})", p[0], p[1]))
            .collect();
        &generated
    } else {
        match require(f, "points")? {
            PropValue::List(items) => items,
            v => return Err(format!("points doit être une liste de points, trouvé '{}'", fmt_value(v))),
        }
    };
    let r = match f.properties.get("size") {
        Some(v) => length(v, scene, scene.min_side())?,
        None => 6.0 * scene.px_scale,
    };
    let fill = look.fill.or(look.color);
    let marker = f.properties.get("marker").map(fmt_value).unwrap_or_else(|| "circle".into());
    if !MARKERS.contains(&marker.as_str()) {
        return Err(format!("marker '{}' inconnu ({})", marker, MARKERS.join(", ")));
    }
    let mut shapes = Vec::new();
    for item in items {
        let inside = item.trim().trim_start_matches('(').trim_end_matches(')');
        let parts: Vec<f64> = inside.split(',').map(|v| v.trim().parse::<f64>()).collect::<Result<_, _>>()
            .map_err(|_| format!("point invalide '{}', attendu (x, y) en coordonnées du repère", item))?;
        if parts.len() != 2 {
            return Err(format!("point invalide '{}', attendu (x, y)", item));
        }
        let (px, py) = m.to_px(parts[0], parts[1]);
        shapes.push(marker_shape(&marker, px, py, r, fill));
    }
    Ok(shapes)
}

fn histogram(f: &ResolvedFigure, _scene: &Scene, refs: &Refs, look: &Look) -> Result<Vec<Shape>, String> {
    let m = axes_of(f, refs)?;
    // Valeurs données (`data`) ou tirées par formule (`count` et `sample`, fonction de k)
    let data: Vec<f64> = if f.properties.contains_key("count") {
        sampled(f, refs, &["sample"])?.into_iter().map(|v| v[0]).filter(|v| v.is_finite()).collect()
    } else {
        match require(f, "data")? {
            PropValue::List(items) => items.iter().map(|v| v.parse::<f64>()).collect::<Result<_, _>>()
                .map_err(|_| "data doit être une liste de nombres".to_string())?,
            v => return Err(format!("data doit être une liste de nombres, trouvé '{}'", fmt_value(v))),
        }
    };
    if data.is_empty() {
        return Ok(Vec::new());
    }
    // `range : [a, b]` : classes fixes (un histogramme qui se remplit ne bouge pas)
    let bins = match f.properties.get("bins") {
        Some(PropValue::Number(n)) if *n >= 1.0 => *n as usize,
        Some(v) => return Err(format!("bins doit être un entier ≥ 1, trouvé '{}'", fmt_value(v))),
        None => 10,
    };
    let (lo, hi) = if f.properties.contains_key("range") {
        range(f, "range")?
    } else {
        (data.iter().cloned().fold(f64::MAX, f64::min), data.iter().cloned().fold(f64::MIN, f64::max))
    };
    let width = if hi > lo { (hi - lo) / bins as f64 } else { 1.0 };
    let mut counts = vec![0usize; bins];
    for v in &data {
        if *v < lo || *v > hi {
            continue;
        }
        let i = (((v - lo) / width) as usize).min(bins - 1);
        counts[i] += 1;
    }
    // `density : true` : aire totale 1, comparable à une densité tracée par `plot`
    let scale = if flag(f, "density", false)? { 1.0 / (data.len() as f64 * width) } else { 1.0 };
    let fill = look.fill.or(look.color.map(|c| Rgba { a: c.a * 0.8, ..c }));
    let edge = Rgba { a: 1.0, ..Rgba::WHITE };
    Ok(counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(i, c)| {
            let x0 = lo + i as f64 * width;
            let (a, b) = (m.to_px(x0, 0.0), m.to_px(x0 + width, *c as f64 * scale));
            Shape { glow: 0.0,
                path: vec![
                    PathCmd::MoveTo(a.0, a.1),
                    PathCmd::LineTo(b.0, a.1),
                    PathCmd::LineTo(b.0, b.1),
                    PathCmd::LineTo(a.0, b.1),
                    PathCmd::Close,
                ],
                fill,
                stroke: Some((edge, 1.0)),
                image: None,
            }
        })
        .collect())
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

pub(crate) fn arrow_length(stroke: f32, scene: &Scene) -> f32 {
    (stroke * 4.0).max(14.0 * scene.px_scale)
}

/// Triangle plein dont la pointe est en (x, y), orienté selon (dx, dy) unitaire
pub(crate) fn arrow_head(x: f32, y: f32, dx: f32, dy: f32, len: f32, color: Rgba) -> Shape {
    let half = len * 0.45;
    let (bx, by) = (x - dx * len, y - dy * len); // base de la flèche
    let (nx, ny) = (-dy, dx); // normale
    Shape { glow: 0.0,
        path: vec![
            PathCmd::MoveTo(x, y),
            PathCmd::LineTo(bx + nx * half, by + ny * half),
            PathCmd::LineTo(bx - nx * half, by - ny * half),
            PathCmd::Close,
        ],
        fill: Some(color),
        stroke: None,
        image: None,
    }
}

/// Cercle exact à 0,03 % près avec 4 courbes de Bézier
pub(crate) fn circle_path(cx: f32, cy: f32, r: f32) -> Vec<PathCmd> {
    ellipse_path(cx, cy, r, r)
}

/// Ellipse de demi-axes rx (horizontal) et ry (vertical) : 4 arcs de Bézier
pub(crate) fn ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathCmd> {
    let (kx, ky) = (0.552_284_8 * rx, 0.552_284_8 * ry);
    vec![
        PathCmd::MoveTo(cx + rx, cy),
        PathCmd::CubicTo(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry),
        PathCmd::CubicTo(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy),
        PathCmd::CubicTo(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry),
        PathCmd::CubicTo(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy),
        PathCmd::Close,
    ]
}

/// Rectangle aux coins arrondis de rayon r (r = 0 : coins droits)
fn rounded_rect_path(left: f32, top: f32, w: f32, h: f32, r: f32) -> Vec<PathCmd> {
    let r = r.clamp(0.0, w.abs().min(h.abs()) / 2.0);
    let (right, bottom) = (left + w, top + h);
    if r <= 0.0 {
        return vec![
            PathCmd::MoveTo(left, top),
            PathCmd::LineTo(right, top),
            PathCmd::LineTo(right, bottom),
            PathCmd::LineTo(left, bottom),
            PathCmd::Close,
        ];
    }
    let k = (1.0 - 0.552_284_8) * r;
    vec![
        PathCmd::MoveTo(left + r, top),
        PathCmd::LineTo(right - r, top),
        PathCmd::CubicTo(right - k, top, right, top + k, right, top + r),
        PathCmd::LineTo(right, bottom - r),
        PathCmd::CubicTo(right, bottom - k, right - k, bottom, right - r, bottom),
        PathCmd::LineTo(left + r, bottom),
        PathCmd::CubicTo(left + k, bottom, left, bottom - k, left, bottom - r),
        PathCmd::LineTo(left, top + r),
        PathCmd::CubicTo(left, top + k, left + k, top, left + r, top),
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

pub(crate) fn require<'a>(f: &'a ResolvedFigure, key: &str) -> Result<&'a PropValue, String> {
    f.properties
        .get(key)
        .ok_or_else(|| format!("propriété `{}` manquante", key))
}

/// Position (x, y) en pixels : tuple (20%, 50%) ou référence dynamique center(fig), offset(fig, d)
pub(crate) fn point(v: &PropValue, scene: &Scene, refs: &Refs) -> Result<(f32, f32), String> {
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

/// Géométrie d'une figure référencée (pour le renderer : follow, auto_tangent)
pub fn resolve_geom<'a>(name: &str, refs: &'a Refs) -> Result<&'a Geom, String> {
    geom(name, refs)
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
        ("center", [name]) if !refs.centers3.contains_key(*name) => Ok(geom(name, refs)?.center),
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
        // Point d'une scène ou d'une surface 3D, ou centre d'un objet 3D : projeté à l'écran
        _ if super::three::point_px(s, refs).is_some() => super::three::point_px(s, refs).unwrap(),
        ("center" | "offset", _) => Err(format!("'{}' : nombre d'arguments invalide", s)),
        // mes_axes(3, 4) : position dans un repère
        (name, [x, y]) => {
            let m = geom(name, refs)?
                .axes
                .ok_or_else(|| format!("'{}' : '{}' n'est pas un repère axes", s, name))?;
            let x = coordinate(x, refs).map_err(|e| format!("'{}' : {}", s, e))?;
            let y = coordinate(y, refs).map_err(|e| format!("'{}' : {}", s, e))?;
            Ok(m.to_px(x, y))
        }
        _ => Err(format!("position invalide '{}'", s)),
    }
}

/// Coordonnée dans un repère : un nombre, ou une expression avec des paramètres (`a - 1`, `sin(a)`)
fn coordinate(s: &str, refs: &Refs) -> Result<f64, String> {
    if let Ok(v) = s.parse::<f64>() {
        return Ok(v);
    }
    let e = super::expr::parse_with(s, &refs.param_names())?;
    Ok(e.eval(0.0, &refs.params))
}

/// "offset(arc_theta,1%)" → ("offset", ["arc_theta", "1%"]) ;
/// les virgules dans des parenthèses imbriquées ne séparent pas : "r(a,sin(a))" → ("r", ["a", "sin(a)"])
pub fn split_call(s: &str) -> (&str, Vec<&str>) {
    let Some((name, rest)) = s.split_once('(') else { return (s.trim(), Vec::new()) };
    let inside = rest.trim_end().strip_suffix(')').unwrap_or(rest);
    let mut args = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in inside.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(&inside[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(&inside[start..]);
    (name.trim(), args.into_iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect())
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

/// Une coordonnée : "20%" (de la largeur ou de la hauteur). Pas de pixels absolus : l'image
/// doit rester la même quelle que soit la taille de l'écran (0 est accepté, = 0%).
/// y est converti : origine en bas (Didact) → origine en haut (pixels).
fn coord(s: &str, scene: &Scene, is_x: bool) -> Result<f32, String> {
    let base = if is_x { scene.width } else { scene.height };
    let v = match s.trim().strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().map_err(|_| format!("pourcentage invalide '{}'", s))? / 100.0 * base,
        None if s.trim().parse::<f32>() == Ok(0.0) => 0.0,
        None => {
            return Err(format!(
                "position '{}' : les positions sont en % de l'écran (ex : 20%), ou dans un repère : repere(x, y)",
                s
            ))
        }
    };
    Ok(if is_x { v } else { scene.height - v })
}

/// Une longueur en pixels : "10%" (de `base`), "2px", ou un nombre (px)
pub(crate) fn length(v: &PropValue, scene: &Scene, base: f32) -> Result<f32, String> {
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

/// Angle en degrés : 90, "90deg", "1.57rad" ou angle(fig)
pub(crate) fn angle(v: &PropValue, refs: &Refs) -> Result<f32, String> {
    match v {
        PropValue::Number(n) => Ok(*n as f32),
        PropValue::Single(s) if s.ends_with("rad") => s
            .trim_end_matches("rad")
            .trim()
            .parse::<f32>()
            .map(f32::to_degrees)
            .map_err(|_| format!("angle invalide '{}'", s)),
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

/// Couleur : nom (red), hexadécimal (#F00, #FF0000, #FF000080), hsl(200, 80%, 50%) ou none.
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
    if let Some(inside) = s.strip_prefix("hsl(").and_then(|r| r.strip_suffix(')')) {
        return hsl(inside).map(Some).map_err(|e| format!("couleur '{}' : {}", s, e));
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

/// `hsl(h, s%, l%)` ou `hsl(h, s%, l%, a)` : teinte en degrés, saturation et luminosité
/// en %, opacité entre 0 et 1 (ou en %)
fn hsl(inside: &str) -> Result<Rgba, String> {
    let args: Vec<&str> = inside.split(',').map(str::trim).collect();
    if !(3..=4).contains(&args.len()) {
        return Err("attendu hsl(teinte, saturation%, luminosité%) ou hsl(t, s%, l%, opacité)".into());
    }
    let num = |a: &str, pct: bool| -> Result<f64, String> {
        let (body, is_pct) = match a.strip_suffix('%') {
            Some(b) => (b.trim(), true),
            None => (a.strip_suffix("deg").unwrap_or(a).trim(), false),
        };
        let v: f64 = body.parse().map_err(|_| format!("nombre invalide '{}'", a))?;
        Ok(if is_pct || pct { v / 100.0 } else { v })
    };
    let h = num(args[0], false)?.rem_euclid(360.0);
    let (sat, l) = (num(args[1], true)?.clamp(0.0, 1.0), num(args[2], true)?.clamp(0.0, 1.0));
    let a = match args.get(3) {
        Some(a) => num(a, false)?.clamp(0.0, 1.0),
        None => 1.0,
    };
    // Conversion standard (CSS)
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        l - sat * l.min(1.0 - l) * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    let c = Rgba::rgb8((f(0.0) * 255.0).round() as u8, (f(8.0) * 255.0).round() as u8, (f(4.0) * 255.0).round() as u8);
    Ok(Rgba { a: a as f32, ..c })
}

// ─────────────────────────────────────────────
//  TESTS
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene { width: 1000.0, height: 500.0, px_scale: 1.0, background: Rgba::WHITE, orbit: (0.0, 0.0, 1.0) }
    }

    #[test]
    fn graduations_en_pi() {
        let pi = std::f64::consts::PI;
        assert_eq!(fmt_tick(pi / 2.0, pi / 2.0), "π/2");
        assert_eq!(fmt_tick(pi, pi / 2.0), "π");
        assert_eq!(fmt_tick(-3.0 * pi / 2.0, pi / 2.0), "−3π/2");
        assert_eq!(fmt_tick(2.0 * pi / 3.0, pi / 3.0), "2π/3");
        assert_eq!(fmt_tick(0.0, pi / 4.0), "0");
        assert_eq!(fmt_tick(2.0 * pi, pi), "2π");
        assert_eq!(fmt_tick(1.5, 0.5), "1.5");
        assert_eq!(fmt_tick(-2.0, 1.0), "−2");
        assert_eq!(ticks_in(-1.0, 1.0, 0.5), vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn coins_arrondis_et_ellipse() {
        // Coins droits : 4 coins ; arrondis : 4 arcs, rayon limité à la moitié du plus petit côté
        assert_eq!(rounded_rect_path(0.0, 0.0, 10.0, 4.0, 0.0).len(), 5);
        let p = rounded_rect_path(0.0, 0.0, 10.0, 4.0, 99.0);
        assert_eq!(p.iter().filter(|c| matches!(c, PathCmd::CubicTo(..))).count(), 4);
        assert_eq!(p[0], PathCmd::MoveTo(2.0, 0.0));
        let e = ellipse_path(0.0, 0.0, 3.0, 1.0);
        assert_eq!(e[0], PathCmd::MoveTo(3.0, 0.0));
        assert!(matches!(e[1], PathCmd::CubicTo(_, _, _, _, x, y) if x == 0.0 && y == 1.0));
    }

    #[test]
    fn marqueurs() {
        for m in MARKERS {
            let s = marker_shape(m, 10.0, 10.0, 2.0, Some(Rgba::BLACK));
            assert!(!s.path.is_empty(), "{}", m);
            // les croix sont des traits, les autres des surfaces
            assert_eq!(s.stroke.is_some(), matches!(*m, "cross" | "plus"), "{}", m);
        }
    }

    #[test]
    fn ancres() {
        assert_eq!(anchor_frac("center").unwrap(), (0.5, 0.5));
        assert_eq!(anchor_frac("bottomright").unwrap(), (1.0, 1.0));
        assert!(anchor_frac("milieu").is_err());
        // Carré [0,10]² : son coin haut-droit amené en (100, 50)
        let sq = Shape {
            path: vec![PathCmd::MoveTo(0.0, 0.0), PathCmd::LineTo(10.0, 0.0), PathCmd::LineTo(10.0, 10.0), PathCmd::Close],
            fill: None,
            stroke: None,
            glow: 0.0,
            image: None,
        };
        let (fx, fy) = anchor_frac("topright").unwrap();
        let out = anchor_shapes(vec![sq], 100.0, 50.0, fx, fy);
        assert_eq!(out[0].path[0], PathCmd::MoveTo(90.0, 50.0));
        assert_eq!(out[0].path[2], PathCmd::LineTo(100.0, 60.0));
    }

    #[test]
    fn couleurs_hsl() {
        let c = |s: &str| parse_color(s).unwrap().unwrap();
        assert_eq!(c("hsl(0, 100%, 50%)"), Rgba::rgb8(255, 0, 0));
        assert_eq!(c("hsl(120, 100%, 25%)"), Rgba::rgb8(0, 128, 0));
        assert_eq!(c("hsl(240deg, 100%, 50%)"), Rgba::rgb8(0, 0, 255));
        assert_eq!(c("hsl(-120, 100%, 50%)"), Rgba::rgb8(0, 0, 255));
        assert_eq!(c("hsl(0, 0%, 100%)"), Rgba::rgb8(255, 255, 255));
        assert_eq!(c("hsl(60, 100%, 50%, 0.5)").a, 0.5);
        assert_eq!(c("hsl(60, 100%, 50%, 25%)").a, 0.25);
        assert!(parse_color("hsl(1, 2)").is_err());
        assert!(parse_color("hsl(a, 50%, 50%)").is_err());
    }

    #[test]
    fn coordonnees_origine_en_bas() {
        let s = scene();
        assert_eq!(coord("0%", &s, false).unwrap(), 500.0); // bas de la scène
        assert_eq!(coord("100%", &s, false).unwrap(), 0.0); // haut
        assert_eq!(coord("25%", &s, true).unwrap(), 250.0);
        assert_eq!(coord("0", &s, true).unwrap(), 0.0);
        assert!(coord("200px", &s, true).unwrap_err().contains("en %"));
        assert!(coord("3", &s, true).is_err());
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
