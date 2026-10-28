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

use crate::ast::*;
use std::collections::{HashMap, HashSet};

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

/// Une définition connue : sa nature et sa position dans le fichier
struct Def {
    kind: Kind,
    index: usize,
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
    v.check_figures(&doc.figures);
    v.check_timeline(doc);

    if v.errors.is_empty() {
        Ok(())
    } else {
        Err(v.errors)
    }
}

impl Validator {
    fn error(&mut self, msg: String) {
        self.errors.push(msg);
    }

    // ─────────────────────────────────────────────
    //  [config]
    // ─────────────────────────────────────────────

    fn check_config(&mut self, config: &Config) {
        if config.languages.is_empty() {
            self.error("[config] : `languages` est vide".to_string());
        } else if !config.languages.contains(&config.default_lang) {
            self.error(format!(
                "[config] : default_lang '{}' n'est pas dans languages {:?}",
                config.default_lang, config.languages
            ));
        }
    }

    // ─────────────────────────────────────────────
    //  Passe 1 : recenser tous les noms
    // ─────────────────────────────────────────────

    fn collect_definitions(&mut self, doc: &Document) {
        // Styles et figures sont dans deux listes séparées :
        // on les remet dans l'ordre du fichier pour signaler le doublon au bon endroit.
        let mut all: Vec<(usize, &str, Kind)> = Vec::new();
        for s in &doc.styles {
            all.push((s.index, &s.name, Kind::Style));
        }
        for f in &doc.figures {
            let ftype = match f.properties.get("type") {
                Some(PropValue::Single(t)) => t.clone(),
                _ => String::new(),
            };
            all.push((f.index, &f.name, Kind::Figure { ftype }));
        }
        all.sort_by_key(|(index, _, _)| *index);

        for (index, name, kind) in all {
            if self.defs.contains_key(name) {
                self.error(format!("'{}' est défini deux fois", name));
            } else {
                self.defs.insert(name.to_string(), Def { kind, index });
            }
        }
    }

    // ─────────────────────────────────────────────
    //  Références dans [figures]
    // ─────────────────────────────────────────────

    /// Vérifie que `from` (défini à la position `from_index`) peut référencer `target` :
    /// `target` existe, est défini plus haut, et est du bon genre.
    fn check_ref(&mut self, from: &str, from_index: usize, target: &str, expected: Expected) {
        let (index, ok_kind) = match self.defs.get(target) {
            None => {
                self.error(format!("'{}' référence '{}', qui n'est défini nulle part", from, target));
                return;
            }
            Some(def) => {
                let ok_kind = match (expected, &def.kind) {
                    (Expected::Style, Kind::Style) => true,
                    (Expected::Figure, Kind::Figure { .. }) => true,
                    (Expected::Axes, Kind::Figure { ftype }) => ftype == "axes",
                    _ => false,
                };
                (def.index, ok_kind)
            }
        };

        if index >= from_index {
            self.error(format!(
                "'{}' référence '{}', défini plus bas dans le fichier (références en avant interdites)",
                from, target
            ));
        }
        if !ok_kind {
            let attendu = match expected {
                Expected::Style => "un style",
                Expected::Figure => "une figure",
                Expected::Axes => "une figure de type axes",
            };
            self.error(format!("'{}' : '{}' devrait être {}", from, target, attendu));
        }
    }

    fn check_styles(&mut self, styles: &[StyleDef]) {
        for s in styles {
            if let Some(parent) = &s.extends {
                self.check_ref(&s.name, s.index, parent, Expected::Style);
            }
        }
    }

    fn check_figures(&mut self, figures: &[FigureDef]) {
        for f in figures {
            // Type obligatoire et connu
            match f.properties.get("type") {
                Some(PropValue::Single(t)) if FIGURE_TYPES.contains(&t.as_str()) => {}
                Some(PropValue::Single(t)) => {
                    self.error(format!("'{}' : type inconnu '{}'", f.name, t));
                }
                _ => self.error(format!("'{}' : propriété `type` manquante", f.name)),
            }

            let mut refs = Vec::new();

            // Tri des clés pour avoir des erreurs dans un ordre stable
            let mut keys: Vec<&String> = f.properties.keys().collect();
            keys.sort();

            for key in keys {
                if FREE_TEXT_PROPS.contains(&key.as_str()) {
                    continue;
                }
                match (key.as_str(), &f.properties[key]) {
                    ("style", PropValue::Single(s)) => {
                        self.check_ref(&f.name, f.index, s, Expected::Style);
                    }
                    ("axes", PropValue::Single(a)) => {
                        self.check_ref(&f.name, f.index, a, Expected::Axes);
                        refs.push(a.clone());
                    }
                    ("items", PropValue::List(items)) => {
                        for item in items {
                            self.check_ref(&f.name, f.index, item, Expected::Figure);
                            refs.push(item.clone());
                        }
                    }
                    (_, PropValue::Single(s)) if s.contains('(') => {
                        if let Some(r) = self.check_call(&f.name, f.index, s) {
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
    fn check_call(&mut self, from: &str, from_index: usize, value: &str) -> Option<String> {
        let (fname, args) = split_call(value);

        if REF_FUNCTIONS.contains(&fname) {
            let target = args.first().copied().unwrap_or("");
            self.check_ref(from, from_index, target, Expected::Figure);
            Some(target.to_string())
        } else if self.defs.contains_key(fname) {
            self.check_ref(from, from_index, fname, Expected::Axes);
            Some(fname.to_string())
        } else {
            self.error(format!(
                "'{}' : dans '{}', '{}' n'est ni une fonction ({}) ni une figure axes",
                from, value, fname, REF_FUNCTIONS.join(", ")
            ));
            None
        }
    }

    // ─────────────────────────────────────────────
    //  [timeline]
    // ─────────────────────────────────────────────

    fn check_timeline(&mut self, doc: &Document) {
        // Timestamps uniques (comparés à la milliseconde)
        let mut seen = HashSet::new();
        for ev in &doc.timeline {
            if !seen.insert((ev.timestamp * 1000.0).round() as i64) {
                self.error(format!("timestamp {:.3} : apparaît deux fois", ev.timestamp));
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

            // On regroupe les commandes par type, quel que soit l'ordre d'écriture
            let mut destroy = Vec::new();
            let mut end = Vec::new();
            let mut start = Vec::new();
            let mut anim = Vec::new();

            for action in &ev.actions {
                match action {
                    TimelineAction::Destroy(names) => destroy.extend(names.iter()),
                    TimelineAction::End(items) => end.extend(items.iter()),
                    TimelineAction::Start(items) => start.extend(items.iter()),
                    TimelineAction::Anim(items) => anim.extend(items.iter()),
                    TimelineAction::Comment(texts) => {
                        if texts.len() != nb_lang {
                            self.error(format!(
                                "t={:.3} : comment a {} traduction(s), {} attendue(s) ({:?})",
                                t, texts.len(), nb_lang, doc.config.languages
                            ));
                        }
                    }
                }
            }

            // Ordre fixe : destroy → end → start → anim (cf. SPEC §6.3)

            // 1. destroy
            let mut destroyed_now = Vec::new();
            for name in destroy {
                if self.check_usable(t, name, &destroyed) {
                    destroyed_now.push(name.clone());
                }
            }
            for name in &destroyed_now {
                destroyed.insert(name.clone(), t);
            }
            // Une figure détruite ne doit plus être référencée par une figure encore en vie
            for name in &destroyed_now {
                let mut users: Vec<&String> = self
                    .figure_refs
                    .iter()
                    .filter(|(user, refs)| !destroyed.contains_key(*user) && refs.contains(name))
                    .map(|(user, _)| user)
                    .collect();
                users.sort();
                let users: Vec<String> = users.into_iter().cloned().collect();
                for user in users {
                    self.error(format!(
                        "t={:.3} : impossible de détruire '{}', '{}' la référence encore (détruire '{}' avant ou en même temps)",
                        t, name, user, user
                    ));
                }
            }

            // 2. end
            for item in end {
                self.check_usable(t, &item.name, &destroyed);
                if let Some(call) = &item.anim {
                    let (fname, _) = split_call(call);
                    self.check_anim_name(t, &item.name, fname);
                }
            }

            // 3. start
            for item in start {
                if let StartItem::Figure(name) = item {
                    self.check_usable(t, name, &destroyed);
                }
            }

            // 4. anim
            for item in anim {
                self.check_usable(t, &item.figure, &destroyed);
                self.check_anim_name(t, &item.figure, &item.call.name);
                // Paramètres qui désignent une figure : follow(path=…), transform(to=…)
                for p in &item.call.params {
                    let target = match (item.call.name.as_str(), p.split_once('=')) {
                        ("follow", Some(("path", v))) => Some(v),
                        ("transform", Some(("to", v))) => Some(v),
                        _ => None,
                    };
                    if let Some(target) = target {
                        self.check_usable(t, target, &destroyed);
                    }
                }
            }
        }
    }

    /// Vérifie qu'un nom utilisé dans la timeline est une figure existante et pas encore détruite.
    fn check_usable(&mut self, t: f64, name: &str, destroyed: &HashMap<String, f64>) -> bool {
        match self.defs.get(name) {
            None => {
                self.error(format!("t={:.3} : '{}' n'est pas défini", t, name));
                false
            }
            Some(Def { kind: Kind::Style, .. }) => {
                self.error(format!("t={:.3} : '{}' est un style, pas une figure", t, name));
                false
            }
            Some(_) => match destroyed.get(name) {
                Some(t_destroy) => {
                    self.error(format!(
                        "t={:.3} : '{}' est utilisée alors qu'elle a été détruite à t={:.3}",
                        t, name, t_destroy
                    ));
                    false
                }
                None => true,
            },
        }
    }

    fn check_anim_name(&mut self, t: f64, figure: &str, anim: &str) {
        if !ANIMATIONS.contains(&anim) {
            self.error(format!("t={:.3} : '{}' : animation inconnue '{}'", t, figure, anim));
        }
    }
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
            "défini plus bas",
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
            "défini plus bas",
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
            "défini deux fois",
        );
    }

    #[test]
    fn type_inconnu() {
        assert_error(
            "[figures]
            deffigure a =
                type : triangle
            [timeline]",
            "type inconnu",
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
            "apparaît deux fois",
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
            "'Ma_Ligne' n'est pas défini",
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
            "détruite à t=5.000",
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
            "impossible de détruire 'a'",
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
            "1 traduction(s), 2 attendue(s)",
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
            "animation inconnue 'teleport'",
        );
    }
}
