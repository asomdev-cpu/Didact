//! Renderer : transforme l'état de la scène à l'instant t en dessin.
//!
//! Deux étages :
//! - `geometry` : figure Didact → formes en pixels (chemins + remplissage + trait).
//!   Indépendant de la bibliothèque de dessin.
//! - un backend (`RenderBackend`) qui dessine ces formes. Pour l'instant : `skia`
//!   (tiny-skia, dessin sur processeur dans une image en mémoire).
//!
//! À chaque frame :
//! 1. on calcule TOUTES les figures non détruites, dans l'ordre de définition :
//!    formes de base → animations appliquées → géométrie enregistrée pour les
//!    références des figures suivantes (pas de référence en avant, donc l'ordre
//!    du fichier suffit) ;
//! 2. on dessine celles qui sont visibles, dans l'ordre des layers.
//!
//! Changer de bibliothèque de dessin = écrire un autre backend, sans toucher au reste.

pub mod geometry;
pub mod skia;

pub use geometry::{Rgba, Scene, Shape};

use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use crate::runtime::animation::AnimKind;
use crate::runtime::timeline::{FigureState, Frame};
use geometry::{Geom, PathCmd, Refs};
use std::collections::HashMap;

/// Ce qu'une bibliothèque de dessin doit savoir faire
pub trait RenderBackend {
    /// Remplit toute l'image avec une couleur
    fn clear(&mut self, color: Rgba);
    /// Dessine une forme (remplissage puis trait)
    fn draw(&mut self, shape: &Shape);
}

/// Dessine une frame complète.
/// Retourne les problèmes rencontrés : (figure, raison) pour les figures visibles
/// non dessinables et les animations pas encore supportées.
pub fn render_frame(frame: &Frame, scene: &Scene, backend: &mut dyn RenderBackend) -> Vec<(String, String)> {
    let mut refs = Refs::new();
    let mut shapes_by_name: HashMap<&str, Vec<Shape>> = HashMap::new();
    let mut problems = Vec::new();

    // 1. Calcul, dans l'ordre de définition
    for st in &frame.states {
        match build_figure(st, frame.t, scene, &refs) {
            Ok((shapes, geom, warnings)) => {
                refs.insert(st.fig.name.clone(), geom);
                shapes_by_name.insert(&st.fig.name, shapes);
                if st.visible {
                    problems.extend(warnings.into_iter().map(|w| (st.fig.name.clone(), w)));
                }
            }
            Err(reason) => {
                if st.visible {
                    problems.push((st.fig.name.clone(), reason));
                }
            }
        }
    }

    // 2. Dessin, dans l'ordre des layers
    backend.clear(scene.background);
    for fig in &frame.visible {
        if let Some(shapes) = shapes_by_name.get(fig.name.as_str()) {
            for shape in shapes {
                backend.draw(shape);
            }
        }
    }
    problems
}

/// Effet cumulé des animations d'une figure à l'instant t
struct AnimEffect {
    /// Position du centre (pixels)
    center: (f32, f32),
    /// Facteur de taille (scale × grow/shrink)
    scale: f32,
    /// Rotation en degrés (sens trigonométrique)
    rotation: f32,
    opacity: f32,
}

/// Calcule les formes animées d'une figure et sa géométrie
fn build_figure(
    st: &FigureState,
    t: f64,
    scene: &Scene,
    refs: &Refs,
) -> Result<(Vec<Shape>, Geom, Vec<String>), String> {
    let mut warnings = Vec::new();

    // ── Couleur animée : on remplace `color` avant de construire les formes ──
    let fig_with_color;
    let fig: &ResolvedFigure = match animated_color(st, t)? {
        Some(color) => {
            let mut f = st.fig.clone();
            f.properties.insert("color".to_string(), PropValue::Single(color));
            fig_with_color = f;
            &fig_with_color
        }
        None => st.fig,
    };

    // ── Formes de base ──
    let mut shapes = geometry::figure_shapes(fig, scene, refs)?;
    let base_center = bbox_center(&shapes);

    // ── Autres animations ──
    let mut fx = AnimEffect { center: base_center, scale: 1.0, rotation: 0.0, opacity: 1.0 };
    let mut size_factor: f32 = 1.0; // scale(...) : facteur cible absolu
    let mut appear: f32 = 1.0; // grow / shrink
    for a in &st.anims {
        let e = a.progress(t) as f32;
        // Les fondus et grow/shrink d'avant le dernier `start` ne comptent plus
        let since_start = a.start >= st.last_start;
        match &a.kind {
            AnimKind::Move { to } => {
                let target = geometry::resolve_point(to, scene, refs)?;
                fx.center.0 += (target.0 - fx.center.0) * e;
                fx.center.1 += (target.1 - fx.center.1) * e;
            }
            AnimKind::Scale(f) => size_factor += (*f as f32 - size_factor) * e,
            AnimKind::Rotate(deg) => fx.rotation += *deg as f32 * e,
            AnimKind::FadeIn if since_start => fx.opacity = e,
            AnimKind::FadeOut if since_start => fx.opacity = 1.0 - e,
            AnimKind::Grow if since_start => appear = e,
            AnimKind::Shrink if since_start => appear = 1.0 - e,
            AnimKind::Color { .. } | AnimKind::FadeIn | AnimKind::FadeOut | AnimKind::Grow | AnimKind::Shrink => {}
            AnimKind::Unsupported(name) => {
                warnings.push(format!("animation '{}' (ligne {}) pas encore supportée, ignorée", name, a.line))
            }
        }
    }
    fx.scale = size_factor * appear;

    // ── Application : rotation et taille autour du centre de base, puis déplacement ──
    let (rad_cos, rad_sin) = {
        // y pixel vers le bas : le sens trigonométrique devient négatif
        let r = (-fx.rotation).to_radians();
        (r.cos(), r.sin())
    };
    let map = |x: f32, y: f32| {
        let (dx, dy) = ((x - base_center.0) * fx.scale, (y - base_center.1) * fx.scale);
        (
            fx.center.0 + dx * rad_cos - dy * rad_sin,
            fx.center.1 + dx * rad_sin + dy * rad_cos,
        )
    };
    let identity = fx.scale == 1.0 && fx.rotation == 0.0 && fx.center == base_center;
    if !identity || fx.opacity < 1.0 {
        for shape in &mut shapes {
            if !identity {
                for cmd in &mut shape.path {
                    *cmd = match *cmd {
                        PathCmd::MoveTo(x, y) => {
                            let (x, y) = map(x, y);
                            PathCmd::MoveTo(x, y)
                        }
                        PathCmd::LineTo(x, y) => {
                            let (x, y) = map(x, y);
                            PathCmd::LineTo(x, y)
                        }
                        PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                            let ((x1, y1), (x2, y2), (x, y)) = (map(x1, y1), map(x2, y2), map(x, y));
                            PathCmd::CubicTo(x1, y1, x2, y2, x, y)
                        }
                        PathCmd::Close => PathCmd::Close,
                    };
                }
                if let Some((_, w)) = &mut shape.stroke {
                    *w *= fx.scale;
                }
            }
            if let Some(c) = &mut shape.fill {
                c.a *= fx.opacity;
            }
            if let Some((c, _)) = &mut shape.stroke {
                c.a *= fx.opacity;
            }
        }
    }

    let geom = figure_geom(fig, &shapes, &fx);
    Ok((shapes, geom, warnings))
}

/// Couleur du trait après les animations color(...), en "#RRGGBBAA" ; None si aucune
fn animated_color(st: &FigureState, t: f64) -> Result<Option<String>, String> {
    let color_anims: Vec<_> = st.anims.iter().filter(|a| matches!(a.kind, AnimKind::Color { .. })).collect();
    if color_anims.is_empty() {
        return Ok(None);
    }
    let mut c = match st.fig.properties.get("color") {
        Some(v) => geometry::parse_color(&fmt_value(v))?.unwrap_or(Rgba { a: 0.0, ..Rgba::BLACK }),
        None => Rgba::BLACK,
    };
    for a in color_anims {
        if let AnimKind::Color { to } = &a.kind {
            let target = geometry::parse_color(to)?.unwrap_or(Rgba { a: 0.0, ..c });
            let e = a.progress(t) as f32;
            c = Rgba {
                r: c.r + (target.r - c.r) * e,
                g: c.g + (target.g - c.g) * e,
                b: c.b + (target.b - c.b) * e,
                a: c.a + (target.a - c.a) * e,
            };
        }
    }
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Ok(Some(format!("#{:02X}{:02X}{:02X}{:02X}", byte(c.r), byte(c.g), byte(c.b), byte(c.a))))
}

/// Géométrie publiée pour les références : centre, angle, boîte, point d'étiquette
fn figure_geom(fig: &ResolvedFigure, shapes: &[Shape], fx: &AnimEffect) -> Geom {
    let bbox = bbox(shapes);

    // Angle : direction pour une line (from → to), rotation sinon
    let angle = match (fig.ftype.as_str(), shapes.first().map(|s| s.path.as_slice())) {
        ("line", Some([PathCmd::MoveTo(x1, y1), PathCmd::LineTo(x2, y2), ..])) => {
            // y pixel vers le bas → on inverse dy pour rester en sens trigonométrique
            (-(y2 - y1)).atan2(x2 - x1).to_degrees()
        }
        _ => fx.rotation,
    };

    // Étiquette d'un arc : point au milieu de l'arc, direction vers l'extérieur
    // (du milieu de la corde vers le milieu de l'arc)
    let label = match (fig.ftype.as_str(), shapes.first().map(|s| s.path.as_slice())) {
        ("arc", Some(path)) if path.len() >= 3 => {
            let pt = |c: &PathCmd| match *c {
                PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) => (x, y),
                PathCmd::CubicTo(_, _, _, _, x, y) => (x, y),
                PathCmd::Close => (0.0, 0.0),
            };
            let (start, mid, end) = (pt(&path[0]), pt(&path[path.len() / 2]), pt(&path[path.len() - 1]));
            let chord = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
            let (dx, dy) = (mid.0 - chord.0, mid.1 - chord.1);
            let n = (dx * dx + dy * dy).sqrt();
            Some((mid, if n > 0.0 { (dx / n, dy / n) } else { (0.0, -1.0) }))
        }
        _ => None,
    };

    Geom { center: fx.center, angle, bbox, label }
}

/// Boîte englobante des points de contrôle : (x min, y min, x max, y max)
fn bbox(shapes: &[Shape]) -> (f32, f32, f32, f32) {
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut add = |x: f32, y: f32| {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    };
    for s in shapes {
        for cmd in &s.path {
            match *cmd {
                PathCmd::MoveTo(x, y) | PathCmd::LineTo(x, y) => add(x, y),
                PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                    add(x1, y1);
                    add(x2, y2);
                    add(x, y);
                }
                PathCmd::Close => {}
            }
        }
    }
    if b.0 > b.2 { (0.0, 0.0, 0.0, 0.0) } else { b }
}

fn bbox_center(shapes: &[Shape]) -> (f32, f32) {
    let b = bbox(shapes);
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}
