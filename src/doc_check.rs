//! La documentation doit décrire tout le langage : une IA qui écrit un `.dct` n'a que
//! `Readme/SPEC.md` et `Readme/REFERENCE.md`, pas le code. Ces tests comparent les listes
//! du code (types, propriétés, animations, fonctions, ...) aux deux documents.

use crate::validator::{type_props, ANIMATIONS, COMMON_PROPS, FIGURE_TYPES, PLANE_PROPS, REF_FUNCTIONS, SPACE_PROPS};

const SPEC: &str = include_str!("../Readme/SPEC.md");
const REFERENCE: &str = include_str!("../Readme/REFERENCE.md");

/// Le mot entier `w` apparaît dans `text` (lettres, chiffres, `_` et `.` comptent dans un mot)
fn has_word(text: &str, w: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    text.match_indices(w).any(|(i, _)| {
        let before = text[..i].chars().next_back().map_or(true, |c| !is_word(c));
        let after = text[i + w.len()..].chars().next().map_or(true, |c| !is_word(c));
        before && after
    })
}

/// Les lignes de tableau dont la première case nomme le type `t`
fn rows_of<'a>(doc: &'a str, t: &str) -> String {
    doc.lines()
        .filter(|l| l.starts_with('|'))
        .filter(|l| {
            let first = l.split('|').nth(1).unwrap_or("");
            has_word(&first.replace('`', " ").replace(',', " ").replace(';', " "), t)
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

fn missing(what: &str, list: Vec<String>) {
    assert!(list.is_empty(), "{} absents de la documentation : {:?}", what, list);
}

#[test]
fn chaque_type_et_ses_proprietes_sont_documentes() {
    let mut m = Vec::new();
    // `group` : documenté avec `defgroup`
    for t in FIGURE_TYPES.iter().filter(|t| !["raw.matplotlib", "group"].contains(t)) {
        for (name, doc) in [("SPEC", SPEC), ("REFERENCE", REFERENCE)] {
            let rows = rows_of(doc, t);
            if rows.is_empty() {
                m.push(format!("{} : type {}", name, t));
                continue;
            }
            if let Some((own, _)) = type_props(t) {
                for p in own {
                    // `anchor` : la ligne qui le décrit nomme chaque type
                    let ok = match *p {
                        "anchor" => doc.lines().any(|l| l.contains("`anchor`") && has_word(l, t)),
                        _ => has_word(&rows, p),
                    };
                    if !ok {
                        m.push(format!("{} : {}.{}", name, t, p));
                    }
                }
            }
        }
    }
    missing("types ou propriétés", m);
}

#[test]
fn proprietes_communes_animations_et_fonctions_documentees() {
    let mut m = Vec::new();
    let groups: [(&str, &[&str]); 5] = [
        ("propriété", COMMON_PROPS),
        ("propriété 2D", PLANE_PROPS),
        ("propriété 3D", SPACE_PROPS),
        ("animation", ANIMATIONS),
        ("position", REF_FUNCTIONS),
    ];
    for (what, list) in groups {
        for w in list.iter().filter(|w| **w != "type") {
            for (name, doc) in [("SPEC", SPEC), ("REFERENCE", REFERENCE)] {
                if !has_word(doc, w) {
                    m.push(format!("{} : {} {}", name, what, w));
                }
            }
        }
    }
    let functions = crate::renderer::expr::FUNCTIONS.iter().map(|f| f.0).chain(["sum", "prod", "pi", "e"]);
    for f in functions {
        for (name, doc) in [("SPEC", SPEC), ("REFERENCE", REFERENCE)] {
            if !has_word(doc, f) {
                m.push(format!("{} : fonction {}", name, f));
            }
        }
    }
    let values = crate::renderer::geometry::MARKERS
        .iter()
        .chain(crate::renderer::three::PALETTES.iter().map(|p| &p.0))
        .chain(["linear", "smooth", "ease_in", "ease_out", "window", "background", "languages", "default_lang", "decimal"].iter());
    for v in values {
        if !has_word(SPEC, v) {
            m.push(format!("SPEC : valeur {}", v));
        }
    }
    missing("noms", m);
}
