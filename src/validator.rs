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
use crate::renderer::geometry::parse_color;
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
];

/// Animations acceptées (§11)
const ANIMATIONS: &[&str] = &[
    "fadein", "slidein", "grow", "write", "draw",
    "move", "follow", "rotate",
    "color", "scale", "transform",
    "fadeout", "slideout", "shrink",
];

/// Fonctions de référence dynamique dont le 1er argument est une figure (§12)
const REF_FUNCTIONS: &[&str] = &["center", "angle", "offset", "auto_tangent"];

/// Propriétés dont la valeur est du texte libre (LaTeX, maths, code) :
/// on n'y cherche pas de références. Ex : `expr : sin(x)` n'est pas un appel.
const FREE_TEXT_PROPS: &[&str] = &["type", "expr", "content", "code", "rows", "header"];

// ─────────────────────────────────────────────
//  STRUCTURES INTERNES
// ─────────────────────────────────────────────

/// Ce qu'un nom désigne
enum Kind {
    Style,
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
}

// ─────────────────────────────────────────────
//  POINT D'ENTRÉE
// ─────────────────────────────────────────────

pub fn validate(doc: &Document) -> Result<(), Vec<String>> {
    let mut v = Validator {
        errors: Vec::new(),
        defs: HashMap::new(),
        figure_refs: HashMap::new(),
    };

    v.check_config(&doc.config);
    v.collect_definitions(doc);
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
                    (Expected::Figure, Kind::Figure { .. }) => true,
                    (Expected::Axes, Kind::Figure { ftype }) => ftype == "axes",
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
                Expected::Figure => "une figure",
                Expected::Axes => "une figure de type axes",
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

            let mut refs = Vec::new();

            // Tri par ligne pour avoir les erreurs dans l'ordre du fichier
            let mut keys: Vec<&String> = f.properties.keys().collect();
            keys.sort_by_key(|k| line_of(k));

            for key in keys {
                if FREE_TEXT_PROPS.contains(&key.as_str()) {
                    continue;
                }
                let line = line_of(key);
                match (key.as_str(), &f.properties[key]) {
                    ("style", PropValue::Single(s)) => {
                        self.check_ref(line, &f.name, f.index, s, Expected::Style);
                    }
                    ("axes", PropValue::Single(a)) => {
                        self.check_ref(line, &f.name, f.index, a, Expected::Axes);
                        refs.push(a.clone());
                    }
                    ("items", PropValue::List(items)) => {
                        for item in items {
                            self.check_ref(line, &f.name, f.index, item, Expected::Figure);
                            refs.push(item.clone());
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

            self.figure_refs.insert(f.name.clone(), refs);
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
                self.check_usable(line, t, &item.figure, &destroyed);
                for call in &item.calls {
                    self.check_anim_call(t, &item.figure, call, &destroyed);
                }
            }
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
        match animation::parse_call(call) {
            Err(e) => self.error_t(line, t, format!("'{}' : {}", figure, e)),
            Ok((AnimKind::Color { to }, _, _)) => {
                if let Err(e) = parse_color(&to) {
                    self.error_t(line, t, format!("'{}' : {}", figure, e));
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

/// Cherche `key` dans un style et ses ancêtres (`extends`).
/// Renvoie le style qui la définit et la ligne, ou None.
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

/// Découpe "offset(arc_theta,1%)" en ("offset", ["arc_theta", "1%"])
fn split_call(value: &str) -> (&str, Vec<&str>) {
    match value.split_once('(') {
        Some((name, rest)) => {
            let inside = rest.trim_end_matches(')');
            let args = inside
                .split(',')
                .map(|a| a.trim())
                .filter(|a| !a.is_empty())
                .collect();
            (name.trim(), args)
        }
        None => (value.trim(), Vec::new()),
    }
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
}
