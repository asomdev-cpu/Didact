//! Validateur sémantique.
//!
//! Reçoit un `Document` déjà parsé et vérifie qu'il a du sens :
//! noms uniques, références valides (et jamais en avant), types connus,
//! timestamps uniques, cohérence des langues, et usage correct des figures
//! dans la timeline (pas d'utilisation après `destroy`, pas de `destroy`
//! sur une figure encore référencée).
//!
//! Le validateur ne s'arrête pas à la première erreur : il les collecte
//! toutes pour que l'utilisateur puisse tout corriger d'un coup.
//! Chaque erreur commence par "ligne N : ...".

use crate::ast::*;
use crate::renderer::geometry::{parse_color, split_call};
use crate::runtime::animation::{self, AnimKind};
use std::collections::HashMap;

// ─────────────────────────────────────────────
//  LISTES DE RÉFÉRENCE (tirées de SPEC.md)
// ─────────────────────────────────────────────

/// Types de figures acceptés (§8, §10, §14)
const FIGURE_TYPES: &[&str] = &[
    "line", "circle", "rect", "polygon", "arc", "star",
    "text", "equation", "latex", "table",
    "axes", "plot", "scatter", "histogram",
    "raw.latex", "raw.matplotlib",
    "group",
    "scene3d", "surface", "curve3d", "iterate", "arrow", "field",
    "cube", "box", "sphere", "cylinder", "cone", "plane", "polyhedron",
];

/// Animations acceptées (§11)
const ANIMATIONS: &[&str] = &[
    "fadein", "slidein", "grow", "write", "draw",
    "move", "follow", "rotate",
    "color", "scale", "transform",
    "fadeout", "slideout", "shrink",
    "to", "rotate3d", "fill",
];

/// Fonctions de référence dynamique dont le 1er argument est une figure (§12)
const REF_FUNCTIONS: &[&str] = &["center", "angle", "offset", "auto_tangent"];

/// Propriétés dont la valeur est du texte libre (LaTeX, maths, code) :
/// on n'y cherche pas de références. Ex : `expr : sin(x)` n'est pas un appel.
const FREE_TEXT_PROPS: &[&str] = &["type", "expr", "content", "code", "rows", "header", "color_by", "update", "init", "vars"];

// ─────────────────────────────────────────────
//  STRUCTURES INTERNES
// ─────────────────────────────────────────────

/// Ce qu'un nom désigne
enum Kind {
    Style,
    Param,
    Figure { ftype: String },
}

/// Une définition connue : sa nature, sa position dans le fichier et sa ligne
struct Def {
    kind: Kind,
    index: usize,
    line: usize,
}

/// Ce qu'une référence doit désigner
#[derive(Clone, Copy)]
enum Expected {
    Style,
    Param,
    Figure,
    Axes,
}

struct Validator {
    errors: Vec<String>,
    /// Tous les noms définis dans [figures]
    defs: HashMap<String, Def>,
    /// Pour chaque figure, les figures qu'elle référence dans sa définition
    /// (sert à vérifier `destroy`)
    figure_refs: HashMap<String, Vec<String>>,
    /// Membres de chaque groupe (groupes imbriqués compris)
    groups: HashMap<String, Vec<String>>,
    /// Noms des paramètres (`defparam`)
    params: Vec<String>,
    /// Type et contenu composé des figures de texte (pour `part="..."`)
    typeset: HashMap<String, (String, crate::renderer::typeset::Content)>,
}

// ─────────────────────────────────────────────
//  POINT D'ENTRÉE
// ─────────────────────────────────────────────

pub fn validate(doc: &Document) -> Result<(), Vec<String>> {
    let mut v = Validator {
        errors: Vec::new(),
        defs: HashMap::new(),
        figure_refs: HashMap::new(),
        groups: HashMap::new(),
        params: doc.params.iter().map(|p| p.name.clone()).collect(),
        typeset: HashMap::new(),
    };

    v.check_config(&doc.config);
    v.collect_definitions(doc);
    v.check_params(&doc.params);
    v.check_styles(&doc.styles);
    v.check_figures(&doc.figures, &doc.styles);
    v.check_timeline(doc);

    if v.errors.is_empty() {
        Ok(())
    } else {
        Err(v.errors)
    }
}

impl Validator {
    fn error(&mut self, line: usize, msg: String) {
        self.errors.push(format!("ligne {} : {}", line, msg));
    }

    /// Erreur dans la timeline : "ligne N (t=1.000) : ..."
    fn error_t(&mut self, line: usize, t: f64, msg: String) {
        self.errors.push(format!("ligne {} (t={:.3}) : {}", line, t, msg));
    }

    // ─────────────────────────────────────────────
    //  [config]
    // ─────────────────────────────────────────────

    fn check_config(&mut self, config: &Config) {
        let line_of = |key: &str| config.lines.get(key).copied().unwrap_or(1);

        if config.languages.is_empty() {
            self.error(line_of("languages"), "[config] : `languages` est vide".to_string());
        } else if !config.languages.contains(&config.default_lang) {
            self.error(
                line_of("default_lang"),
                format!(
                    "[config] : default_lang '{}' n'est pas dans languages {:?}",
                    config.default_lang, config.languages
                ),
            );
        }
    }

    // ─────────────────────────────────────────────
    //  Passe 1 : recenser tous les noms
    // ─────────────────────────────────────────────

    fn collect_definitions(&mut self, doc: &Document) {
        // Styles et figures sont dans deux listes séparées :
        // on les remet dans l'ordre du fichier pour signaler le doublon au bon endroit.
        let mut all: Vec<(usize, usize, &str, Kind)> = Vec::new();
        for s in &doc.styles {
            all.push((s.index, s.line, &s.name, Kind::Style));
        }
        for p in &doc.params {
            all.push((p.index, p.line, &p.name, Kind::Param));
        }
        for f in &doc.figures {
            let ftype = match f.properties.get("type") {
                Some(PropValue::Single(t)) => t.clone(),
                _ => String::new(),
            };
            all.push((f.index, f.line, &f.name, Kind::Figure { ftype }));
        }
        all.sort_by_key(|(index, _, _, _)| *index);

        for (index, line, name, kind) in all {
            if let Some(first) = self.defs.get(name) {
                let first_line = first.line;
                self.error(line, format!("'{}' est défini deux fois (déjà à la ligne {})", name, first_line));
            } else {
                self.defs.insert(name.to_string(), Def { kind, index, line });
            }
        }
    }

    /// Noms de paramètres utilisables dans une expression
    fn check_params(&mut self, params: &[ParamDef]) {
        for p in params {
            let mut chars = p.name.chars();
            let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                self.error(p.line, format!("paramètre '{}' : nom invalide (lettres, chiffres et _ seulement)", p.name));
            } else if crate::renderer::expr::is_reserved(&p.name) {
                self.error(p.line, format!("paramètre '{}' : ce nom est réservé dans les expressions", p.name));
            }
        }
    }

    /// Expression qui peut utiliser des paramètres : elle doit se lire, et ses paramètres
    /// doivent être définis plus haut que `from`. `x` n'est permis que si `allow_x` est vrai.
    fn check_expr(&mut self, line: usize, from: &str, from_index: usize, src: &str, allow_x: bool) {
        match crate::renderer::expr::parse_with(src, &self.params) {
            Err(e) => self.error(line, format!("'{}' : {}", from, e)),
            Ok(e) => {
                if !allow_x && e.uses_x() {
                    self.error(line, format!("'{}' : '{}' utilise x, qui n'existe que dans `expr`", from, src));
                }
                let mut used = Vec::new();
                e.vars(&mut used);
                for p in used {
                    self.check_ref(line, from, from_index, &p, Expected::Param);
                }
            }
        }
    }

    // ─────────────────────────────────────────────
    //  Références dans [figures]
    // ─────────────────────────────────────────────

    /// Vérifie que `from` (défini à la position `from_index`) peut référencer `target` :
    /// `target` existe, est défini plus haut, et est du bon genre.
    /// `line` est la ligne où la référence est écrite.
    fn check_ref(&mut self, line: usize, from: &str, from_index: usize, target: &str, expected: Expected) {
        let (index, target_line, ok_kind) = match self.defs.get(target) {
            None => {
                self.error(line, format!("'{}' référence '{}', qui n'est défini nulle part", from, target));
                return;
            }
            Some(def) => {
                let ok_kind = match (expected, &def.kind) {
                    (Expected::Style, Kind::Style) => true,
                    (Expected::Param, Kind::Param) => true,
                    (Expected::Figure, Kind::Figure { .. }) => true,
                    (Expected::Axes, Kind::Figure { ftype }) => matches!(ftype.as_str(), "axes" | "scene3d" | "surface"),
                    _ => false,
                };
                (def.index, def.line, ok_kind)
            }
        };

        if index >= from_index {
            self.error(
                line,
                format!(
                    "'{}' référence '{}', défini plus bas à la ligne {} (références en avant interdites)",
                    from, target, target_line
                ),
            );
        }
        if !ok_kind {
            let attendu = match expected {
                Expected::Style => "un style",
                Expected::Param => "un paramètre",
                Expected::Figure => "une figure",
                Expected::Axes => "un repère (axes ou scene3d) ou une surface",
            };
            self.error(line, format!("'{}' : '{}' devrait être {}", from, target, attendu));
        }
    }

    fn check_styles(&mut self, styles: &[StyleDef]) {
        for s in styles {
            if let Some(parent) = &s.extends {
                let line = s.prop_lines.get("extends").copied().unwrap_or(s.line);
                self.check_ref(line, &s.name, s.index, parent, Expected::Style);
            }
        }
    }

    fn check_figures(&mut self, figures: &[FigureDef], styles: &[StyleDef]) {
        let styles_by_name: HashMap<&str, &StyleDef> =
            styles.iter().map(|s| (s.name.as_str(), s)).collect();

        for f in figures {
            let line_of = |key: &str| f.prop_lines.get(key).copied().unwrap_or(f.line);

            // Type obligatoire et connu
            let ftype = match f.properties.get("type") {
                Some(PropValue::Single(t)) => t.as_str(),
                _ => "",
            };
            if ftype.is_empty() {
                self.error(f.line, format!("'{}' : propriété `type` manquante", f.name));
            } else if !FIGURE_TYPES.contains(&ftype) {
                self.error(line_of("type"), format!("'{}' : type inconnu '{}'", f.name, ftype));
            }

            // `width` est réservé aux rect (l'épaisseur du trait s'appelle `stroke`),
            // qu'il soit écrit sur la figure ou hérité d'un style
            if !ftype.is_empty() && ftype != "rect" {
                if f.properties.contains_key("width") {
                    self.error(
                        line_of("width"),
                        format!(
                            "'{}' : `width` est réservé aux rect ; pour l'épaisseur du trait, utiliser `stroke`",
                            f.name
                        ),
                    );
                } else if let Some(PropValue::Single(style)) = f.properties.get("style") {
                    if let Some((style_name, line)) = find_in_style_chain(style, "width", &styles_by_name) {
                        self.error(
                            line,
                            format!(
                                "'{}' ({}) hérite `width` du style '{}' ; `width` est réservé aux rect, utiliser `stroke` pour l'épaisseur du trait",
                                f.name, ftype, style_name
                            ),
                        );
                    }
                }
            }

            if ftype == "group" {
                self.check_group(f);
            }

            let mut refs = Vec::new();

            // Tri par ligne pour avoir les erreurs dans l'ordre du fichier
            let mut keys: Vec<&String> = f.properties.keys().collect();
            keys.sort_by_key(|k| line_of(k));

            for key in keys {
                // Texte et LaTeX : on vérifie qu'ils se composent (et le cache est prêt pour le rendu)
                if let Some(content) = typeset_content(ftype, key, f) {
                    if let Err(e) = crate::renderer::typeset::check(&content) {
                        self.error(line_of(key), format!("'{}' : {}", f.name, e));
                    }
                    self.typeset.insert(f.name.clone(), (ftype.to_string(), content));
                }
                // Formule d'un plot : on vérifie qu'elle se lit
                if key == "expr" && ftype == "plot" {
                    if let PropValue::Single(src) = &f.properties[key] {
                        self.check_expr(line_of(key), &f.name, f.index, src, true);
                    }
                }
                if FREE_TEXT_PROPS.contains(&key.as_str()) {
                    continue;
                }
                let line = line_of(key);
                match (key.as_str(), &f.properties[key]) {
                    ("style", PropValue::Single(s)) => {
                        self.check_ref(line, &f.name, f.index, s, Expected::Style);
                    }
                    ("axes", PropValue::Single(a)) if ftype != "scene3d" => {
                        self.check_ref(line, &f.name, f.index, a, Expected::Axes);
                        refs.push(a.clone());
                    }
                    // Scène d'un objet 3D, fonction d'une trajectoire
                    ("scene" | "on", PropValue::Single(a)) => {
                        self.check_ref(line, &f.name, f.index, a, Expected::Figure);
                        let want = if key == "scene" { &["scene3d"][..] } else { &["surface", "plot"][..] };
                        if let Some(Def { kind: Kind::Figure { ftype: t }, .. }) = self.defs.get(a.as_str()) {
                            if !want.contains(&t.as_str()) {
                                let msg = format!("'{}' : {} '{}' devrait être de type {}", f.name, key, a, want.join(" ou "));
                                self.error(line, msg);
                            }
                        }
                        refs.push(a.clone());
                    }
                    ("items", PropValue::List(items)) => {
                        for item in items {
                            self.check_ref(line, &f.name, f.index, item, Expected::Figure);
                            refs.push(item.clone());
                        }
                    }
                    // Points d'un polygone : (x%, y%) ou repere(x, y)
                    ("points", PropValue::List(points)) => {
                        for p in points.iter().filter(|p| !p.starts_with('(') && p.contains('(')) {
                            if let Some(r) = self.check_call(line, &f.name, f.index, p) {
                                refs.push(r);
                            }
                        }
                    }
                    (_, PropValue::Single(s)) if s.contains('(') => {
                        if let Some(r) = self.check_call(line, &f.name, f.index, s) {
                            refs.push(r);
                        }
                    }
                    _ => {}
                }
            }

            // 3D : formules et propriétés propres (avec celles héritées des styles)
            let merged = with_style_props(f, &styles_by_name);
            for (key, msg) in crate::renderer::three::check(ftype, &merged, &self.params) {
                self.error(line_of(&key), format!("'{}' : {}", f.name, msg));
            }

            self.figure_refs.insert(f.name.clone(), refs);
        }
    }

    /// Groupe : `items` non vide, `pos` = décalage en %
    fn check_group(&mut self, f: &FigureDef) {
        let line_of = |key: &str| f.prop_lines.get(key).copied().unwrap_or(f.line);
        match f.properties.get("items") {
            Some(PropValue::List(items)) if !items.is_empty() => {
                // membres, groupes imbriqués compris (ils sont définis plus haut)
                let mut members = Vec::new();
                for item in items {
                    if !members.contains(item) {
                        members.push(item.clone());
                    }
                    for sub in self.groups.get(item).cloned().unwrap_or_default() {
                        if !members.contains(&sub) {
                            members.push(sub);
                        }
                    }
                }
                self.groups.insert(f.name.clone(), members);
            }
            _ => self.error(
                line_of("items"),
                format!("groupe '{}' : `items` attend une liste de figures, ex : items : [a, b]", f.name),
            ),
        }
        if let Some(pos) = f.properties.get("pos") {
            let ok = matches!(pos, PropValue::Tuple(v) if v.len() == 2 && v.iter().all(|x| {
                x.trim().trim_end_matches('%').parse::<f64>().is_ok() && x.trim().ends_with('%')
            }));
            if !ok {
                self.error(
                    line_of("pos"),
                    format!("groupe '{}' : `pos` est un décalage en %, ex : pos : (10%, 0%)", f.name),
                );
            }
        }
    }

    /// Vérifie une valeur de la forme `nom(args)` :
    /// - `center(fig)`, `angle(fig)`, `offset(fig, d)`, `auto_tangent(fig)` → fig doit exister
    /// - `mes_axes(3, 4)` → mes_axes doit être une figure de type axes
    /// Retourne le nom de la figure référencée.
    fn check_call(&mut self, line: usize, from: &str, from_index: usize, value: &str) -> Option<String> {
        let (fname, args) = split_call(value);

        if REF_FUNCTIONS.contains(&fname) {
            let target = args.first().copied().unwrap_or("");
            self.check_ref(line, from, from_index, target, Expected::Figure);
            Some(target.to_string())
        } else if self.defs.contains_key(fname) {
            self.check_ref(line, from, from_index, fname, Expected::Axes);
            // Coordonnées : des nombres ou des expressions avec des paramètres
            for arg in args {
                self.check_expr(line, from, from_index, arg, false);
            }
            Some(fname.to_string())
        } else {
            self.error(
                line,
                format!(
                    "'{}' : dans '{}', '{}' n'est ni une fonction ({}) ni une figure axes",
                    from, value, fname, REF_FUNCTIONS.join(", ")
                ),
            );
            None
        }
    }

    // ─────────────────────────────────────────────
    //  [timeline]
    // ─────────────────────────────────────────────

    fn check_timeline(&mut self, doc: &Document) {
        // Timestamps uniques (comparés à la milliseconde) : ms → ligne de la 1re occurrence
        let mut seen: HashMap<i64, usize> = HashMap::new();
        for ev in &doc.timeline {
            let ms = (ev.timestamp * 1000.0).round() as i64;
            if let Some(&first_line) = seen.get(&ms) {
                self.error(
                    ev.line,
                    format!("timestamp {:.3} apparaît deux fois (déjà à la ligne {})", ev.timestamp, first_line),
                );
            } else {
                seen.insert(ms, ev.line);
            }
        }

        // On rejoue la timeline dans l'ordre chronologique
        let mut events: Vec<&TimelineEvent> = doc.timeline.iter().collect();
        events.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap());

        // figure → instant de sa destruction
        let mut destroyed: HashMap<String, f64> = HashMap::new();
        let nb_lang = doc.config.languages.len();

        for ev in events {
            let t = ev.timestamp;

            // On regroupe les commandes par type, quel que soit l'ordre d'écriture.
            // Chaque élément garde la ligne de sa commande.
            let mut destroy: Vec<(usize, &String)> = Vec::new();
            let mut end: Vec<(usize, &EndItem)> = Vec::new();
            let mut start: Vec<(usize, &StartItem)> = Vec::new();
            let mut anim: Vec<(usize, &AnimItem)> = Vec::new();

            for located in &ev.actions {
                let line = located.line;
                match &located.action {
                    TimelineAction::Destroy(names) => destroy.extend(names.iter().map(|n| (line, n))),
                    TimelineAction::End(items) => end.extend(items.iter().map(|i| (line, i))),
                    TimelineAction::Start(items) => start.extend(items.iter().map(|i| (line, i))),
                    TimelineAction::Anim(items) => anim.extend(items.iter().map(|i| (line, i))),
                    TimelineAction::Comment(texts) => {
                        if texts.len() != nb_lang {
                            self.error_t(
                                line,
                                t,
                                format!(
                                    "comment a {} traduction(s), {} attendue(s) ({:?})",
                                    texts.len(), nb_lang, doc.config.languages
                                ),
                            );
                        }
                    }
                }
            }

            // Ordre fixe : destroy → end → start → anim (cf. SPEC §6.3)

            // 1. destroy
            let mut destroyed_now: Vec<(usize, String)> = Vec::new();
            for (line, name) in destroy {
                if self.check_usable(line, t, name, &destroyed) {
                    destroyed_now.push((line, name.clone()));
                    // détruire un groupe détruit aussi ses figures
                    for m in self.groups.get(name).cloned().unwrap_or_default() {
                        if !destroyed.contains_key(&m) && !destroyed_now.iter().any(|(_, n)| *n == m) {
                            destroyed_now.push((line, m));
                        }
                    }
                }
            }
            for (_, name) in &destroyed_now {
                destroyed.insert(name.clone(), t);
            }
            // Une figure détruite ne doit plus être référencée par une figure encore en vie
            for (line, name) in &destroyed_now {
                let mut users: Vec<String> = self
                    .figure_refs
                    .iter()
                    .filter(|(user, refs)| !destroyed.contains_key(*user) && refs.contains(name))
                    .map(|(user, _)| user.clone())
                    .collect();
                users.sort();
                for user in users {
                    self.error_t(
                        *line,
                        t,
                        format!(
                            "impossible de détruire '{}', '{}' la référence encore (détruire '{}' avant ou en même temps)",
                            name, user, user
                        ),
                    );
                }
            }

            // 2. end
            for (line, item) in end {
                if item.audio {
                    continue; // fichier audio, pas une figure
                }
                self.check_usable(line, t, &item.name, &destroyed);
                if let Some(call) = &item.anim {
                    self.check_anim_call(t, &item.name, call, &destroyed);
                }
            }

            // 3. start
            for (line, item) in start {
                if let StartItem::Figure { name, anim } = item {
                    self.check_usable(line, t, name, &destroyed);
                    if let Some(call) = anim {
                        self.check_anim_call(t, name, call, &destroyed);
                    }
                }
            }

            // 4. anim
            for (line, item) in anim {
                if matches!(self.defs.get(&item.figure), Some(Def { kind: Kind::Param, .. })) {
                    for call in &item.calls {
                        self.check_param_anim(t, &item.figure, call);
                    }
                    continue;
                }
                self.check_usable(line, t, &item.figure, &destroyed);
                for call in &item.calls {
                    self.check_anim_call(t, &item.figure, call, &destroyed);
                }
            }
        }
    }

    /// color(..., part="...") : la figure est un texte et contient ce morceau
    fn check_part(&mut self, line: usize, t: f64, figure: &str, part: &str) {
        use crate::renderer::{morph, typeset, Rgba};
        let Some((ftype, content)) = self.typeset.get(figure) else {
            self.error_t(line, t, format!("'{}' : part ne s'applique qu'à un texte ou une équation", figure));
            return;
        };
        let compose = |c: &typeset::Content| typeset::shapes(c, 0.0, 0.0, 40.0, typeset::Align::Center, Rgba::BLACK);
        let found = compose(content)
            .and_then(|whole| Ok(morph::find_part(&whole, &compose(&typeset::part_content(ftype, part))?)));
        match found {
            Err(e) => self.error_t(line, t, format!("'{}' : part : {}", figure, e)),
            Ok(found) if found.is_empty() => {
                let hint = if ftype == "equation" { "" } else { " (maths entre $...$)" };
                self.error_t(line, t, format!("'{}' : part « {} » introuvable dans la figure{}", figure, part, hint));
            }
            Ok(_) => {}
        }
    }

    /// Un paramètre ne connaît qu'une animation : to(valeur, durée)
    fn check_param_anim(&mut self, t: f64, param: &str, call: &AnimCall) {
        if call.name != "to" {
            self.error_t(
                call.line,
                t,
                format!("'{}' est un paramètre : seule l'animation to(valeur, durée) s'y applique, pas '{}'", param, call.name),
            );
        } else if let Err(e) = animation::parse_call(call) {
            self.error_t(call.line, t, format!("'{}' : {}", param, e));
        }
    }

    /// Vérifie un appel d'animation : nom connu, paramètres valides (via le même code
    /// que le runtime), et figures citées dans les paramètres.
    fn check_anim_call(&mut self, t: f64, figure: &str, call: &AnimCall, destroyed: &HashMap<String, f64>) {
        let line = call.line;
        if !ANIMATIONS.contains(&call.name.as_str()) {
            self.error_t(line, t, format!("'{}' : animation inconnue '{}'", figure, call.name));
            return;
        }
        if call.name == "to" {
            self.error_t(line, t, format!("'{}' : to(...) s'applique à un paramètre (defparam), pas à une figure", figure));
            return;
        }
        match animation::parse_call(call) {
            Err(e) => self.error_t(line, t, format!("'{}' : {}", figure, e)),
            Ok((AnimKind::Fill { to }, _, _)) => {
                if let Err(e) = parse_color(&to) {
                    self.error_t(line, t, format!("'{}' : {}", figure, e));
                }
            }
            Ok((AnimKind::Color { to, part }, _, _)) => {
                if let Err(e) = parse_color(&to) {
                    self.error_t(line, t, format!("'{}' : {}", figure, e));
                }
                if let Some(part) = part {
                    self.check_part(line, t, figure, &part);
                }
            }
            Ok((AnimKind::Transform { to }, _, _)) => {
                if to == figure {
                    self.error_t(line, t, format!("'{}' : transform vers elle-même", figure));
                }
                for name in [figure, to.as_str()] {
                    if self.groups.contains_key(name) {
                        self.error_t(line, t, format!("transform : '{}' est un groupe (pas encore possible)", name));
                    }
                }
            }
            Ok(_) => {}
        }
        // Figures citées dans les paramètres : follow(path=…), transform(to=…), move(to=center(…))
        for p in &call.params {
            let Some((key, value)) = p.split_once('=') else { continue };
            let target = match (call.name.as_str(), key) {
                ("follow", "path") | ("transform", "to") => Some(value.to_string()),
                ("move", "to") if value.contains('(') && !value.starts_with('(') => {
                    let (fname, args) = split_call(value);
                    if REF_FUNCTIONS.contains(&fname) {
                        args.first().map(|a| a.to_string())
                    } else {
                        Some(fname.to_string()) // mes_axes(3, 4)
                    }
                }
                _ => None,
            };
            if let Some(target) = target {
                self.check_usable(line, t, &target, destroyed);
                // move(to=repere(a, sin(a)), ...) : coordonnées avec des paramètres
                let (fname, args) = split_call(value);
                if call.name == "move" && !REF_FUNCTIONS.contains(&fname) {
                    for arg in args {
                        if let Err(e) = crate::renderer::expr::parse_with(arg, &self.params) {
                            self.error_t(line, t, format!("'{}' : {}", figure, e));
                        }
                    }
                }
            }
        }
    }

    /// Vérifie qu'un nom utilisé dans la timeline est une figure existante et pas encore détruite.
    fn check_usable(&mut self, line: usize, t: f64, name: &str, destroyed: &HashMap<String, f64>) -> bool {
        match self.defs.get(name) {
            None => {
                self.error_t(line, t, format!("'{}' n'est pas défini", name));
                false
            }
            Some(Def { kind: Kind::Style, .. }) => {
                self.error_t(line, t, format!("'{}' est un style, pas une figure", name));
                false
            }
            Some(Def { kind: Kind::Param, .. }) => {
                self.error_t(
                    line,
                    t,
                    format!("'{}' est un paramètre, pas une figure : il s'anime avec anim : [{} to(valeur, durée)]", name, name),
                );
                false
            }
            Some(_) => match destroyed.get(name) {
                Some(&t_destroy) => {
                    self.error_t(
                        line,
                        t,
                        format!("'{}' est utilisée alors qu'elle a été détruite à t={:.3}", name, t_destroy),
                    );
                    false
                }
                None => true,
            },
        }
    }

}

/// Contenu à composer pour une propriété de texte/LaTeX, si `key` en est une
fn typeset_content(ftype: &str, key: &str, f: &FigureDef) -> Option<crate::renderer::typeset::Content> {
    use crate::renderer::typeset::Content;
    let text = |v: &PropValue| match v {
        PropValue::Single(s) | PropValue::Block(s) => s.clone(),
        other => crate::resolver::fmt_value(other),
    };
    let v = f.properties.get(key)?;
    match (ftype, key) {
        ("text", "content") => Some(Content::Text(text(v))),
        ("equation", "expr") => Some(Content::Math(text(v))),
        ("latex", "content") | ("raw.latex", "code") => Some(Content::Latex(text(v))),
        ("table", "rows") => {
            let header = match f.properties.get("header") {
                Some(PropValue::List(items)) => items.clone(),
                _ => Vec::new(),
            };
            let rows = text(v)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.split('|').map(|c| c.trim().to_string()).collect())
                .collect();
            Some(Content::Table { header, rows })
        }
        _ => None,
    }
}

/// Cherche `key` dans un style et ses ancêtres (`extends`).
/// Renvoie le style qui la définit et la ligne, ou None.
/// Propriétés d'une figure complétées par celles de ses styles (la figure l'emporte)
fn with_style_props(f: &FigureDef, styles: &HashMap<&str, &StyleDef>) -> HashMap<String, PropValue> {
    let mut props = f.properties.clone();
    let mut visited: Vec<&str> = Vec::new();
    let mut current = match f.properties.get("style") {
        Some(PropValue::Single(s)) => styles.get(s.as_str()).copied(),
        _ => None,
    };
    while let Some(style) = current {
        if visited.contains(&style.name.as_str()) {
            break;
        }
        visited.push(style.name.as_str());
        for (k, v) in &style.properties {
            props.entry(k.clone()).or_insert_with(|| v.clone());
        }
        current = style.extends.as_deref().and_then(|p| styles.get(p).copied());
    }
    props
}

fn find_in_style_chain<'a>(
    start: &str,
    key: &str,
    styles: &HashMap<&str, &'a StyleDef>,
) -> Option<(&'a str, usize)> {
    let mut visited = Vec::new(); // garde-fou si la chaîne est invalide (déjà signalé ailleurs)
    let mut current = styles.get(start).copied();
    while let Some(style) = current {
        if visited.contains(&style.name.as_str()) {
            return None;
        }
        visited.push(style.name.as_str());
        if style.properties.contains_key(key) {
            let line = style.prop_lines.get(key).copied().unwrap_or(style.line);
            return Some((style.name.as_str(), line));
        }
        current = style.extends.as_deref().and_then(|p| styles.get(p).copied());
    }
    None
}

// ─────────────────────────────────────────────
//  TESTS  (cargo test)
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    /// Parse + valide un source Didact
    fn check(src: &str) -> Result<(), Vec<String>> {
        let doc = parser::parse(lexer::tokenize(src)).expect("le parsing doit réussir");
        validate(&doc)
    }

    /// Vérifie qu'il y a une erreur contenant `fragment`
    fn assert_error(src: &str, fragment: &str) {
        match check(src) {
            Ok(()) => panic!("erreur attendue contenant '{}', mais validation OK", fragment),
            Err(errs) => assert!(
                errs.iter().any(|e| e.contains(fragment)),
                "erreur attendue contenant '{}', obtenu : {:?}",
                fragment,
                errs
            ),
        }
    }

    // Dans les tests, la 1re ligne de la chaîne est la ligne 1.

    #[test]
    fn exemple_officiel_valide() {
        assert_eq!(check(include_str!("../exemple.dct")), Ok(()));
    }

    #[test]
    fn reference_en_avant_interdite() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
                from : center(b)
            deffigure b =
                type : circle
            [timeline]",
            "ligne 4 : 'a' référence 'b', défini plus bas à la ligne 5",
        );
    }

    #[test]
    fn style_en_avant_interdit() {
        assert_error(
            "[figures]
            deffigure a =
                type  : line
                style : fin
            defstyle fin =
                width : 2px
            [timeline]",
            "ligne 4 : 'a' référence 'fin', défini plus bas",
        );
    }

    #[test]
    fn nom_defini_deux_fois() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
            deffigure a =
                type : circle
            [timeline]",
            "ligne 4 : 'a' est défini deux fois (déjà à la ligne 2)",
        );
    }

    #[test]
    fn type_inconnu() {
        assert_error(
            "[figures]
            deffigure a =
                type : triangle
            [timeline]",
            "ligne 3 : 'a' : type inconnu 'triangle'",
        );
    }

    #[test]
    fn timestamp_en_double() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            1.000 => { start : [a] }
            1.000 => { end : [a] }",
            "ligne 6 : timestamp 1.000 apparaît deux fois (déjà à la ligne 5)",
        );
    }

    #[test]
    fn casse_sensible() {
        assert_error(
            "[figures]
            deffigure ma_ligne =
                type : line
            [timeline]
            0.000 => { start : [Ma_Ligne] }",
            "ligne 5 (t=0.000) : 'Ma_Ligne' n'est pas défini",
        );
    }

    #[test]
    fn ordre_fixe_start_avant_anim() {
        // anim écrit avant start : valide grâce à l'ordre fixe
        assert_eq!(
            check(
                "[figures]
                deffigure a =
                    type : line
                [timeline]
                0.000 => {
                    anim  : [a fadein(1s)]
                    start : [a]
                }"
            ),
            Ok(())
        );
    }

    #[test]
    fn anim_apres_destroy_meme_bloc() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            0.000 => { start : [a] }
            5.000 => {
                anim    : [a fadeout(1s)]
                destroy : [a]
            }",
            "ligne 7 (t=5.000) : 'a' est utilisée alors qu'elle a été détruite à t=5.000",
        );
    }

    #[test]
    fn destroy_figure_encore_referencee() {
        assert_error(
            "[figures]
            deffigure a =
                type : circle
            deffigure b =
                type : line
                from : center(a)
            [timeline]
            0.000 => { start : [a, b] }
            5.000 => { destroy : [a] }",
            "ligne 9 (t=5.000) : impossible de détruire 'a'",
        );
    }

    #[test]
    fn destroy_ensemble_ok() {
        assert_eq!(
            check(
                "[figures]
                deffigure a =
                    type : circle
                deffigure b =
                    type : line
                    from : center(a)
                [timeline]
                0.000 => { start : [a, b] }
                5.000 => { destroy : [a, b] }"
            ),
            Ok(())
        );
    }

    #[test]
    fn mauvais_nombre_de_traductions() {
        assert_error(
            "[config]
            languages    : [FR, EN]
            default_lang : FR
            [figures]
            [timeline]
            0.000 => { comment : [\"bonjour\"] }",
            "ligne 6 (t=0.000) : comment a 1 traduction(s), 2 attendue(s)",
        );
    }

    #[test]
    fn default_lang_absente() {
        assert_error(
            "[config]
            languages    : [FR, EN]
            default_lang : NL
            [figures]
            [timeline]",
            "ligne 3 : [config] : default_lang 'NL'",
        );
    }

    #[test]
    fn animation_inconnue() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            0.000 => {
                start : [a]
                anim  : [a teleport(1s)]
            }",
            "ligne 7 (t=0.000) : 'a' : animation inconnue 'teleport'",
        );
    }

    #[test]
    fn width_interdit_hors_rect() {
        assert_error(
            "[figures]
            deffigure a =
                type  : line
                width : 2px
            [timeline]",
            "ligne 4 : 'a' : `width` est réservé aux rect",
        );
    }

    #[test]
    fn width_herite_d_un_style() {
        assert_error(
            "[figures]
            defstyle base =
                width : 2px
            defstyle epais =
                extends : base
            deffigure a =
                type  : circle
                style : epais
            [timeline]",
            "ligne 3 : 'a' (circle) hérite `width` du style 'base'",
        );
    }

    #[test]
    fn width_ok_sur_rect() {
        assert_eq!(
            check(
                "[figures]
                deffigure r =
                    type   : rect
                    width  : 30%
                    height : 20%
                    stroke : 2px
                [timeline]"
            ),
            Ok(())
        );
    }

    #[test]
    fn animations_valides() {
        assert_eq!(
            check(
                "[figures]
                deffigure a =
                    type : circle
                deffigure b =
                    type : line
                [timeline]
                0.000 => {
                    start : [a fadein(1s), b]
                    anim  : [b [move(to=center(a), 2s), color(to=#FF0000, 500ms, ease=linear)]]
                }
                3.000 => { end : [a fadeout(0.5s)] }"
            ),
            Ok(())
        );
    }

    #[test]
    fn animation_sans_duree() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            0.000 => {
                start : [a]
                anim  : [a
                    fadeout()]
            }",
            "ligne 8 (t=0.000) : 'a' : fadeout : durée manquante",
        );
    }

    #[test]
    fn animation_parametres_invalides() {
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            0.000 => { start : [a fadein(1s, ease=rapide)] }",
            "easing inconnu 'rapide'",
        );
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            0.000 => { anim : [a color(to=rouge, 1s)] }",
            "couleur inconnue 'rouge'",
        );
        assert_error(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            0.000 => { anim : [a move(to=center(fantome), 1s)] }",
            "'fantome' n'est pas défini",
        );
    }

    #[test]
    fn formule_de_plot_invalide() {
        let src = "[figures]
deffigure a =
    type   : axes
    from   : (10%, 10%)
    to     : (90%, 90%)
    xrange : [0, 10]
    yrange : [-1, 1]
deffigure c =
    type : plot
    expr : sin x + y
    axes : a
[timeline]";
        let doc = parser::parse_with_source(lexer::tokenize(src), src).unwrap();
        let errs = validate(&doc).unwrap_err();
        assert!(errs.iter().any(|e| e.starts_with("ligne 10 : 'c' : sin attend des parenthèses")), "{:?}", errs);
    }

    #[test]
    fn latex_invalide() {
        let src = "[figures]
deffigure eq =
    type : equation
    pos  : (50%, 50%)
    expr : \\frac{1}{2} + \\pasunecommande{x}
[timeline]";
        let doc = parser::parse_with_source(lexer::tokenize(src), src).unwrap();
        let errs = validate(&doc).unwrap_err();
        assert!(errs.iter().any(|e| e.starts_with("ligne 5 : 'eq' : LaTeX")), "{:?}", errs);
    }

    #[test]
    fn lignes_avec_commentaires_et_lignes_vides() {
        // Les commentaires et lignes vides doivent être comptés
        assert_error(
            "# un commentaire

[figures]
# encore un
deffigure a =
    type : hexagone",
            "ligne 6 : 'a' : type inconnu 'hexagone'",
        );
    }

    // ── Paramètres (defparam) ──

    const REPERE: &str = "deffigure r =
                type : axes
                xrange : [-5, 5]
                yrange : [-5, 5]
            ";

    #[test]
    fn parametre_valide() {
        let src = format!(
            "[figures]
            defparam k = 1
            {REPERE}
            deffigure c =
                type : plot
                axes : r
                expr : sin(k*x)
            deffigure p =
                type   : circle
                center : r(k - 1, sin(k) + 2)
                radius : 2%
            [timeline]
            0.000 => {{ start : [r, c, p] anim : [k to(3, 2s)] }}
            3.000 => {{ anim : [p move(to=r((k + 1)/2, k - 1), 1s)] }}"
        );
        assert!(crate::load(&src).is_ok(), "le fichier doit être accepté");
    }

    #[test]
    fn erreurs_de_parametres() {
        let base = format!("[figures]\ndefparam k = 1\n{REPERE}");
        // en avant
        assert_error(
            &format!("[figures]\n{REPERE}\ndeffigure c =\n type : plot\n axes : r\n expr : k*x\ndefparam k = 1\n[timeline]"),
            "défini plus bas",
        );
        // nom inconnu dans une formule
        assert_error(
            &format!("{base}\ndeffigure c =\n type : plot\n axes : r\n expr : j*x\n[timeline]"),
            "nom inconnu 'j'",
        );
        // réservé, doublon
        assert_error("[figures]\ndefparam pi = 3\n[timeline]", "réservé");
        assert_error("[figures]\ndefparam k = 1\ndefparam k = 2\n[timeline]", "défini deux fois");
        // le temps : start, anim inconnue, to sur une figure
        assert_error(&format!("{base}\n[timeline]\n0.000 => {{ start : [k] }}"), "est un paramètre");
        assert_error(&format!("{base}\n[timeline]\n0.000 => {{ anim : [k fadein(1s)] }}"), "seule l'animation to");
        assert_error(&format!("{base}\n[timeline]\n0.000 => {{ anim : [r to(2, 1s)] }}"), "s'applique à un paramètre");
        assert_error(&format!("{base}\n[timeline]\n0.000 => {{ anim : [k to(1s)] }}"), "to attend une valeur");
        // x hors d'une formule
        let e = crate::load(&format!("{base}\ndeffigure p =\n type : circle\n center : r(x, 0)\n radius : 2%\n[timeline]")).err().unwrap();
        assert!(e.iter().any(|m| m.contains("utilise x")), "{:?}", e);
    }

    // ── color(part=...) ──

    #[test]
    fn part_dans_une_figure() {
        let src = |fig: &str, part: &str| {
            format!("[figures]\n{fig}\n[timeline]\n0.000 => {{ start : [f] anim : [f color(to=red, 1s, part=\"{part}\")] }}")
        };
        let eq = "deffigure f =\n type : equation\n expr : x - \\frac{x^3}{6} + y\n pos : (50%, 50%)";
        assert!(crate::load(&src(eq, r"\frac{x^3}{6}")).is_ok());
        assert!(crate::load(&src(eq, "y")).is_ok());
        let e = crate::load(&src(eq, "z")).err().unwrap();
        assert!(e.iter().any(|m| m.contains("introuvable")), "{:?}", e);

        let txt = "deffigure f =\n type : text\n content : \"Soit $x^2$ et un mot\"\n pos : (50%, 50%)";
        assert!(crate::load(&src(txt, "mot")).is_ok());
        assert!(crate::load(&src(txt, "$x^2$")).is_ok());

        let forme = "deffigure f =\n type : circle\n center : (50%, 50%)\n radius : 5%";
        let e = crate::load(&src(forme, "z")).err().unwrap();
        assert!(e.iter().any(|m| m.contains("ne s'applique qu'à un texte")), "{:?}", e);
    }

    // ── defmodel / defrepeat ──

    #[test]
    fn modeles_et_repetitions() {
        let (doc, tl) = crate::load(
            "[figures]
defstyle epais =
    stroke : 4px
deffigure r =
    type : axes
    xrange : [0, 4]
    yrange : [0, 4]
defmodel courbe(e, c) =
    type  : plot
    axes  : r
    expr  : @e
    color : @c   # commentaire
    style : epais
deffigure a = courbe(\"x^2/4 + 1\", #2563EB)
deffigure b = courbe(sqrt(x), red)   # fin de ligne

defmodel barre(i, h) =
    type   : polygon
    points : [r(@(i*h), 0), r(@((i+1)*h), 0), r(@((i+1)*h), @(i*h)), r(@(i*h), @(i*h))]
    fill   : blue
defrepeat barres = barre(@i, 0.5) for i in 0..3   # fin de ligne
[timeline]
0.000 => { start : [r, a, b, barres write(1s)] }
1.000 => { anim : [barres_2 color(to=red, 1s)] }",
        )
        .map_err(|e| panic!("{:?}", e))
        .ok()
        .unwrap();
        let fig = |n: &str| doc.figures.iter().find(|f| f.name == n).unwrap_or_else(|| panic!("{} absente", n));
        assert_eq!(crate::resolver::fmt_value(&fig("a").properties["expr"]), "x^2/4 + 1");
        assert_eq!(crate::resolver::fmt_value(&fig("a").properties["color"]), "#2563EB");
        assert_eq!(crate::resolver::fmt_value(&fig("a").properties["stroke"]), "4px"); // style appliqué
        assert_eq!(crate::resolver::fmt_value(&fig("b").properties["expr"]), "sqrt(x)");
        assert_eq!(fig("barres").ftype, "group");
        assert_eq!(tl.members("barres"), ["barres_0", "barres_1", "barres_2", "barres_3"]);
        assert_eq!(
            crate::resolver::fmt_value(&fig("barres_2").properties["points"]),
            "[r(1,0), r(1.5,0), r(1.5,1), r(1,1)]"
        );
    }

    #[test]
    fn erreurs_de_modeles() {
        let err = |src: &str, what: &str| {
            let e = crate::load(src).err().expect("une erreur est attendue");
            assert!(e.iter().any(|m| m.contains(what)), "attendu « {} » : {:?}", what, e);
        };
        let m = "[figures]\ndefmodel c(r) =\n    type : circle\n    center : (50%, 50%)\n    radius : @r\n";
        err(&format!("{m}deffigure a = c(1%, 2%)\n[timeline]"), "ligne 6 : le modèle 'c' attend 1 argument(s)");
        err(&format!("{m}deffigure a = d(1%)\n[timeline]"), "ligne 6 : modèle inconnu 'd'");
        err(&format!("{m}defrepeat a = c(@j) for i in 0..2\n[timeline]"), "@j n'est pas un argument");
        err(&format!("{m}defrepeat a = c(1%) for i in 3..1\n[timeline]"), "intervalle");
        err(&format!("{m}defrepeat a = c(1%) pour tous\n[timeline]"), "forme attendue");
        err("[figures]\ndefmodel c(r) =\n    color : @q\ndeffigure a = c(1)\n[timeline]", "@q");
    }

    // ── 3D ──

    #[test]
    fn scene_3d_valide() {
        let src = "[figures]
defparam theta = 30
deffigure s =
    type   : scene3d
    camera : (40, theta)
    axes   : box
deffigure f =
    type     : surface
    scene    : s
    expr     : sin(x)*cos(y)
    colormap : viridis
    grid     : (10, 10)
defstyle trace =
    on    : f
    start : (1, 1)
    rate  : 0.1
deffigure chemin =
    type   : iterate
    style  : trace
    method : adam
deffigure tore =
    type   : surface
    scene  : s
    expr   : (cos(u)*(2 + cos(v)), sin(u)*(2 + cos(v)), sin(v))
    urange : [0, 6.28]
    vrange : [0, 6.28]
deffigure c =
    type   : cube
    scene  : s
    center : f(0.5, 0.5)
deffigure v =
    type : arrow
    from : s(0, 0, 0)
    to   : center(c)
deffigure lab =
    type    : equation
    expr    : \\sigma
    pos     : s(1, 1, 1)
[timeline]
0.000 => { start : [s, f draw(1s), chemin draw(2s), c, v, lab] anim : [c rotate3d(90, axis=(1,0,1), 2s), c fill(to=red, 1s), theta to(80, 3s)] }
1.000 => { anim : [f transform(to=tore, 1s)] }";
        if let Err(e) = crate::load(src) {
            panic!("{:?}", e);
        }
    }

    #[test]
    fn erreurs_3d() {
        let base = "[figures]\ndeffigure s =\n    type : scene3d\n";
        assert_error(&format!("{base}deffigure f =\n    type : surface\n    scene : s\n    expr : sin(q*x)\n[timeline]"), "nom inconnu 'q'");
        assert_error(&format!("{base}deffigure c =\n    type : cube\n    scene : t\n[timeline]"), "qui n'est défini nulle part");
        assert_error(&format!("{base}deffigure i =\n    type : iterate\n    on : s\n    start : (1, 1)\n[timeline]"), "devrait être de type surface ou plot");
        assert_error(
            &format!("{base}deffigure c =\n    type : cube\n    scene : s\n[timeline]\n0.000 => {{ anim : [c rotate3d(1s)] }}"),
            "rotate3d attend un angle",
        );
        assert_error(&format!("{base}deffigure c =\n    type : curve3d\n    scene : s\n[timeline]"), "curve3d");
    }
}
