//! Structures discrètes : graphes, arbres, tableaux de valeurs (matrices).
//!
//! ```text
//! deffigure g =
//!     type    : graph
//!     nodes   : [a, b, c, d]
//!     edges   : [a-b, a-c, b->d, c-d]     # a->b : arête orientée
//!     layout  : circle                    # circle, grid, tree, force
//!     weights : [3, 1, 4, 2]              # étiquettes des arêtes (facultatif)
//!
//! deffigure m =
//!     type   : array
//!     values : [[1, 2, 3], [4, 5, 6]]
//! ```
//!
//! Chaque morceau a un nom, utilisable par `color(to=..., part=...)` :
//! un nœud (`part=b`), une arête (`part="a-b"`), une case (`part="(2,3)"`, ligne puis colonne
//! à partir de 1), une ligne ou une colonne entière (`part="row 2"`, `part="col 3"`).

use super::geometry::{self, PathCmd, Refs, Rgba, Scene, Shape};
use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;

/// Formes et, pour chaque nom de morceau, les indices de ses formes
pub type Built = (Vec<Shape>, Vec<(String, Vec<usize>)>);

fn text(f: &ResolvedFigure, key: &str) -> Option<String> {
    f.properties.get(key).map(|v| match v {
        PropValue::Single(s) | PropValue::Block(s) => s.clone(),
        v => fmt_value(v),
    })
}

/// Éléments d'une liste `[a, b, c]` écrite sur une ligne (découpée au niveau 0)
fn list(src: &str) -> Vec<String> {
    let s = src.trim();
    let inner = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(s);
    let mut out = Vec::new();
    let (mut depth, mut cur, mut math) = (0i32, String::new(), false);
    for c in inner.chars() {
        match c {
            '$' => math = !math,
            '(' | '[' | '{' if !math => depth += 1,
            ')' | ']' | '}' if !math => depth -= 1,
            ',' if depth == 0 && !math => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn prop_list(f: &ResolvedFigure, key: &str) -> Option<Vec<String>> {
    f.properties.get(key).map(|v| match v {
        PropValue::List(items) => items.iter().map(|s| s.trim().to_string()).collect(),
        other => list(&fmt_value(other)),
    })
}

/// Une arête : (origine, extrémité, orientée)
fn edge(src: &str) -> Result<(String, String, bool), String> {
    let s: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some((a, b)) = s.split_once("->") {
        return Ok((a.to_string(), b.to_string(), true));
    }
    if let Some((a, b)) = s.split_once('-') {
        return Ok((a.to_string(), b.to_string(), false));
    }
    Err(format!("arête '{}' : écrire a-b, ou a->b pour une arête orientée", src))
}

/// Nom de morceau normalisé (sans espaces, sauf « row 2 » / « col 3 »)
pub fn normalize_part(p: &str) -> String {
    let p = p.trim();
    for k in ["row", "col"] {
        if let Some(rest) = p.strip_prefix(k) {
            return format!("{} {}", k, rest.trim());
        }
    }
    p.chars().filter(|c| !c.is_whitespace()).collect()
}

// ─────────────────────────────────────────────
//  DISPOSITION D'UN GRAPHE (coordonnées dans [-1, 1]², y vers le haut)
// ─────────────────────────────────────────────

fn layout_circle(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::FRAC_PI_2 - std::f64::consts::TAU * i as f64 / n.max(1) as f64;
            (a.cos(), a.sin())
        })
        .collect()
}

fn layout_grid(n: usize) -> Vec<(f64, f64)> {
    let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
    let rows = n.div_ceil(cols).max(1);
    (0..n)
        .map(|i| {
            let (c, r) = (i % cols, i / cols);
            let x = if cols > 1 { -1.0 + 2.0 * c as f64 / (cols - 1) as f64 } else { 0.0 };
            let y = if rows > 1 { 1.0 - 2.0 * r as f64 / (rows - 1) as f64 } else { 0.0 };
            (x, y)
        })
        .collect()
}

/// Arbre : racine en haut, niveaux par parcours en largeur, feuilles régulièrement espacées,
/// chaque parent au milieu de ses enfants
fn layout_tree(n: usize, adj: &[Vec<usize>], root: usize) -> Vec<(f64, f64)> {
    let mut depth = vec![usize::MAX; n];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut order = vec![root];
    depth[root] = 0;
    let mut i = 0;
    while i < order.len() {
        let u = order[i];
        for &v in &adj[u] {
            if depth[v] == usize::MAX {
                depth[v] = depth[u] + 1;
                children[u].push(v);
                order.push(v);
            }
        }
        i += 1;
    }
    // Les nœuds non atteints : d'autres arbres, posés à droite
    for u in 0..n {
        if depth[u] == usize::MAX {
            depth[u] = 0;
            order.push(u);
        }
    }
    let mut x = vec![0.0f64; n];
    let mut next_leaf = 0.0;
    fn place(u: usize, children: &[Vec<usize>], x: &mut [f64], next: &mut f64) {
        if children[u].is_empty() {
            x[u] = *next;
            *next += 1.0;
            return;
        }
        for &c in &children[u] {
            place(c, children, x, next);
        }
        let cs = &children[u];
        x[u] = (x[cs[0]] + x[cs[cs.len() - 1]]) / 2.0;
    }
    let roots: Vec<usize> = (0..n).filter(|u| depth[*u] == 0).collect();
    for r in roots {
        place(r, &children, &mut x, &mut next_leaf);
    }
    let max_depth = depth.iter().copied().max().unwrap_or(0).max(1) as f64;
    let width = (next_leaf - 1.0).max(1.0);
    (0..n).map(|u| (-1.0 + 2.0 * x[u] / width, 1.0 - 2.0 * depth[u] as f64 / max_depth)).collect()
}

/// Ressorts (Fruchterman–Reingold), départ en cercle : toujours le même résultat
fn layout_force(n: usize, edges: &[(usize, usize)]) -> Vec<(f64, f64)> {
    let mut p = layout_circle(n);
    if n < 2 {
        return p;
    }
    let k = (4.0 / n as f64).sqrt();
    let mut temp = 0.2;
    for _ in 0..400 {
        let mut d = vec![(0.0f64, 0.0f64); n];
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let (dx, dy) = (p[i].0 - p[j].0, p[i].1 - p[j].1);
                    let dist = (dx * dx + dy * dy).sqrt().max(1e-3);
                    let f = k * k / dist;
                    d[i].0 += dx / dist * f;
                    d[i].1 += dy / dist * f;
                }
            }
        }
        for &(a, b) in edges {
            let (dx, dy) = (p[a].0 - p[b].0, p[a].1 - p[b].1);
            let dist = (dx * dx + dy * dy).sqrt().max(1e-3);
            let f = dist * dist / k;
            d[a].0 -= dx / dist * f;
            d[a].1 -= dy / dist * f;
            d[b].0 += dx / dist * f;
            d[b].1 += dy / dist * f;
        }
        for i in 0..n {
            let len = (d[i].0 * d[i].0 + d[i].1 * d[i].1).sqrt().max(1e-9);
            let step = len.min(temp);
            p[i].0 += d[i].0 / len * step;
            p[i].1 += d[i].1 / len * step;
        }
        temp *= 0.985;
    }
    normalize(p)
}

/// Ramène des positions dans [-1, 1]² (sans déformer)
fn normalize(p: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for (x, y) in &p {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    let s = ((x1 - x0).max(y1 - y0) / 2.0).max(1e-9);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    p.into_iter().map(|(x, y)| ((x - cx) / s, (y - cy) / s)).collect()
}

// ─────────────────────────────────────────────
//  GRAPHE
// ─────────────────────────────────────────────

struct Graph {
    nodes: Vec<String>,
    edges: Vec<(usize, usize, bool)>,
}

fn graph_def(f: &ResolvedFigure) -> Result<Graph, String> {
    let nodes = prop_list(f, "nodes").ok_or("graph : `nodes : [a, b, c]` manquant")?;
    if nodes.is_empty() {
        return Err("graph : `nodes` est vide".to_string());
    }
    for (i, n) in nodes.iter().enumerate() {
        if nodes[..i].contains(n) {
            return Err(format!("graph : nœud '{}' en double", n));
        }
        if n.contains('-') || n.contains(' ') {
            return Err(format!("graph : nom de nœud '{}' invalide (lettres, chiffres, _)", n));
        }
    }
    let idx = |name: &str| nodes.iter().position(|n| n == name).ok_or_else(|| format!("arête : nœud '{}' inconnu (nodes : {})", name, nodes.join(", ")));
    let mut edges = Vec::new();
    for e in prop_list(f, "edges").unwrap_or_default() {
        let (a, b, dir) = edge(&e)?;
        edges.push((idx(&a)?, idx(&b)?, dir));
    }
    Ok(Graph { nodes, edges })
}

/// Positions des nœuds dans [-1, 1]²
fn positions(f: &ResolvedFigure, g: &Graph) -> Result<Vec<(f64, f64)>, String> {
    let n = g.nodes.len();
    if let Some(ps) = prop_list(f, "positions") {
        if ps.len() != n {
            return Err(format!("positions : {} points attendus (un par nœud)", n));
        }
        let mut out = Vec::new();
        for p in ps {
            let inner = p.trim().trim_start_matches('(').trim_end_matches(')');
            let v: Vec<f64> = inner.split(',').map(|s| s.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("position invalide '{}' : (x, y)", p))?;
            if v.len() != 2 {
                return Err(format!("position invalide '{}' : (x, y)", p));
            }
            out.push((v[0], v[1]));
        }
        return Ok(normalize(out));
    }
    let mut adj = vec![Vec::new(); n];
    for &(a, b, _) in &g.edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    Ok(match text(f, "layout").as_deref() {
        None | Some("circle") => layout_circle(n),
        Some("grid") => layout_grid(n),
        Some("tree") => {
            let root = match text(f, "root") {
                Some(r) => g.nodes.iter().position(|n| *n == r).ok_or_else(|| format!("root : nœud '{}' inconnu", r))?,
                None => 0,
            };
            layout_tree(n, &adj, root)
        }
        Some("force") => layout_force(n, &g.edges.iter().map(|e| (e.0, e.1)).collect::<Vec<_>>()),
        Some(o) => return Err(format!("layout '{}' invalide (circle, grid, tree, force)", o)),
    })
}

fn size_px(f: &ResolvedFigure, key: &str, scene: &Scene, default_frac: f32) -> Result<f32, String> {
    match f.properties.get(key) {
        Some(v) => geometry::length(v, scene, scene.min_side()).map_err(|e| format!("{} : {}", key, e)),
        None => Ok(default_frac * scene.min_side()),
    }
}

fn color_prop(f: &ResolvedFigure, key: &str) -> Result<Option<Rgba>, String> {
    match f.properties.get(key) {
        None => Ok(None),
        Some(v) => geometry::parse_color(&fmt_value(v)),
    }
}

fn opacity(f: &ResolvedFigure) -> f32 {
    match f.properties.get("opacity") {
        Some(PropValue::Number(n)) => (*n as f32).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

fn graph(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<Built, String> {
    let g = graph_def(f)?;
    let pos = positions(f, &g)?;
    let (cx, cy) = geometry::point(f.properties.get("pos").ok_or("graph : `pos : (50%, 50%)` manquant (centre)")?, scene, refs)?;
    let half = size_px(f, "size", scene, 0.6)? / 2.0;
    let r = size_px(f, "radius", scene, 0.035)?;
    let op = opacity(f);
    let with = |c: Rgba| Rgba { a: c.a * op, ..c };
    let line = with(color_prop(f, "color")?.unwrap_or(Rgba::BLACK));
    let fill = with(color_prop(f, "fill")?.unwrap_or(Rgba::WHITE));
    let label_color = with(color_prop(f, "label_color")?.unwrap_or(Rgba::BLACK));
    let width = match f.properties.get("stroke") {
        Some(v) => geometry::length(v, scene, scene.min_side())?,
        None => 2.5 * scene.px_scale,
    };
    let at: Vec<(f32, f32)> = pos.iter().map(|(x, y)| (cx + *x as f32 * half, cy - *y as f32 * half)).collect();

    let mut shapes = Vec::new();
    let mut parts: Vec<(String, Vec<usize>)> = Vec::new();
    let weights = prop_list(f, "weights");
    if let Some(w) = &weights {
        if w.len() != g.edges.len() {
            return Err(format!("weights : {} étiquettes attendues (une par arête)", g.edges.len()));
        }
    }
    // Arêtes d'abord (sous les nœuds)
    for (k, &(a, b, dir)) in g.edges.iter().enumerate() {
        if a == b {
            continue;
        }
        let (pa, pb) = (at[a], at[b]);
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / len, dy / len);
        let start = (pa.0 + ux * r, pa.1 + uy * r);
        let mut end = (pb.0 - ux * r, pb.1 - uy * r);
        let mut ids = Vec::new();
        if dir {
            let head = geometry::arrow_length(width, scene);
            ids.push(shapes.len());
            shapes.push(geometry::arrow_head(end.0, end.1, ux, uy, head, line));
            end = (end.0 - ux * head * 0.8, end.1 - uy * head * 0.8);
        }
        ids.push(shapes.len());
        shapes.push(Shape { path: vec![PathCmd::MoveTo(start.0, start.1), PathCmd::LineTo(end.0, end.1)], fill: None, stroke: Some((line, width)), glow: 0.0, image: None });
        if let Some(w) = &weights {
            // Étiquette à côté du milieu, du côté gauche de l'arête
            let (mx, my) = ((pa.0 + pb.0) / 2.0 - uy * r * 0.9, (pa.1 + pb.1) / 2.0 + ux * r * 0.9);
            let ls = super::text::text_shapes(&w[k], mx, my, r * 1.0, super::text::Align::Center, label_color)?;
            ids.extend(shapes.len()..shapes.len() + ls.len());
            shapes.extend(ls);
        }
        let (na, nb) = (&g.nodes[a], &g.nodes[b]);
        parts.push((format!("{}-{}", na, nb), ids.clone()));
        if dir {
            parts.push((format!("{}->{}", na, nb), ids));
        } else {
            parts.push((format!("{}-{}", nb, na), ids));
        }
    }
    // Nœuds : disque et nom
    let labels: Option<Vec<String>> = match text(f, "labels").as_deref() {
        Some("false") => None,
        Some("true") | None => Some(g.nodes.clone()),
        Some(_) => prop_list(f, "labels"),
    };
    if let Some(l) = &labels {
        if l.len() != g.nodes.len() {
            return Err(format!("labels : {} étiquettes attendues (une par nœud), ou true / false", g.nodes.len()));
        }
    }
    for (i, name) in g.nodes.iter().enumerate() {
        let ids = vec![shapes.len()];
        shapes.push(Shape { path: geometry::circle_path(at[i].0, at[i].1, r), fill: Some(fill), stroke: Some((line, width)), glow: 0.0, image: None });
        if let Some(l) = &labels {
            shapes.extend(super::text::text_shapes(&l[i], at[i].0, at[i].1, r * 1.1, super::text::Align::Center, label_color)?);
        }
        parts.push((name.clone(), ids));
    }
    Ok((shapes, parts))
}

// ─────────────────────────────────────────────
//  TABLEAU DE VALEURS
// ─────────────────────────────────────────────

fn array_values(f: &ResolvedFigure) -> Result<Vec<Vec<String>>, String> {
    let src = text(f, "values").ok_or("array : `values : [[1, 2], [3, 4]]` manquant")?;
    let rows = list(&src);
    if rows.is_empty() {
        return Err("array : `values` est vide".to_string());
    }
    let rows: Vec<Vec<String>> = if rows.iter().all(|r| r.starts_with('[')) {
        rows.iter().map(|r| list(r)).collect()
    } else {
        vec![rows] // une seule ligne : [1, 2, 3]
    };
    Ok(rows)
}

fn array(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<Built, String> {
    let rows = array_values(f)?;
    let nr = rows.len();
    let nc = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    let (cx, cy) = geometry::point(f.properties.get("pos").ok_or("array : `pos : (50%, 50%)` manquant (centre)")?, scene, refs)?;
    let cell = size_px(f, "cell", scene, 0.09)?;
    let op = opacity(f);
    let with = |c: Rgba| Rgba { a: c.a * op, ..c };
    let txt = with(color_prop(f, "color")?.unwrap_or(Rgba::BLACK));
    let grid = !matches!(text(f, "grid").as_deref(), Some("false"));
    let brackets = matches!(text(f, "brackets").as_deref(), Some("true"));
    let edge = with(color_prop(f, "grid_color")?.unwrap_or(Rgba { r: 0.6, g: 0.6, b: 0.6, a: 1.0 }));
    // Fond des cases : transparent par défaut (color(part=...) le fait apparaître)
    let back = match color_prop(f, "fill")? {
        Some(c) => with(c),
        None => Rgba { a: 0.0, ..Rgba::WHITE },
    };
    let font = match f.properties.get("font") {
        Some(v) => geometry::length(v, scene, scene.min_side())?,
        None => cell * 0.42,
    };
    let (w, h) = (nc as f32 * cell, nr as f32 * cell);
    let (x0, y0) = (cx - w / 2.0, cy - h / 2.0);
    let mut shapes = Vec::new();
    let mut parts = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            let (x, y) = (x0 + j as f32 * cell, y0 + i as f32 * cell);
            let mut ids = vec![shapes.len()];
            shapes.push(Shape {
                path: vec![PathCmd::MoveTo(x, y), PathCmd::LineTo(x + cell, y), PathCmd::LineTo(x + cell, y + cell), PathCmd::LineTo(x, y + cell), PathCmd::Close],
                fill: Some(back),
                stroke: if grid { Some((edge, 1.5 * scene.px_scale)) } else { None },
                glow: 0.0,
                image: None,
            });
            if !v.is_empty() {
                let ts = super::text::text_shapes(v, x + cell / 2.0, y + cell / 2.0, font, super::text::Align::Center, txt)?;
                // Le texte de la case n'est pas recoloré : seul le fond l'est
                let _ = &mut ids;
                shapes.extend(ts);
            }
            parts.push((format!("({},{})", i + 1, j + 1), ids));
        }
    }
    for i in 0..nr {
        let ids: Vec<usize> = parts.iter().filter(|(k, _)| k.starts_with(&format!("({},", i + 1))).flat_map(|(_, v)| v.clone()).collect();
        parts.push((format!("row {}", i + 1), ids));
    }
    for j in 0..nc {
        let ids: Vec<usize> = parts.iter().filter(|(k, _)| k.ends_with(&format!(",{})", j + 1)) && k.starts_with('(')).flat_map(|(_, v)| v.clone()).collect();
        parts.push((format!("col {}", j + 1), ids));
    }
    if brackets {
        // Crochets de matrice : [ à gauche, ] à droite
        let t = cell * 0.18;
        let m = cell * 0.08;
        let bw = 2.5 * scene.px_scale;
        for (x, d) in [(x0 - m, 1.0f32), (x0 + w + m, -1.0)] {
            shapes.push(Shape {
                path: vec![PathCmd::MoveTo(x + d * t, y0 - m), PathCmd::LineTo(x, y0 - m), PathCmd::LineTo(x, y0 + h + m), PathCmd::LineTo(x + d * t, y0 + h + m)],
                fill: None,
                stroke: Some((txt, bw)),
                glow: 0.0,
                image: None,
            });
        }
    }
    Ok((shapes, parts))
}

/// Formes d'un `graph` ou d'un `array` (calcul gardé d'une image à l'autre)
pub fn build(f: &ResolvedFigure, scene: &Scene, refs: &Refs) -> Result<Built, String> {
    let extra = format!("{} {} {}", scene.width, scene.height, scene.px_scale);
    // Position par une référence (center(fig)) : la géométrie de la cible compte aussi
    let pos_ref = f.properties.get("pos").map(fmt_value).unwrap_or_default();
    let target = pos_ref.split(['(', ')']).nth(1).and_then(|n| refs.get(n.trim())).map(|g| format!("{:?}", g.center)).unwrap_or_default();
    let key = super::memo::key(&f.properties, &HashMap::new(), &(extra + &target));
    super::memo::get_or(&format!("2d:{}", f.name), key, || match f.ftype.as_str() {
        "graph" => graph(f, scene, refs),
        _ => array(f, scene, refs),
    })
}

/// Indices des formes du morceau `part`
pub fn part_indices(built: &Built, part: &str) -> Option<Vec<usize>> {
    let p = normalize_part(part);
    built.1.iter().find(|(k, _)| *k == p).map(|(_, v)| v.clone())
}

/// Noms des morceaux d'une figure (validateur), ou une erreur de définition
pub fn part_names(f: &ResolvedFigure) -> Result<Vec<String>, String> {
    match f.ftype.as_str() {
        "graph" => {
            let g = graph_def(f)?;
            let mut v = g.nodes.clone();
            for &(a, b, dir) in &g.edges {
                let (na, nb) = (&g.nodes[a], &g.nodes[b]);
                v.push(format!("{}-{}", na, nb));
                v.push(if dir { format!("{}->{}", na, nb) } else { format!("{}-{}", nb, na) });
            }
            Ok(v)
        }
        _ => {
            let rows = array_values(f)?;
            let mut v = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                v.push(format!("row {}", i + 1));
                for j in 0..r.len() {
                    v.push(format!("({},{})", i + 1, j + 1));
                }
            }
            let nc = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            v.extend((1..=nc).map(|j| format!("col {}", j)));
            Ok(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fig(ftype: &str, props: &[(&str, &str)]) -> ResolvedFigure {
        let mut f = ResolvedFigure {
            name: "g".into(),
            index: 0,
            ftype: ftype.into(),
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
    fn listes_et_aretes() {
        assert_eq!(list("[a, b, (1, 2), $x, y$]"), vec!["a", "b", "(1, 2)", "$x, y$"]);
        assert_eq!(edge("a -> b").unwrap(), ("a".into(), "b".into(), true));
        assert_eq!(edge("a-b").unwrap(), ("a".into(), "b".into(), false));
        assert!(edge("ab").is_err());
        assert_eq!(normalize_part(" ( 2 , 3 ) "), "(2,3)");
        assert_eq!(normalize_part("row  2"), "row 2");
    }

    #[test]
    fn arbre() {
        // a → b, c ; b → d, e : a au-dessus et au milieu de b et c
        let f = fig("graph", &[("nodes", "[a, b, c, d, e]"), ("edges", "[a-b, a-c, b-d, b-e]"), ("layout", "tree")]);
        let g = graph_def(&f).unwrap();
        let p = positions(&f, &g).unwrap();
        assert!(p[0].1 > p[1].1 && p[1].1 > p[3].1);
        assert!((p[0].0 - (p[1].0 + p[2].0) / 2.0).abs() < 1e-9);
        assert!((p[1].0 - (p[3].0 + p[4].0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn forces_dans_le_carre() {
        let f = fig("graph", &[("nodes", "[a, b, c, d, e, f]"), ("edges", "[a-b, b-c, c-a, d-e, e-f, f-d, a-d]"), ("layout", "force")]);
        let g = graph_def(&f).unwrap();
        let p = positions(&f, &g).unwrap();
        assert!(p.iter().all(|(x, y)| x.abs() <= 1.0 + 1e-9 && y.abs() <= 1.0 + 1e-9));
        // Pas deux nœuds au même endroit
        for i in 0..p.len() {
            for j in 0..i {
                assert!((p[i].0 - p[j].0).hypot(p[i].1 - p[j].1) > 0.1);
            }
        }
    }

    #[test]
    fn morceaux() {
        let g = fig("graph", &[("nodes", "[a, b, c]"), ("edges", "[a-b, b->c]")]);
        let names = part_names(&g).unwrap();
        for p in ["a", "a-b", "b-a", "b->c", "b-c"] {
            assert!(names.contains(&p.to_string()), "{}", p);
        }
        assert!(!names.contains(&"c-b".to_string()));
        let m = fig("array", &[("values", "[[1, 2, 3], [4, 5, 6]]")]);
        let names = part_names(&m).unwrap();
        for p in ["(1,1)", "(2,3)", "row 2", "col 3"] {
            assert!(names.contains(&p.to_string()), "{}", p);
        }
        assert!(graph_def(&fig("graph", &[("nodes", "[a, b]"), ("edges", "[a-z]")])).err().unwrap().contains("'z' inconnu"));
    }
}
