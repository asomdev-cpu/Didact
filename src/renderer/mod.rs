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
//!    Un groupe applique ensuite son décalage et ses animations à ses figures ;
//! 2. `transform(to=b)` : les formes en cours de déformation sont remplacées ;
//! 3. on dessine celles qui sont visibles, dans l'ordre des layers.
//!
//! Changer de bibliothèque de dessin = écrire un autre backend, sans toucher au reste.

pub mod expr;
pub mod geometry;
pub mod morph;
pub mod reveal;
pub mod skia;
pub mod text;
pub mod three;
pub mod typeset;

pub use geometry::{Rgba, Scene, Shape};

use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use crate::runtime::animation::AnimKind;
use crate::runtime::timeline::{FigureState, Frame};
use geometry::{AxesMap, Geom, PathCmd, Refs};
use std::collections::HashMap;

/// Ce qu'une bibliothèque de dessin doit savoir faire
pub trait RenderBackend {
    /// Remplit toute l'image avec une couleur
    fn clear(&mut self, color: Rgba);
    /// Dessine une forme (remplissage puis trait)
    fn draw(&mut self, shape: &Shape);
    /// Ajoute le halo lumineux (de ce rayon, en pixels) de ces formes, déjà dessinées nettes.
    /// Par défaut : pas de halo.
    fn draw_glow(&mut self, _shapes: &[Shape], _radius: f32) {}
}

/// Dessine des formes nettes, dans l'ordre ; garde de côté celles qui ont un halo
fn draw_shapes<'a>(backend: &mut dyn RenderBackend, shapes: &'a [Shape], glowing: &mut Vec<&'a Shape>) {
    for s in shapes {
        backend.draw(s);
        if s.glow > 0.0 {
            glowing.push(s);
        }
    }
}

/// Halos de toute l'image, ajoutés à la fin (comme le « bloom » d'un moteur 3D) :
/// un seul flou par rayon, quel que soit le nombre de formes
fn draw_glows(backend: &mut dyn RenderBackend, glowing: &[&Shape]) {
    let mut radii: Vec<f32> = glowing.iter().map(|s| s.glow).collect();
    radii.sort_by(f32::total_cmp);
    radii.dedup();
    for r in radii {
        let batch: Vec<Shape> = glowing.iter().filter(|s| s.glow == r).map(|s| (*s).clone()).collect();
        backend.draw_glow(&batch, r);
    }
}

/// Dessine une frame complète.
/// Retourne les problèmes rencontrés : (figure, raison) pour les figures visibles
/// non dessinables.
pub fn render_frame(frame: &Frame, scene: &Scene, backend: &mut dyn RenderBackend) -> Vec<(String, String)> {
    let mut refs = Refs::with_params(frame.params.clone());
    let mut shapes_by_name: HashMap<String, Vec<Shape>> = HashMap::new();
    // Objets 3D (morceaux en coordonnées de leur scène), et repère de chaque scène
    let mut objs: HashMap<String, three::Obj> = HashMap::new();
    let mut problems = Vec::new();
    let visible: HashMap<&str, bool> = frame.states.iter().map(|s| (s.fig.name.as_str(), s.visible)).collect();

    // 1. Calcul, dans l'ordre de définition (un groupe est défini après ses figures)
    for st in &frame.states {
        let name = st.fig.name.clone();
        let result = if st.fig.ftype == "group" {
            build_group(st, frame.t, scene, &mut refs, &mut shapes_by_name, &visible, &mut objs)
        } else if st.fig.ftype == "scene3d" {
            build_scene3d(st, frame.t, scene, &mut refs, &mut objs)
        } else if three::is_3d(st.fig, &refs) {
            build_object3d(st, frame.t, scene, &mut refs, &mut objs)
        } else {
            build_figure(st, frame.t, scene, &refs).map(|(shapes, geom)| {
                refs.insert(name.clone(), geom);
                shapes_by_name.insert(name.clone(), shapes);
                if st.fig.ftype == "plot" {
                    three::register_plot(st.fig, &mut refs);
                }
            })
        };
        if let Err(reason) = result {
            if st.visible {
                problems.push((st.fig.name.clone(), reason));
            }
        }
    }

    // 2. transform(to=b) : les formes de la figure se déforment vers celles de b
    //    (b est calculée même si elle n'est pas encore affichée)
    for st in &frame.states {
        if !st.visible {
            continue;
        }
        for a in &st.anims {
            if let AnimKind::Transform { to } = &a.kind {
                if a.start < st.last_start {
                    continue;
                }
                let e = a.progress(frame.t) as f32;
                // Entre deux surfaces : la surface se déforme point par point
                let target_state = frame.states.iter().find(|s| s.fig.name == *to);
                if let (Some(src), Some(dst)) = (objs.get(&st.fig.name), target_state) {
                    if st.fig.ftype == "surface" && dst.fig.ftype == "surface" {
                        if let Some(space) = refs.spaces.get(&src.scene) {
                            let morphed = three::morph_surfaces(st.fig, dst.fig, e as f64, space, scene, &refs).and_then(|mut items| {
                                let c0 = three::Obj { items: items.clone(), ..Default::default() }.center();
                                three::effect(st, frame.t, c0, three::span_of(space), &refs)?.apply(&mut items);
                                Ok(items)
                            });
                            match morphed {
                                Ok(items) => {
                                    if let Some(o) = objs.get_mut(&st.fig.name) {
                                        o.items = items;
                                    }
                                }
                                Err(reason) => problems.push((st.fig.name.clone(), reason)),
                            }
                            continue;
                        }
                    }
                }
                // Entre objets 3D : les morceaux glissent (ou fondu enchaîné)
                if let (Some(src), Some(dst)) = (objs.get(&st.fig.name), objs.get(to)) {
                    let items = three::morph(&src.items, &dst.items, e);
                    if let Some(o) = objs.get_mut(&st.fig.name) {
                        o.items = items;
                    }
                    continue;
                }
                let target = shapes_by_name.get(to).cloned().unwrap_or_default();
                let typeset = |name: &str| {
                    frame.states.iter().any(|s| s.fig.name == name && is_typeset(&s.fig.ftype))
                };
                let glyphs = typeset(&st.fig.name) && typeset(to);
                if let Some(shapes) = shapes_by_name.get_mut(&st.fig.name) {
                    *shapes = if glyphs { morph::morph_glyphs(shapes, &target, e) } else { morph::morph(shapes, &target, e) };
                }
            }
        }
    }

    // 3. Chaque scène 3D : morceaux de ses objets visibles (et son repère si elle l'est),
    //    projetés et triés du plus loin au plus proche
    let mut composed: HashMap<String, Vec<Shape>> = HashMap::new();
    for (scene_name, space) in &refs.spaces {
        // Dans l'ordre du fichier : à profondeur égale, l'ordre de dessin est toujours le même
        let mut items: Vec<&three::Item> = Vec::new();
        for st in &frame.states {
            let Some(obj) = objs.get(&st.fig.name) else { continue };
            if obj.scene == *scene_name && st.visible {
                items.extend(obj.items.iter());
            }
        }
        composed.insert(scene_name.clone(), three::compose(space, &items));
    }

    // 4. Dessin, dans l'ordre des layers. Une scène 3D est dessinée une seule fois,
    //    à la place de la première de ses figures (elle-même ou un de ses objets)
    backend.clear(scene.background);
    let mut drawn3d: Vec<Vec<Shape>> = Vec::new();
    for fig in &frame.visible {
        let scene3d = objs.get(&fig.name).map(|o| o.scene.clone());
        if let Some(shapes) = scene3d.and_then(|s| composed.remove(&s)) {
            drawn3d.push(shapes);
        }
    }
    let mut glowing: Vec<&Shape> = Vec::new();
    let mut next3d = drawn3d.iter();
    let mut seen_scenes: Vec<String> = Vec::new();
    for fig in &frame.visible {
        match objs.get(&fig.name).map(|o| o.scene.clone()) {
            Some(sc) => {
                if !seen_scenes.contains(&sc) {
                    seen_scenes.push(sc);
                    if let Some(shapes) = next3d.next() {
                        draw_shapes(backend, shapes, &mut glowing);
                    }
                }
            }
            None => {
                if let Some(shapes) = shapes_by_name.get(&fig.name) {
                    draw_shapes(backend, shapes, &mut glowing);
                }
            }
        }
    }
    draw_glows(backend, &glowing);
    problems
}

/// Scène 3D : caméra à l'instant t, repère (avec les animations de la scène : fondu, dessin)
fn build_scene3d(
    st: &FigureState,
    t: f64,
    scene: &Scene,
    refs: &mut Refs,
    objs: &mut HashMap<String, three::Obj>,
) -> Result<(), String> {
    let fig = with_animated_colors(st, t)?;
    let fig = fig.as_ref().unwrap_or(st.fig);
    let (space, mut axes) = three::build_space(fig, scene, refs)?;
    let fx = three::effect(st, t, axes.center(), three::span_of(&space), refs)?;
    // La scène ne bouge pas : seuls le fondu et le dessin progressif s'appliquent à son repère
    let only_look = three::Effect3::look_only(&fx);
    only_look.apply(&mut axes.items);
    let geom = three::screen_geom(&space, &axes, axes.center());
    refs.insert(st.fig.name.clone(), geom);
    refs.spaces.insert(st.fig.name.clone(), space);
    objs.insert(st.fig.name.clone(), axes);
    Ok(())
}

/// Objet 3D : morceaux, animations, et ce qu'il publie pour les références
fn build_object3d(
    st: &FigureState,
    t: f64,
    scene: &Scene,
    refs: &mut Refs,
    objs: &mut HashMap<String, three::Obj>,
) -> Result<(), String> {
    let fig = with_animated_colors(st, t)?;
    let fig = fig.as_ref().unwrap_or(st.fig);
    let scene_name = three::scene_of(fig, refs).ok_or("objet 3D sans scène : ajouter `scene : nom_de_la_scene`")?;
    let space = refs.spaces.get(&scene_name).cloned().ok_or_else(|| format!("la scène '{}' n'a pas pu être calculée", scene_name))?;
    let (mut obj, surface) = three::build_object(fig, &space, &scene_name, scene, refs)?;
    let c0 = obj.center();
    let fx = three::effect(st, t, c0, three::span_of(&space), refs)?;
    fx.apply(&mut obj.items);
    obj.path = obj.path.iter().map(|p| fx.map(*p)).collect();
    let center = fx.map(c0);

    let name = st.fig.name.clone();
    if let Some(s) = surface {
        refs.surfaces.insert(name.clone(), s);
    }
    if obj.path.len() >= 2 {
        refs.paths3.insert(name.clone(), obj.path.clone());
    }
    refs.centers3.insert(name.clone(), center);
    refs.scene_of.insert(name.clone(), scene_name);
    refs.insert(name.clone(), three::screen_geom(&space, &obj, center));
    objs.insert(name, obj);
    Ok(())
}

/// Comme `render_frame`, mais le dessin est réparti sur tous les cœurs du processeur
/// (même image au pixel près)
pub fn render_frame_parallel(frame: &Frame, scene: &Scene, backend: &mut skia::SkiaBackend) -> Vec<(String, String)> {
    let mut rec = skia::Recorder::default();
    let problems = render_frame(frame, scene, &mut rec);
    backend.replay(&rec.ops);
    problems
}

/// Figures composées par le moteur de texte : leurs formes sont des lettres
pub fn is_typeset(ftype: &str) -> bool {
    matches!(ftype, "text" | "equation" | "latex" | "raw.latex" | "table")
}

/// Effet cumulé des animations d'une figure (ou d'un groupe) à l'instant t
struct AnimEffect {
    /// Position du centre (pixels)
    center: (f32, f32),
    /// Facteur de taille (scale × grow/shrink)
    scale: f32,
    /// Rotation en degrés (sens trigonométrique)
    rotation: f32,
    opacity: f32,
    /// Orientation imposée par follow(..., rotate=tangent)
    follow_rotation: Option<f32>,
    /// draw / write en cours : (mode, avancement)
    reveal: Option<(reveal::Mode, f32)>,
}

/// Distance d'un slidein / slideout, en part de la hauteur de la scène
const SLIDE_DISTANCE: f32 = 0.12;

/// Calcule l'effet des animations commencées, à partir du centre et de la rotation de base
fn compute_effect(
    st: &FigureState,
    t: f64,
    base_center: (f32, f32),
    base_rotation: f32,
    scene: &Scene,
    refs: &Refs,
) -> Result<AnimEffect, String> {
    let mut fx = AnimEffect {
        center: base_center,
        scale: 1.0,
        rotation: base_rotation,
        opacity: 1.0,
        follow_rotation: None,
        reveal: None,
    };
    let mut size_factor: f32 = 1.0; // scale(...) : facteur cible absolu
    let mut appear: f32 = 1.0; // grow / shrink
    let mut slide = (0.0f32, 0.0f32); // décalage temporaire de slidein / slideout
    let slide_len = scene.height as f32 * SLIDE_DISTANCE;
    for a in &st.anims {
        let e = a.progress(t) as f32;
        // Les apparitions / disparitions d'avant le dernier `start` ne comptent plus
        if a.kind.is_entry_or_exit() && a.start < st.last_start {
            continue;
        }
        match &a.kind {
            AnimKind::Move { to } => {
                let target = geometry::resolve_point(to, scene, refs)?;
                fx.center.0 += (target.0 - fx.center.0) * e;
                fx.center.1 += (target.1 - fx.center.1) * e;
            }
            AnimKind::MoveBy { dir, distance } => {
                let (ux, uy) = dir.unit();
                let d = *distance as f32 / 100.0;
                fx.center.0 += ux * d * scene.width as f32 * e;
                fx.center.1 += uy * d * scene.height as f32 * e;
            }
            AnimKind::Scale(f) => size_factor += (*f as f32 - size_factor) * e,
            AnimKind::Rotate(deg) => fx.rotation += *deg as f32 * e,
            AnimKind::Follow { path, tangent } => {
                let outline = &geometry::resolve_geom(path, refs)?.outline;
                let (point, dir) = point_along(outline, e as f64)
                    .ok_or_else(|| format!("follow : la figure '{}' n'a pas de contour à suivre", path))?;
                fx.center = point;
                if *tangent {
                    fx.follow_rotation = Some(direction_angle(dir));
                }
            }
            AnimKind::FadeIn => fx.opacity = e,
            AnimKind::FadeOut => fx.opacity = 1.0 - e,
            AnimKind::Grow => appear = e,
            AnimKind::Shrink => appear = 1.0 - e,
            AnimKind::SlideIn { from } => {
                let (ux, uy) = from.unit();
                slide = (ux * slide_len * (1.0 - e), uy * slide_len * (1.0 - e));
                fx.opacity = e;
            }
            AnimKind::SlideOut { to } => {
                let (ux, uy) = to.unit();
                slide = (ux * slide_len * e, uy * slide_len * e);
                fx.opacity = 1.0 - e;
            }
            AnimKind::Draw => fx.reveal = Some((reveal::Mode::Draw, e)),
            AnimKind::Write => fx.reveal = Some((reveal::Mode::Write, e)),
            // rotate3d autour de z : une rotation dans le plan de l'écran
            AnimKind::Rotate3d { axis, angle } if axis[0] == 0.0 && axis[1] == 0.0 => {
                fx.rotation += (*angle * axis[2].signum()) as f32 * e
            }
            AnimKind::Color { .. }
            | AnimKind::Fill { .. }
            | AnimKind::Transform { .. }
            | AnimKind::To(_)
            | AnimKind::Rotate3d { .. } => {}
        }
    }
    fx.scale = size_factor * appear;
    fx.center = (fx.center.0 + slide.0, fx.center.1 + slide.1);
    if let Some(r) = fx.follow_rotation {
        fx.rotation = r;
    }
    Ok(fx)
}

/// Transformation d'un point : rotation et taille autour de `base_center`, puis déplacement vers fx.center
fn mapper(base_center: (f32, f32), fx: &AnimEffect) -> impl Fn(f32, f32) -> (f32, f32) {
    // y pixel vers le bas : le sens trigonométrique devient négatif
    let r = (-fx.rotation).to_radians();
    let (c, s) = (r.cos(), r.sin());
    let (center, scale) = (fx.center, fx.scale);
    move |x, y| {
        let (dx, dy) = ((x - base_center.0) * scale, (y - base_center.1) * scale);
        (center.0 + dx * c - dy * s, center.1 + dx * s + dy * c)
    }
}

/// Applique l'effet (géométrie et opacité) à des formes
fn apply_effect(shapes: &mut [Shape], base_center: (f32, f32), fx: &AnimEffect) {
    let identity = fx.scale == 1.0 && fx.rotation == 0.0 && fx.center == base_center;
    if identity && fx.opacity >= 1.0 {
        return;
    }
    let map = mapper(base_center, fx);
    for shape in shapes {
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

/// Épaisseur du trait temporaire de draw / write
fn reveal_line(scene: &Scene) -> f32 {
    (1.2 * scene.px_scale).max(0.8)
}

/// Calcule les formes animées d'une figure et sa géométrie
fn build_figure(st: &FigureState, t: f64, scene: &Scene, refs: &Refs) -> Result<(Vec<Shape>, Geom), String> {
    // ── Couleur animée : on remplace `color` avant de construire les formes ──
    let fig_with_color = with_animated_colors(st, t)?;
    let fig: &ResolvedFigure = fig_with_color.as_ref().unwrap_or(st.fig);

    // ── Formes de base ──
    let mut shapes = geometry::figure_shapes(fig, scene, refs)?;
    color_parts(st, t, &mut shapes)?;
    let base_center = bbox_center(&shapes);

    // ── Animations ──
    // Rotation fixe écrite sur la figure : rotate : 30
    let base_rotation = match fig.properties.get("rotate") {
        Some(PropValue::Number(n)) => *n as f32,
        _ => 0.0,
    };
    let mut fx = compute_effect(st, t, base_center, base_rotation, scene, refs)?;

    // Orientation selon une courbe : rotate : auto_tangent(courbe)
    if fx.follow_rotation.is_none() {
        if let Some(PropValue::Single(v)) = fig.properties.get("rotate") {
            if let Some(name) = v.strip_prefix("auto_tangent(").and_then(|r| r.strip_suffix(')')) {
                let outline = &geometry::resolve_geom(name.trim(), refs)?.outline;
                if let Some(dir) = tangent_near(outline, fx.center) {
                    fx.rotation = direction_angle(dir);
                }
            }
        }
    }

    apply_effect(&mut shapes, base_center, &fx);

    // Repère : la conversion suit les déplacements et la taille (pas la rotation)
    let axes = if fig.ftype == "axes" {
        let m = geometry::axes_map(fig, scene, refs)?;
        let (ox, oy) = mapper(base_center, &fx)(m.ox, m.oy);
        Some(AxesMap { ox, oy, ux: m.ux * fx.scale, uy: m.uy * fx.scale, ..m })
    } else {
        None
    };

    // La géométrie publiée est celle de la figure entière, même pendant draw / write
    let geom = figure_geom(fig, &shapes, &fx, axes);
    if let Some((mode, e)) = fx.reveal {
        let n = shapes.len();
        shapes = reveal::reveal(shapes, mode, e, 0, n, reveal_line(scene));
    }
    // Halo : glow : 10px
    if let Some(v) = fig.properties.get("glow") {
        let g = geometry::length(v, scene, scene.width.min(scene.height)).map_err(|e| format!("glow : {}", e))?;
        shapes.iter_mut().for_each(|s| s.glow = g);
    }
    Ok((shapes, geom))
}

/// Groupe : ses figures sont déjà calculées (elles sont définies avant lui).
/// Son décalage `pos` et ses animations leur sont appliqués comme à un seul objet,
/// autour du centre de l'ensemble ; leurs géométries (références) sont mises à jour.
fn build_group(
    st: &FigureState,
    t: f64,
    scene: &Scene,
    refs: &mut Refs,
    shapes_by_name: &mut HashMap<String, Vec<Shape>>,
    visible: &HashMap<&str, bool>,
    objs: &mut HashMap<String, three::Obj>,
) -> Result<(), String> {
    // Membres 3D : l'effet du groupe s'applique en 3D, autour du centre de l'ensemble
    let members3: Vec<&String> = st.members.iter().filter(|m| objs.contains_key(*m) && refs.centers3.contains_key(*m)).collect();
    if let Some(first) = members3.first() {
        let scene_name = objs[*first].scene.clone();
        if let Some(space) = refs.spaces.get(&scene_name).cloned() {
            let all = three::Obj {
                scene: scene_name.clone(),
                items: members3.iter().flat_map(|m| objs[*m].items.clone()).collect(),
                path: Vec::new(),
            };
            let c0 = all.center();
            let fx = three::effect(st, t, c0, three::span_of(&space), refs)?;
            for m in &members3 {
                if let Some(o) = objs.get_mut(*m) {
                    fx.apply(&mut o.items);
                    o.path = o.path.iter().map(|p| fx.map(*p)).collect();
                    let c = fx.map(refs.centers3[*m]);
                    refs.centers3.insert((*m).clone(), c);
                    if o.path.len() >= 2 {
                        refs.paths3.insert((*m).clone(), o.path.clone());
                    }
                    refs.insert((*m).clone(), three::screen_geom(&space, o, c));
                }
            }
            refs.centers3.insert(st.fig.name.clone(), fx.map(c0));
            refs.scene_of.insert(st.fig.name.clone(), scene_name);
        }
    }

    let members: Vec<&String> = st.members.iter().filter(|m| shapes_by_name.contains_key(*m)).collect();
    if members.is_empty() {
        shapes_by_name.insert(st.fig.name.clone(), Vec::new());
        if !refs.contains_key(&st.fig.name) {
            if let Some(c) = refs.centers3.get(&st.fig.name).copied() {
                if let Some(sp) = refs.scene_of.get(&st.fig.name).and_then(|s| refs.spaces.get(s)) {
                    let g = three::screen_geom(sp, &three::Obj::default(), c);
                    refs.insert(st.fig.name.clone(), g);
                }
            }
        }
        return Ok(());
    }

    // Centre de l'ensemble : figures affichées (toutes si aucune ne l'est)
    let shown: Vec<&String> = members.iter().copied().filter(|m| visible.get(m.as_str()) == Some(&true)).collect();
    let basis = if shown.is_empty() { &members } else { &shown };
    let all: Vec<Shape> = basis.iter().flat_map(|m| shapes_by_name[*m].clone()).collect();
    let base_center = bbox_center(&all);

    // pos : décalage (x en % de la largeur, y en % de la hauteur, vers le haut)
    let offset = match st.fig.properties.get("pos") {
        Some(PropValue::Tuple(v)) if v.len() == 2 => {
            let pct = |s: &str| {
                s.trim()
                    .trim_end_matches('%')
                    .parse::<f32>()
                    .map_err(|_| format!("pos d'un groupe : décalage en %, ex : (10%, 0%), pas '{}'", s))
            };
            (pct(&v[0])? / 100.0 * scene.width as f32, -pct(&v[1])? / 100.0 * scene.height as f32)
        }
        Some(other) => {
            return Err(format!("pos d'un groupe : décalage (x%, y%) attendu, pas '{}'", fmt_value(other)))
        }
        None => (0.0, 0.0),
    };
    let start_center = (base_center.0 + offset.0, base_center.1 + offset.1);
    let fx = compute_effect(st, t, start_center, 0.0, scene, refs)?;

    let total: usize = members.iter().map(|m| shapes_by_name[*m].len()).sum();
    let mut first = 0;
    let map = mapper(base_center, &fx);
    for m in &members {
        let shapes = shapes_by_name.get_mut(*m).unwrap();
        apply_effect(shapes, base_center, &fx);
        // Références : la figure a bougé avec le groupe
        if let Some(g) = refs.get_mut(*m) {
            g.center = map(g.center.0, g.center.1);
            g.angle += fx.rotation;
            g.bbox = bbox(shapes);
            g.outline = outline(shapes);
            if let Some(ax) = &mut g.axes {
                let (ox, oy) = map(ax.ox, ax.oy);
                *ax = AxesMap { ox, oy, ux: ax.ux * fx.scale, uy: ax.uy * fx.scale, ..ax.clone() };
            }
            if let Some((p, d)) = g.label {
                g.label = Some((map(p.0, p.1), d));
            }
        }
        let n = shapes.len();
        if let Some((mode, e)) = fx.reveal {
            let taken = std::mem::take(shapes);
            *shapes = reveal::reveal(taken, mode, e, first, total, reveal_line(scene));
        }
        first += n;
    }

    let shapes_now: Vec<Shape> = members.iter().flat_map(|m| shapes_by_name[*m].clone()).collect();
    refs.insert(
        st.fig.name.clone(),
        Geom {
            center: fx.center,
            angle: fx.rotation,
            bbox: bbox(&shapes_now),
            label: None,
            axes: None,
            outline: Vec::new(),
        },
    );
    shapes_by_name.insert(st.fig.name.clone(), Vec::new());
    Ok(())
}

/// La figure avec `color` et `fill` remplacés par leurs valeurs animées (color(...), fill(...)) ;
/// None si aucune animation de couleur
pub(crate) fn with_animated_colors(st: &FigureState, t: f64) -> Result<Option<ResolvedFigure>, String> {
    let color = animated_color(st, t, "color")?;
    let fill = animated_color(st, t, "fill")?;
    if color.is_none() && fill.is_none() {
        return Ok(None);
    }
    let mut f = st.fig.clone();
    if let Some(c) = color {
        f.properties.insert("color".to_string(), PropValue::Single(c));
    }
    if let Some(c) = fill {
        f.properties.insert("fill".to_string(), PropValue::Single(c));
    }
    Ok(Some(f))
}

/// Couleur `key` (color : le trait, fill : le remplissage) après ses animations, en "#RRGGBBAA" ; None si aucune
fn animated_color(st: &FigureState, t: f64, key: &str) -> Result<Option<String>, String> {
    let target_of = |kind: &AnimKind| match (kind, key) {
        (AnimKind::Color { to, part: None }, "color") | (AnimKind::Fill { to }, "fill") => Some(to.clone()),
        _ => None,
    };
    let color_anims: Vec<_> = st.anims.iter().filter(|a| target_of(&a.kind).is_some()).collect();
    if color_anims.is_empty() {
        return Ok(None);
    }
    // Sans valeur de départ : noir pour le trait, transparent pour le remplissage
    let fallback = if key == "color" { Rgba::BLACK } else { Rgba { a: 0.0, ..Rgba::BLACK } };
    let mut c = match st.fig.properties.get(key) {
        Some(v) => geometry::parse_color(&fmt_value(v))?.unwrap_or(Rgba { a: 0.0, ..Rgba::BLACK }),
        None => fallback,
    };
    for a in color_anims {
        if let Some(to) = target_of(&a.kind) {
            let target = geometry::parse_color(&to)?.unwrap_or(Rgba { a: 0.0, ..c });
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

/// color(to=..., part="...") : recolore les morceaux d'un texte ou d'une équation
fn color_parts(st: &FigureState, t: f64, shapes: &mut [Shape]) -> Result<(), String> {
    for a in &st.anims {
        let AnimKind::Color { to, part: Some(part) } = &a.kind else { continue };
        let target = geometry::parse_color(to)?.ok_or_else(|| format!("color : couleur '{}' invalide pour part", to))?;
        let content = typeset::part_content(&st.fig.ftype, part);
        let needle = typeset::shapes(&content, 0.0, 0.0, 40.0, typeset::Align::Center, Rgba::BLACK)?;
        let e = a.progress(t) as f32;
        let mix = |c: Rgba| Rgba {
            r: c.r + (target.r - c.r) * e,
            g: c.g + (target.g - c.g) * e,
            b: c.b + (target.b - c.b) * e,
            a: c.a + (target.a - c.a) * e,
        };
        for i in morph::find_part(shapes, &needle) {
            if let Some(c) = &mut shapes[i].fill {
                *c = mix(*c);
            }
            if let Some((c, _)) = &mut shapes[i].stroke {
                *c = mix(*c);
            }
        }
    }
    Ok(())
}

/// Géométrie publiée pour les références : centre, angle, boîte, point d'étiquette
fn figure_geom(fig: &ResolvedFigure, shapes: &[Shape], fx: &AnimEffect, axes: Option<AxesMap>) -> Geom {
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

    Geom { center: fx.center, angle, bbox, label, axes, outline: outline(shapes) }
}

/// Contour de la première forme, en points (les courbes sont découpées en segments)
fn outline(shapes: &[Shape]) -> Vec<(f32, f32)> {
    let mut pts: Vec<(f32, f32)> = Vec::new();
    let Some(shape) = shapes.first() else { return pts };
    let mut start = (0.0, 0.0);
    for cmd in &shape.path {
        match *cmd {
            PathCmd::MoveTo(x, y) => {
                start = (x, y);
                pts.push((x, y));
            }
            PathCmd::LineTo(x, y) => pts.push((x, y)),
            PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                let p0 = *pts.last().unwrap_or(&(x, y));
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    pts.push((
                        u * u * u * p0.0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x,
                        u * u * u * p0.1 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y,
                    ));
                }
            }
            PathCmd::Close => pts.push(start),
        }
    }
    pts
}

/// Point situé à la fraction `e` (0 → 1) de la longueur d'un contour, et direction à cet endroit
fn point_along(pts: &[(f32, f32)], e: f64) -> Option<((f32, f32), (f32, f32))> {
    if pts.len() < 2 {
        return None;
    }
    let seg_len = |i: usize| {
        let (a, b) = (pts[i], pts[i + 1]);
        ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
    };
    let total: f32 = (0..pts.len() - 1).map(seg_len).sum();
    let mut target = total * (e.clamp(0.0, 1.0) as f32);
    for i in 0..pts.len() - 1 {
        let l = seg_len(i);
        if l > 0.0 && (target <= l || i == pts.len() - 2) {
            let t = (target / l).min(1.0);
            let (a, b) = (pts[i], pts[i + 1]);
            return Some(((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), (b.0 - a.0, b.1 - a.1)));
        }
        target -= l;
    }
    None
}

/// Direction du contour au point le plus proche de `p`
fn tangent_near(pts: &[(f32, f32)], p: (f32, f32)) -> Option<(f32, f32)> {
    let mut best: Option<(f32, (f32, f32))> = None;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l2 = dx * dx + dy * dy;
        if l2 == 0.0 {
            continue;
        }
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
        let (qx, qy) = (a.0 + dx * t, a.1 + dy * t);
        let d = (qx - p.0).powi(2) + (qy - p.1).powi(2);
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, (dx, dy)));
        }
    }
    best.map(|(_, dir)| dir)
}

/// Direction en pixels → angle en degrés, sens trigonométrique (y pixel vers le bas)
fn direction_angle((dx, dy): (f32, f32)) -> f32 {
    (-dy).atan2(dx).to_degrees()
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
