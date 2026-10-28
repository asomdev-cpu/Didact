//! Résolveur.
//!
//! Reçoit un `Document` **déjà validé** et produit un `ResolvedDocument` où :
//! - chaque figure porte ses valeurs effectives : propriétés héritées de son style
//!   (et de toute la chaîne `extends`), écrasées par celles écrites sur la figure ;
//! - les propriétés `style` et `extends` ont disparu (elles ont été appliquées) ;
//! - les figures sont triées par `layer` (ordre de dessin) ;
//! - la timeline est triée chronologiquement.
//!
//! Le validateur garantit que les styles référencés existent et sont définis plus haut,
//! donc la chaîne `extends` ne peut pas boucler.

use crate::ast::*;
use std::collections::HashMap;

/// Une figure prête pour le moteur d'exécution
#[derive(Debug, Clone)]
pub struct ResolvedFigure {
    pub name: String,
    /// Position de la définition dans le fichier (ordre de calcul des références)
    pub index: usize,
    /// Type de la figure (line, circle, group, ...)
    pub ftype: String,
    /// Ordre de dessin : les petites valeurs sont dessinées en premier (donc en dessous)
    pub layer: f64,
    /// Ligne du `deffigure`
    pub line: usize,
    /// Valeurs effectives (style + héritage + surcharges)
    pub properties: HashMap<String, PropValue>,
    /// Pour chaque propriété, la ligne où sa valeur effective est écrite
    /// (dans la figure ou dans un des styles) : utile pour les messages d'erreur
    pub prop_lines: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDocument {
    pub config: Config,
    /// Triées par layer, puis par ordre de définition
    pub figures: Vec<ResolvedFigure>,
    /// Triée par timestamp
    pub timeline: Vec<TimelineEvent>,
}

pub fn resolve(doc: &Document) -> ResolvedDocument {
    let styles: HashMap<&str, &StyleDef> =
        doc.styles.iter().map(|s| (s.name.as_str(), s)).collect();

    let mut figures: Vec<(usize, ResolvedFigure)> = doc
        .figures
        .iter()
        .map(|f| (f.index, resolve_figure(f, &styles)))
        .collect();

    // Tri par layer ; à layer égal, l'ordre de définition est conservé
    figures.sort_by(|(ia, a), (ib, b)| {
        a.layer.partial_cmp(&b.layer).unwrap().then(ia.cmp(ib))
    });

    let mut timeline = doc.timeline.clone();
    timeline.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap());

    ResolvedDocument {
        config: doc.config.clone(),
        figures: figures.into_iter().map(|(_, f)| f).collect(),
        timeline,
    }
}

fn resolve_figure(f: &FigureDef, styles: &HashMap<&str, &StyleDef>) -> ResolvedFigure {
    let mut properties = HashMap::new();
    let mut prop_lines = HashMap::new();

    // 1. Le style et ses ancêtres, du plus ancien au plus proche :
    //    chaque niveau écrase le précédent
    if let Some(PropValue::Single(style_name)) = f.properties.get("style") {
        for style in style_chain(style_name, styles) {
            for (key, value) in &style.properties {
                properties.insert(key.clone(), value.clone());
                let line = style.prop_lines.get(key).copied().unwrap_or(style.line);
                prop_lines.insert(key.clone(), line);
            }
        }
    }

    // 2. Les propriétés de la figure écrasent tout
    for (key, value) in &f.properties {
        if key == "style" {
            continue;
        }
        properties.insert(key.clone(), value.clone());
        let line = f.prop_lines.get(key).copied().unwrap_or(f.line);
        prop_lines.insert(key.clone(), line);
    }

    let ftype = match properties.get("type") {
        Some(PropValue::Single(t)) => t.clone(),
        _ => String::new(),
    };

    // Pas de layer écrit → 0
    let layer = match properties.get("layer") {
        Some(PropValue::Number(n)) => *n,
        _ => 0.0,
    };

    ResolvedFigure {
        name: f.name.clone(),
        index: f.index,
        ftype,
        layer,
        line: f.line,
        properties,
        prop_lines,
    }
}

/// Renvoie la chaîne d'héritage d'un style, du plus ancien au style lui-même.
/// Ex : trait_epais extends trait_fin → [trait_fin, trait_epais]
fn style_chain<'a>(name: &str, styles: &HashMap<&str, &'a StyleDef>) -> Vec<&'a StyleDef> {
    let mut chain = Vec::new();
    let mut current = styles.get(name).copied();
    while let Some(style) = current {
        chain.push(style);
        current = style.extends.as_deref().and_then(|parent| styles.get(parent).copied());
    }
    chain.reverse();
    chain
}

/// Affichage lisible d'une valeur (pour main.rs)
pub fn fmt_value(v: &PropValue) -> String {
    match v {
        PropValue::Single(s) | PropValue::Percent(s) => s.clone(),
        PropValue::Number(n) => n.to_string(),
        PropValue::Tuple(items) => format!("({})", items.join(", ")),
        PropValue::List(items) => format!("[{}]", items.join(", ")),
        PropValue::Block(s) => format!("| {}", s),
    }
}

// ─────────────────────────────────────────────
//  TESTS  (cargo test)
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser, validator};

    fn resolve_src(src: &str) -> ResolvedDocument {
        let doc = parser::parse(lexer::tokenize(src)).expect("le parsing doit réussir");
        validator::validate(&doc).expect("la validation doit réussir");
        resolve(&doc)
    }

    fn fig<'a>(doc: &'a ResolvedDocument, name: &str) -> &'a ResolvedFigure {
        doc.figures.iter().find(|f| f.name == name).expect("figure absente")
    }

    fn prop(f: &ResolvedFigure, key: &str) -> String {
        f.properties.get(key).map(fmt_value).unwrap_or_default()
    }

    #[test]
    fn style_applique() {
        let doc = resolve_src(
            "[figures]
            defstyle rouge =
                color : red
                stroke : 2px
            deffigure a =
                type  : line
                style : rouge
            [timeline]",
        );
        let a = fig(&doc, "a");
        assert_eq!(prop(a, "color"), "red");
        assert_eq!(prop(a, "stroke"), "2px");
        assert!(!a.properties.contains_key("style"));
    }

    #[test]
    fn figure_ecrase_le_style() {
        let doc = resolve_src(
            "[figures]
            defstyle rouge =
                color : red
                stroke : 2px
            deffigure a =
                type  : line
                style : rouge
                color : blue
            [timeline]",
        );
        let a = fig(&doc, "a");
        assert_eq!(prop(a, "color"), "blue");
        assert_eq!(prop(a, "stroke"), "2px");
        // La ligne pointe là où la valeur effective est écrite
        assert_eq!(a.prop_lines["color"], 8);
        assert_eq!(a.prop_lines["stroke"], 4);
    }

    #[test]
    fn heritage_sur_trois_niveaux() {
        let doc = resolve_src(
            "[figures]
            defstyle base =
                opacity : 0.5
                stroke  : 1px
                color   : black
            defstyle moyen =
                extends : base
                stroke  : 2px
            defstyle fort =
                extends : moyen
                color   : red
            deffigure a =
                type  : line
                style : fort
            [timeline]",
        );
        let a = fig(&doc, "a");
        assert_eq!(prop(a, "opacity"), "0.5"); // de base
        assert_eq!(prop(a, "stroke"), "2px");   // moyen écrase base
        assert_eq!(prop(a, "color"), "red");   // fort écrase base
        assert!(!a.properties.contains_key("extends"));
    }

    #[test]
    fn tri_par_layer_stable() {
        let doc = resolve_src(
            "[figures]
            deffigure dessus =
                type  : circle
                layer : 2
            deffigure sans_layer_1 =
                type : circle
            deffigure dessous =
                type  : circle
                layer : -1
            deffigure sans_layer_2 =
                type : circle
            [timeline]",
        );
        let noms: Vec<&str> = doc.figures.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(noms, ["dessous", "sans_layer_1", "sans_layer_2", "dessus"]);
    }

    #[test]
    fn timeline_triee() {
        let doc = resolve_src(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            5.000 => { end : [a] }
            0.000 => { start : [a] }",
        );
        let ts: Vec<f64> = doc.timeline.iter().map(|e| e.timestamp).collect();
        assert_eq!(ts, [0.0, 5.0]);
    }

    #[test]
    fn exemple_officiel() {
        let doc = resolve_src(include_str!("../exemple.dct"));
        // axes_xy utilise trait_fin
        let axes = fig(&doc, "axes_xy");
        assert_eq!(prop(axes, "stroke"), "2px");
        assert_eq!(prop(axes, "opacity"), "1");
        assert_eq!(axes.ftype, "axes");
    }
}
