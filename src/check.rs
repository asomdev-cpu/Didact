//! `didact <fichier> --check` : tout vérifier sans ouvrir de fenêtre, pour un humain ou une IA.
//!
//! - Erreurs : celles du parser et du validateur (le fichier ne peut pas être joué).
//! - Avertissements de mise en page, trouvés en calculant la géométrie (sans dessiner de pixels)
//!   à chaque bloc, à la fin de chaque animation et toutes les 0,25 s :
//!   - une figure qui sort du cadre ;
//!   - deux textes (text, equation, latex, table) qui se chevauchent.
//!   Ce n'est pas forcément une faute (une figure qui entre par le bord) : `allow : outside`,
//!   `allow : overlap` ou `allow : [outside, overlap]` sur la figure les fait taire.
//!
//! - Remarques (sans effet sur le code de retour) : figure, style ou paramètre défini mais jamais
//!   utilisé.
//!
//! Sortie lisible (défaut) ou JSON (`--format json`) ; code de retour : 0 rien (ou seulement des
//! remarques), 2 des avertissements, 1 des erreurs.

use crate::ast::PropValue;
use crate::renderer::{self, RenderBackend, Rgba, Scene, Shape};
use crate::resolver::{fmt_value, ResolvedDocument};
use crate::runtime::Timeline;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    /// Remarque d'hygiène : ne change pas le code de retour
    Info,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// Un problème trouvé
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Nature du problème : "parse", "validation", "outside", "overlap", "render",
    /// "unused_figure", "unused_style", "unused_param"
    pub code: &'static str,
    pub line: Option<usize>,
    pub time: Option<f64>,
    pub figure: Option<String>,
    /// Pour un chevauchement : l'autre figure
    pub other: Option<String>,
    pub message: String,
}

impl Diagnostic {
    /// À partir d'un message du parser ou du validateur : « ligne N (t=X) : 'fig' : texte »
    pub fn from_message(severity: Severity, code: &'static str, msg: &str) -> Diagnostic {
        let mut rest = msg.trim();
        let mut line = None;
        let mut time = None;
        if let Some(r) = rest.strip_prefix("ligne ") {
            let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse() {
                line = Some(n);
                rest = &r[digits.len()..];
                if let Some(r2) = rest.trim_start().strip_prefix("(t=") {
                    if let Some((t, after)) = r2.split_once(')') {
                        time = t.trim().parse().ok();
                        rest = after;
                    }
                }
                rest = rest.trim_start().trim_start_matches(':').trim_start();
            }
        }
        let mut figure = None;
        if let Some(r) = rest.strip_prefix('\'') {
            if let Some((name, after)) = r.split_once('\'') {
                if !name.is_empty() && !name.contains(' ') {
                    figure = Some(name.to_string());
                    if let Some(a) = after.trim_start().strip_prefix(':') {
                        rest = a.trim_start();
                    } else {
                        rest = msg.trim();
                        figure = None;
                    }
                }
            }
        }
        let message = if figure.is_some() { rest.to_string() } else { strip_location(msg) };
        Diagnostic { severity, code, line, time, figure, other: None, message }
    }

    /// Une ligne lisible, comme les messages habituels
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        if let Some(l) = self.line {
            s.push_str(&format!("ligne {}", l));
        }
        if let Some(t) = self.time {
            s.push_str(&format!("{}t={:.3}", if s.is_empty() { "" } else { " " }, t));
        }
        if !s.is_empty() {
            s.push_str(" : ");
        }
        if let Some(f) = &self.figure {
            s.push_str(&format!("'{}' : ", f));
        }
        s.push_str(&self.message);
        s
    }

    pub fn to_json(&self) -> String {
        let opt_str = |v: &Option<String>| v.as_ref().map(|s| json_str(s)).unwrap_or_else(|| "null".into());
        format!(
            "{{\"severity\": \"{}\", \"code\": \"{}\", \"line\": {}, \"time\": {}, \"figure\": {}, \"other\": {}, \"message\": {}}}",
            self.severity.name(),
            self.code,
            self.line.map(|l| l.to_string()).unwrap_or_else(|| "null".into()),
            self.time.map(|t| format!("{:.3}", t)).unwrap_or_else(|| "null".into()),
            opt_str(&self.figure),
            opt_str(&self.other),
            json_str(&self.message)
        )
    }
}

fn strip_location(msg: &str) -> String {
    let m = msg.trim();
    match m.strip_prefix("ligne ") {
        Some(r) => match r.split_once(" : ") {
            Some((_, rest)) => rest.to_string(),
            None => m.to_string(),
        },
        None => m.to_string(),
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Liste JSON de diagnostics
pub fn to_json(diags: &[Diagnostic]) -> String {
    if diags.is_empty() {
        return "[]".to_string();
    }
    let items: Vec<String> = diags.iter().map(|d| format!("  {}", d.to_json())).collect();
    format!("[\n{}\n]", items.join(",\n"))
}

/// Code de retour : 0 rien, 2 seulement des avertissements, 1 au moins une erreur
pub fn exit_code(diags: &[Diagnostic]) -> i32 {
    if diags.iter().any(|d| d.severity == Severity::Error) {
        1
    } else if diags.iter().any(|d| d.severity == Severity::Warning) {
        2
    } else {
        0
    }
}

// ─────────────────────────────────────────────
//  BOÎTES ENGLOBANTES
// ─────────────────────────────────────────────

/// (x min, y min, x max, y max) en pixels
type Rect = (f32, f32, f32, f32);

/// Moteur de rendu qui ne dessine rien : il note la boîte de chaque figure
#[derive(Default)]
struct BoxBackend {
    current: Option<String>,
    boxes: Vec<(String, Rect)>,
    /// Opacité la plus forte de chaque figure (en train d'apparaître ou de disparaître : < 1)
    alpha: HashMap<String, f32>,
}

fn visible_color(c: Option<Rgba>) -> bool {
    c.is_some_and(|c| c.a > 0.05)
}

fn shape_box(s: &Shape) -> Option<Rect> {
    let fill = visible_color(s.fill) || s.image.is_some();
    let stroke = s.stroke.filter(|(c, _)| c.a > 0.05).map(|(_, w)| w);
    if !fill && stroke.is_none() {
        return None;
    }
    let pad = stroke.unwrap_or(0.0) / 2.0;
    let mut b = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut add = |x: f32, y: f32| {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    };
    for c in &s.path {
        match *c {
            renderer::geometry::PathCmd::MoveTo(x, y) | renderer::geometry::PathCmd::LineTo(x, y) => add(x, y),
            renderer::geometry::PathCmd::CubicTo(a, b2, c2, d, x, y) => {
                // Les points de contrôle encadrent la courbe (un peu large, jamais trop serré)
                add(a, b2);
                add(c2, d);
                add(x, y);
            }
            renderer::geometry::PathCmd::Close => {}
        }
    }
    (b.0 <= b.2).then_some((b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad))
}

impl RenderBackend for BoxBackend {
    fn clear(&mut self, _color: Rgba) {}
    fn draw(&mut self, shape: &Shape) {
        let (Some(name), Some(b)) = (&self.current, shape_box(shape)) else { return };
        let a = shape.fill.map(|c| c.a).unwrap_or(0.0).max(shape.stroke.map(|s| s.0.a).unwrap_or(0.0));
        let e = self.alpha.entry(name.clone()).or_insert(0.0);
        *e = e.max(a);
        match self.boxes.iter_mut().find(|(n, _)| n == name) {
            Some((_, r)) => *r = (r.0.min(b.0), r.1.min(b.1), r.2.max(b.2), r.3.max(b.3)),
            None => self.boxes.push((name.clone(), b)),
        }
    }
    fn begin_figure(&mut self, name: &str) {
        self.current = Some(name.to_string());
    }
}

/// Boîtes des figures dessinées à l'instant t (pixels, origine en haut à gauche), et la taille
/// de l'image ; pour les tests
pub fn boxes_at(source: &str, t: f64) -> Result<(HashMap<String, Rect>, (u32, u32)), Vec<String>> {
    let (resolved, timeline) = crate::load(source)?;
    let (w, h) = Scene::default_size(&resolved.config);
    let scene = Scene::new(&resolved.config, w, h);
    let frame = timeline.frame_at(t, None);
    let mut backend = BoxBackend::default();
    let problems = renderer::render_frame(&frame, &scene, &mut backend);
    if !problems.is_empty() {
        return Err(problems.into_iter().map(|(n, r)| format!("{} : {}", n, r)).collect());
    }
    Ok((backend.boxes.into_iter().collect(), (w, h)))
}

/// Ce que la figure autorise : (sortir du cadre, chevaucher)
fn allowed(v: Option<&PropValue>) -> (bool, bool) {
    let words: Vec<String> = match v {
        None => return (false, false),
        Some(PropValue::List(items)) => items.iter().map(|s| s.trim().to_string()).collect(),
        Some(v) => vec![fmt_value(v).trim().to_string()],
    };
    (words.iter().any(|w| w == "outside"), words.iter().any(|w| w == "overlap"))
}

const TEXT_TYPES: &[&str] = &["text", "equation", "latex", "raw.latex", "table"];

/// Avertissements de mise en page sur toute la durée
pub fn layout(resolved: &ResolvedDocument, timeline: &Timeline, lang: Option<&str>) -> Vec<Diagnostic> {
    let (w, h) = Scene::default_size(&resolved.config);
    let scene = Scene::new(&resolved.config, w, h);
    let figs: HashMap<&str, &crate::resolver::ResolvedFigure> = resolved.figures.iter().map(|f| (f.name.as_str(), f)).collect();

    // Instants : chaque bloc, chaque fin d'animation, et toutes les 0,25 s
    let duration = timeline.duration();
    let mut times: Vec<f64> = timeline.times().to_vec();
    times.extend(timeline.anim_ends());
    // (au plus 20 000 instants : pas plus large pour une timeline de plusieurs heures)
    let step = (duration / 20_000.0).max(0.25);
    let steps = (duration / step).ceil() as usize;
    times.extend((0..=steps).map(|i| (i as f64 * step).min(duration)));
    times.retain(|t| t.is_finite() && *t >= 0.0 && *t <= duration + 1e-9);
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    // Tolérance : 2 pixels d'une image en 1080p
    let tol = 2.0 * h as f32 / 1080.0;
    // figure → (premier instant, plus grand débordement, côtés)
    let mut outside: Vec<(String, f64, f32, Vec<&'static str>)> = Vec::new();
    // (a, b) → (premier instant, dernier instant)
    let mut overlaps: Vec<((String, String), f64, f64)> = Vec::new();
    let mut problems: Vec<Diagnostic> = Vec::new();

    renderer::pixels::set_geometry_only(true);
    for &t in &times {
        let frame = timeline.frame_at(t, lang);
        let mut backend = BoxBackend::default();
        for (name, reason) in renderer::render_frame(&frame, &scene, &mut backend) {
            // Une seule fois par figure et par raison (au premier instant)
            let message = format!("ne peut pas être dessinée : {}", reason);
            if !problems.iter().any(|d| d.figure.as_deref() == Some(name.as_str()) && d.message == message) {
                let line = figs.get(name.as_str()).map(|f| f.line);
                problems.push(Diagnostic {
                    severity: Severity::Error,
                    code: "render",
                    line,
                    time: Some(t),
                    figure: Some(name),
                    other: None,
                    message,
                });
            }
        }
        for (name, b) in &backend.boxes {
            let (allow_out, _) = allowed(figs.get(name.as_str()).and_then(|f| f.properties.get("allow")));
            if allow_out {
                continue;
            }
            let sides = [(0.0 - b.0, "à gauche"), (b.2 - w as f32, "à droite"), (0.0 - b.1, "en haut"), (b.3 - h as f32, "en bas")];
            let over: Vec<(f32, &'static str)> = sides.into_iter().filter(|(d, _)| *d > tol).collect();
            if over.is_empty() {
                continue;
            }
            let worst = over.iter().map(|o| o.0).fold(0.0, f32::max);
            match outside.iter_mut().find(|o| o.0 == *name) {
                Some(o) => {
                    o.2 = o.2.max(worst);
                    for (_, s) in &over {
                        if !o.3.contains(s) {
                            o.3.push(s);
                        }
                    }
                }
                None => outside.push((name.clone(), t, worst, over.iter().map(|o| o.1).collect())),
            }
        }
        // Chevauchements entre textes pleinement affichés (un fondu enchaîné au même endroit
        // est voulu : on ignore un texte qui apparaît ou disparaît)
        let texts: Vec<&(String, Rect)> = backend
            .boxes
            .iter()
            .filter(|(n, _)| figs.get(n.as_str()).is_some_and(|f| TEXT_TYPES.contains(&f.ftype.as_str()) && !allowed(f.properties.get("allow")).1))
            .filter(|(n, _)| backend.alpha.get(n).copied().unwrap_or(1.0) >= 0.95)
            .collect();
        for i in 0..texts.len() {
            for j in (i + 1)..texts.len() {
                let (a, b) = (&texts[i].1, &texts[j].1);
                let ix = (a.2.min(b.2) - a.0.max(b.0)).max(0.0);
                let iy = (a.3.min(b.3) - a.1.max(b.1)).max(0.0);
                let inter = ix * iy;
                let smaller = ((a.2 - a.0) * (a.3 - a.1)).min((b.2 - b.0) * (b.3 - b.1)).max(1.0);
                // Plus de 10 % de la plus petite boîte : ce n'est plus un simple frôlement
                if inter / smaller <= 0.10 {
                    continue;
                }
                let key = (texts[i].0.clone(), texts[j].0.clone());
                match overlaps.iter_mut().find(|o| o.0 == key) {
                    Some(o) => o.2 = t,
                    None => overlaps.push((key, t, t)),
                }
            }
        }
    }
    renderer::pixels::set_geometry_only(false);

    let line_of = |name: &str| figs.get(name).map(|f| f.line);
    let px = |v: f32| (v * 1080.0 / h as f32).round();
    for (name, t, worst, sides) in outside {
        problems.push(Diagnostic {
            severity: Severity::Warning,
            code: "outside",
            line: line_of(&name),
            time: Some(t),
            message: format!(
                "sort du cadre ({}, jusqu'à {} px sur une image de 1080 px de haut) ; `allow : outside` si c'est voulu",
                sides.join(", "),
                px(worst)
            ),
            figure: Some(name),
            other: None,
        });
    }
    for ((a, b), t0, t1) in overlaps {
        let span = if t1 > t0 { format!(" (de t={:.3} à t={:.3})", t0, t1) } else { String::new() };
        problems.push(Diagnostic {
            severity: Severity::Warning,
            code: "overlap",
            line: line_of(&a),
            time: Some(t0),
            message: format!("chevauche '{}'{} ; `allow : overlap` si c'est voulu", b, span),
            figure: Some(a),
            other: Some(b),
        });
    }
    problems.sort_by(|x, y| x.time.partial_cmp(&y.time).unwrap_or(std::cmp::Ordering::Equal));
    problems
}

/// Vérification complète d'un source : erreurs (parser, validateur), puis mise en page
pub fn check(source: &str, lang: Option<&str>) -> Vec<Diagnostic> {
    let tokens = crate::lexer::tokenize(source);
    let doc = match crate::parser::parse_with_source(tokens, source) {
        Ok(d) => d,
        Err(e) => return vec![Diagnostic::from_message(Severity::Error, "parse", &e)],
    };
    if let Err(errors) = crate::validator::validate(&doc) {
        return errors.iter().map(|e| Diagnostic::from_message(Severity::Error, "validation", e)).collect();
    }
    let resolved = crate::resolver::resolve(&doc);
    let timeline = Timeline::new(&resolved);
    let mut diags = layout(&resolved, &timeline, lang);
    diags.extend(unused(&doc));
    diags
}

/// Les mots (noms) d'un texte ; `2k` (multiplication implicite) donne `k`
fn words(text: &str, out: &mut std::collections::HashSet<String>) {
    for w in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        let w = w.trim_start_matches(|c: char| c.is_ascii_digit());
        if !w.is_empty() {
            out.insert(w.to_string());
        }
    }
}

/// Remarques d'hygiène : figures, styles et paramètres définis mais jamais utilisés
pub fn unused(doc: &crate::ast::Document) -> Vec<Diagnostic> {
    use std::collections::HashSet;
    let info = |code, line, figure: &str, message: String| Diagnostic {
        severity: Severity::Info,
        code,
        line: Some(line),
        time: None,
        figure: Some(figure.to_string()),
        other: None,
        message,
    };
    // Mots de la timeline, et des propriétés de chaque figure et de chaque style
    let mut in_timeline = HashSet::new();
    words(&format!("{:?}", doc.timeline), &mut in_timeline);
    let props_words = |p: &HashMap<String, PropValue>| {
        let mut w = HashSet::new();
        for v in p.values() {
            words(&format!("{:?}", v), &mut w);
        }
        w
    };
    let fig_words: Vec<HashSet<String>> = doc.figures.iter().map(|f| props_words(&f.properties)).collect();
    let style_words: Vec<HashSet<String>> = doc.styles.iter().map(|s| props_words(&s.properties)).collect();

    // Figures : affichées par la timeline, utilisées par une autre figure, ou dans un groupe utilisé
    let mut used: HashSet<&str> = doc
        .figures
        .iter()
        .enumerate()
        .filter(|(i, f)| {
            in_timeline.contains(&f.name)
                || fig_words.iter().enumerate().any(|(j, w)| j != *i && w.contains(&f.name))
        })
        .map(|(_, f)| f.name.as_str())
        .collect();
    loop {
        let before = used.len();
        for (f, w) in doc.figures.iter().zip(&fig_words) {
            if used.contains(f.name.as_str()) && f.properties.get("type").map(fmt_value).as_deref() == Some("group") {
                for g in &doc.figures {
                    if w.contains(&g.name) {
                        used.insert(g.name.as_str());
                    }
                }
            }
        }
        if used.len() == before {
            break;
        }
    }
    let mut out = Vec::new();
    for f in &doc.figures {
        if !used.contains(f.name.as_str()) {
            out.push(info("unused_figure", f.line, &f.name, "figure jamais affichée ni utilisée par une autre".into()));
        }
    }
    for s in &doc.styles {
        let referenced = fig_words.iter().any(|w| w.contains(&s.name))
            || doc.styles.iter().zip(&style_words).any(|(o, w)| {
                o.name != s.name && (o.extends.as_deref() == Some(s.name.as_str()) || w.contains(&s.name))
            });
        if !referenced {
            out.push(info("unused_style", s.line, &s.name, "style jamais utilisé".into()));
        }
    }
    for p in &doc.params {
        let referenced = in_timeline.contains(&p.name)
            || fig_words.iter().chain(&style_words).any(|w| w.contains(&p.name));
        if !referenced {
            out.push(info("unused_param", p.line, &p.name, "paramètre jamais utilisé".into()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cercle_en_unites_du_repere() {
        let src = "[config]\nwindow : [1000, 1000]\n[figures]\ndefparam a = 2\n\
                   deffigure r =\n    type : axes\n    from : (0%, 0%)\n    to : (100%, 100%)\n    xrange : [-5, 5]\n    yrange : [-5, 5]\n    ticks : false\n\
                   deffigure c =\n    type : circle\n    axes : r\n    center : r(1, 0)\n    radius : 1\n\
                   deffigure e =\n    type : circle\n    axes : r\n    center : r(0, 0)\n    radius : (a, 1)\n\
                   [timeline]\n0.000 => { start : [r, c, e] }\n";
        let (b, _) = boxes_at(src, 0.0).unwrap();
        // 100 px par unité ; trait de 2 px
        let c = b["c"];
        assert!((c.2 - c.0 - 202.0).abs() < 1.0 && (c.0 - 499.0).abs() < 1.0, "{:?}", c);
        let e = b["e"];
        assert!((e.2 - e.0 - 402.0).abs() < 1.0 && (e.3 - e.1 - 202.0).abs() < 1.0, "{:?}", e);
    }

    #[test]
    fn morceaux_et_placement_relatif() {
        let src = "[config]\nwindow : [1000, 1000]\n[figures]\n\
                   deffigure e =\n    type : equation\n    expr : x^2 + \\frac{a}{b}\n    pos : (50%, 50%)\n\
                   deffigure r =\n    type : rect\n    pos : center(e, part=\"\\frac{a}{b}\")\n    width : 1%\n    height : 1%\n\
                   deffigure d =\n    type : rect\n    pos : below(e, 2%, left)\n    width : 4%\n    height : 4%\n\
                   deffigure g =\n    type : text\n    content : \"Mot\"\n    pos : right_of(e, 1%)\n\
                   [timeline]\n0.000 => { start : [e, r, d, g] }\n";
        let (b, _) = boxes_at(src, 0.0).unwrap();
        let (e, r, d, g) = (b["e"], b["r"], b["d"], b["g"]);
        let mid = |b: Rect| ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
        // La fraction est dans la moitié droite de la formule
        assert!(mid(r).0 > mid(e).0 && mid(r).0 < e.2, "{:?} {:?}", r, e);
        // Sous la formule, alignée à gauche, 20 px plus bas (trait de 2 px)
        assert!((d.1 - e.3 - 20.0).abs() < 3.0 && (d.0 - e.0).abs() < 3.0, "{:?} {:?}", d, e);
        // À droite, centrée verticalement
        assert!((g.0 - e.2 - 10.0).abs() < 4.0 && (mid(g).1 - mid(e).1).abs() < 8.0, "{:?} {:?}", g, e);
    }

    #[test]
    fn cadre_riemann_tangente_reperes_alignes() {
        let src = "[config]\nwindow : [1000, 1000]\n[figures]\ndeffunction f(x) = x\n\
                   deffigure r =\n    type : axes\n    from : (0%, 50%)\n    to : (100%, 100%)\n    xrange : [-5, 5]\n    yrange : [-2.5, 2.5]\n    ticks : false\n\
                   deffigure r2 =\n    type : axes\n    from : (30%, 0%)\n    to : (60%, 50%)\n    xrange : [0, 2]\n    yrange : [-1, 1]\n    ticks : false\n    align_x : r\n\
                   deffigure s =\n    type : riemann\n    axes : r\n    expr : f(x)\n    xrange : [0, 2]\n    n : 4\n    stroke : 0px\n\
                   deffigure t =\n    type : tangent\n    axes : r\n    expr : f(x)^2\n    at : 1\n    length : 2\n\
                   deffigure e =\n    type : equation\n    expr : a + b\n    pos : (50%, 20%)\n\
                   deffigure c =\n    type : frame\n    around : e\n    padding : 10px\n    stroke : 0px\n    fill : red\n\
                   [timeline]\n0.000 => { start : [r, r2, s, t, e, c] }\n";
        let (b, _) = boxes_at(src, 0.0).unwrap();
        // 100 px par unité ; rectangles de 0 à 2 en x, hauteur max f(1.5) = 1.5 (gauche)
        let s = b["s"];
        assert!((s.0 - 500.0).abs() < 1.0 && (s.2 - 700.0).abs() < 1.0 && (s.3 - s.1 - 150.0).abs() < 1.0, "{:?}", s);
        // Tangente à x² en 1 : pente 2, longueur 2 → largeur 2/√5 unités
        let t = b["t"];
        assert!((t.2 - t.0 - 200.0 / 5f32.sqrt()).abs() < 4.0, "{:?}", t);
        // r2 aligné sur r : x = 0 à la même colonne (500 px), mêmes unités
        let r2 = b["r2"];
        assert!((r2.0 - 500.0).abs() < 8.0 && (r2.2 - 700.0).abs() < 2.0 && (r2.3 - r2.1 - 200.0).abs() < 3.0, "{:?}", r2);
        // Cadre : la formule plus 10 px de chaque côté
        let (e, c) = (b["e"], b["c"]);
        assert!((e.0 - c.0 - 10.0).abs() < 1.5 && (c.3 - e.3 - 10.0).abs() < 1.5, "{:?} {:?}", e, c);
    }

    #[test]
    fn courbes_partielles_et_parametrees() {
        let src = |t: &str| {
            format!(
                "[config]\nwindow : [1000, 1000]\n[figures]\ndefparam T = {}\n\
                 deffigure r =\n    type : axes\n    from : (0%, 0%)\n    to : (100%, 100%)\n    xrange : [-5, 5]\n    yrange : [-5, 5]\n    ticks : false\n\
                 deffigure c =\n    type : curve\n    axes : r\n    expr : (cos(t), sin(t))\n    t : [0, T]\n\
                 deffigure p =\n    type : curve\n    axes : r\n    polar : 2\n\
                 deffigure s =\n    type : plot\n    axes : r\n    expr : 1\n    xrange : [0, T]\n\
                 [timeline]\n0.000 => {{ start : [r, c, p, s] }}\n",
                t
            )
        };
        // Demi-cercle (T = π) : de x = -1 à 1, y de 0 à 1 ; cercle polaire complet de rayon 2
        let (b, _) = boxes_at(&src("3.14159265"), 0.0).unwrap();
        let unit = 100.0; // 1000 px pour 10 unités
        let c = b["c"];
        assert!((c.2 - c.0 - 2.0 * unit).abs() < 4.0 && (c.3 - c.1 - unit).abs() < 4.0, "{:?}", c);
        let p = b["p"];
        assert!((p.2 - p.0 - 4.0 * unit).abs() < 4.0 && (p.3 - p.1 - 4.0 * unit).abs() < 4.0, "{:?}", p);
        // plot sur [0, π] seulement
        let s = b["s"];
        assert!((s.0 - 500.0).abs() < 3.0 && (s.2 - (500.0 + 3.14159 * unit)).abs() < 3.0, "{:?}", s);
        // T = 0 : rien de tracé
        let (b, _) = boxes_at(&src("0"), 0.0).unwrap();
        assert!(!b.contains_key("c") && !b.contains_key("s"));
    }

    #[test]
    fn messages_decoupes() {
        let d = Diagnostic::from_message(Severity::Error, "validation", "ligne 12 (t=3.500) : 'cercle' : figure inconnue");
        assert_eq!((d.line, d.time, d.figure.as_deref(), d.message.as_str()), (Some(12), Some(3.5), Some("cercle"), "figure inconnue"));
        let d = Diagnostic::from_message(Severity::Error, "parse", "ligne 4 : attendu Colon");
        assert_eq!((d.line, d.time, d.figure, d.message.as_str()), (Some(4), None, None, "attendu Colon"));
        let d = Diagnostic::from_message(Severity::Error, "validation", "ligne 7 : 'a b' n'est pas un nom");
        assert_eq!((d.figure.as_deref(), d.message.as_str()), (None, "'a b' n'est pas un nom"));
        assert!(d.to_json().contains("\"line\": 7") && d.to_json().contains("\"figure\": null"));
        assert_eq!(json_str("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn cadre_et_chevauchements() {
        let src = "[config]\nwindow : [16:9]\n[figures]\n\
            deffigure a =\n    type : text\n    content : \"Bonjour\"\n    pos : (50%, 50%)\n\
            deffigure b =\n    type : text\n    content : \"Bonjour\"\n    pos : (51%, 50%)\n\
            deffigure c =\n    type : circle\n    center : (98%, 50%)\n    radius : 10%\n\
            deffigure d =\n    type : circle\n    center : (2%, 50%)\n    radius : 10%\n    allow : outside\n\
            [timeline]\n0.000 => { start : [a, b, c, d] }\n1.000 => { end : [a] }\n";
        let diags = check(src, None);
        let codes: Vec<(&str, Option<&str>)> = diags.iter().map(|d| (d.code, d.figure.as_deref())).collect();
        assert!(codes.contains(&("overlap", Some("a"))), "{:?}", diags);
        assert!(codes.contains(&("outside", Some("c"))), "{:?}", diags);
        assert!(!codes.iter().any(|c| c.1 == Some("d")), "allow : outside respecté");
        let out = diags.iter().find(|d| d.code == "outside").unwrap();
        assert!(out.message.contains("à droite"), "{}", out.message);
        assert_eq!(exit_code(&diags), 2);
        // Erreur de validation : code 1, avec la ligne
        let diags = check("[figures]\n[timeline]\n0.000 => { start : [x] }\n", None);
        assert_eq!(exit_code(&diags), 1);
        assert_eq!(diags[0].line, Some(3));
    }

    #[test]
    fn remarques_hygiene() {
        let src = "[config]\nwindow : [16:9]\n[figures]\n\
            defparam k = 1\ndefparam inutile = 2\n\
            defstyle gras =\n    stroke : 4px\n\
            defstyle oublie =\n    stroke : 1px\n\
            deffigure a =\n    type : circle\n    center : (50%, 50%)\n    radius : (2k)%\n    style : gras\n\
            deffigure b =\n    type : circle\n    center : center(a)\n    radius : 5%\n\
            deffigure seule =\n    type : circle\n    center : (20%, 50%)\n    radius : 5%\n\
            deffigure g =\n    type : group\n    items : [b]\n\
            [timeline]\n0.000 => { start : [a, g] }\n";
        let diags = check(src, None);
        let infos: Vec<(&str, &str)> = diags
            .iter()
            .filter(|d| d.severity == Severity::Info)
            .map(|d| (d.code, d.figure.as_deref().unwrap()))
            .collect();
        assert_eq!(infos, [("unused_figure", "seule"), ("unused_style", "oublie"), ("unused_param", "inutile")], "{:?}", diags);
        assert_eq!(exit_code(&diags), 0, "les remarques ne changent pas le code de retour");
    }
}
