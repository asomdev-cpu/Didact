//! Paramètres dans les propriétés et dans le texte.
//!
//! - Toute propriété numérique peut être une expression de paramètres, avec son unité :
//!   `radius : 2*k`, `stroke : (k + 1)px`, `opacity : k/4`, `pos : (k*10%, 50%)`, `rotate : 30k`.
//!   À chaque image, la valeur est calculée avec les paramètres du moment.
//! - Dans le texte (content, expr d'une equation, rows, labels, ...) `#{expr}` est remplacé
//!   par la valeur : `#{k}` (au plus 2 décimales), `#{k:0}` (entier), `#{k:3}` (3 décimales).

use super::expr::{self, Expr};
use crate::ast::PropValue;
use crate::resolver::ResolvedFigure;
use std::collections::HashMap;

/// Propriétés de texte : seules les `#{...}` y sont remplacées
const TEXT_PROPS: &[&str] = &[
    "type", "expr", "content", "code", "rows", "header", "labels", "xlabel", "ylabel", "zlabel", "label",
    "color_by", "update", "init", "vars", "map", "cmap", "upper", "lower", "where", "domain", "escape", "z0", "ode", "nodes", "edges", "weights", "values", "layout", "root", "sample", "x", "y", "allow", "style", "scene", "on", "axes", "items",
];

/// Unités acceptées après une expression
const UNITS: &[&str] = &["px", "%", "deg", "rad"];

/// L'expression sans son unité : "(k+1)px" → "(k+1)"
pub fn strip_unit(s: &str) -> &str {
    let s = s.trim();
    UNITS.iter().find_map(|u| s.strip_suffix(u)).unwrap_or(s)
}

/// Expression de paramètres (avec unité éventuelle) : `2*k`, `(k+1)px`, `k*10%`.
/// None si ce n'est pas une expression qui utilise au moins un paramètre (nombre seul, nom, x, ...).
pub fn param_expr(s: &str, params: &[String]) -> Option<(Expr, &'static str)> {
    if params.is_empty() {
        return None;
    }
    let s = s.trim();
    let (body, unit) = UNITS
        .iter()
        .find_map(|u| s.strip_suffix(u).map(|b| (b, *u)))
        .unwrap_or((s, ""));
    // Un nombre seul (avec ou sans unité) n'a rien à calculer
    if body.trim().is_empty() || body.trim().parse::<f64>().is_ok() {
        return None;
    }
    let e = expr::parse_with(body, params).ok()?;
    let mut used = Vec::new();
    e.vars(&mut used);
    if used.is_empty() || e.uses_x() {
        return None;
    }
    Some((e, unit))
}

/// Nombre écrit court : 3, 2.5, 0.33
pub fn fmt_num(v: f64, decimals: Option<usize>) -> String {
    match decimals {
        Some(d) => format!("{:.*}", d, v),
        None => {
            let s = format!("{:.2}", v);
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s == "-0" { "0".to_string() } else { s.to_string() }
        }
    }
}

/// Les `#{expr}` ou `#{expr:d}` d'un texte : (début, fin, expression, décimales)
pub fn interpolations(s: &str) -> Result<Vec<(usize, usize, String, Option<usize>)>, String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = s[from..].find("#{") {
        let start = from + i;
        let rest = &s[start + 2..];
        // Accolade fermante correspondante (une expression n'en contient pas, mais on reste prudent)
        let mut depth = 1;
        let mut end = None;
        for (j, c) in rest.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| format!("'#{{' sans '}}' dans « {} »", s))?;
        let inner = &rest[..end];
        let (src, decimals) = match inner.rsplit_once(':') {
            Some((e, d)) if d.trim().parse::<usize>().is_ok() => (e.trim(), Some(d.trim().parse::<usize>().unwrap().min(12))),
            Some((_, d)) => return Err(format!("#{{{}}} : décimales invalides '{}' (ex : #{{k:2}})", inner, d)),
            None => (inner.trim(), None),
        };
        if src.is_empty() {
            return Err("#{} vide : écrire #{nom_du_parametre}".to_string());
        }
        out.push((start, start + 2 + end + 1, src.to_string(), decimals));
        from = start + 2 + end + 1;
    }
    Ok(out)
}

/// Remplace les `#{...}` par leurs valeurs
pub fn interpolate(s: &str, names: &[String], vals: &HashMap<String, f64>) -> Result<String, String> {
    let parts = interpolations(s)?;
    if parts.is_empty() {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (a, b, src, decimals) in parts {
        out.push_str(&s[last..a]);
        let e = expr::parse_with(&src, names).map_err(|e| format!("#{{{}}} : {}", src, e))?;
        out.push_str(&fmt_num(e.eval(0.0, vals), decimals));
        last = b;
    }
    out.push_str(&s[last..]);
    Ok(out)
}

/// Valeur calculée d'un élément (de tuple ou de liste) : "2.5%" / "3px" / "4"
fn item_value(s: &str, names: &[String], vals: &HashMap<String, f64>) -> Option<String> {
    let (e, unit) = param_expr(s, names)?;
    Some(format!("{}{}", e.eval(0.0, vals), unit))
}

/// Point "(a,b)" d'une liste dont les coordonnées sont des expressions
fn point_value(s: &str, names: &[String], vals: &HashMap<String, f64>) -> Option<String> {
    let inside = s.trim().strip_prefix('(')?.strip_suffix(')')?;
    let call = format!("p({})", inside);
    let (_, args) = super::geometry::split_call(&call);
    let parts: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut changed = false;
    let vals: Vec<String> = parts
        .iter()
        .map(|p| match item_value(p, names, vals) {
            Some(v) => {
                changed = true;
                v
            }
            None => p.clone(),
        })
        .collect();
    changed.then(|| format!("({})", vals.join(",")))
}

/// Couleur `hsl(...)` dont les composantes sont des expressions : `hsl(360*t, 80%, 50%)`
fn color_value(s: &str, names: &[String], vals: &HashMap<String, f64>) -> Option<PropValue> {
    let inside = s.trim().strip_prefix("hsl(")?.strip_suffix(')')?;
    let call = format!("p({})", inside);
    let (_, args) = super::geometry::split_call(&call);
    let mut changed = false;
    let parts: Vec<String> = args
        .iter()
        .map(|a| match item_value(a, names, vals) {
            Some(v) => {
                changed = true;
                v
            }
            None => a.to_string(),
        })
        .collect();
    changed.then(|| PropValue::Single(format!("hsl({})", parts.join(", "))))
}

/// La figure avec ses expressions de paramètres calculées et ses `#{...}` remplacés ;
/// None si elle n'en a pas
pub fn bind(fig: &ResolvedFigure, vals: &HashMap<String, f64>) -> Result<Option<ResolvedFigure>, String> {
    if vals.is_empty() {
        return Ok(None);
    }
    let mut names: Vec<String> = vals.keys().cloned().collect();
    names.sort();
    let mut out: Option<ResolvedFigure> = None;
    for (key, v) in &fig.properties {
        let text = TEXT_PROPS.contains(&key.as_str());
        let new = match v {
            PropValue::Single(s) | PropValue::Block(s) if s.contains("#{") => {
                let r = interpolate(s, &names, vals).map_err(|e| format!("{} : {}", key, e))?;
                Some(if matches!(v, PropValue::Block(_)) { PropValue::Block(r) } else { PropValue::Single(r) })
            }
            PropValue::List(items) if items.iter().any(|i| i.contains("#{")) => {
                let r: Result<Vec<String>, String> = items.iter().map(|i| interpolate(i, &names, vals)).collect();
                Some(PropValue::List(r.map_err(|e| format!("{} : {}", key, e))?))
            }
            _ if text => None,
            PropValue::Single(s) if s.trim_start().starts_with("hsl(") => color_value(s, &names, vals),
            PropValue::Single(s) => param_expr(s, &names).map(|(e, unit)| {
                let n = e.eval(0.0, vals);
                match unit {
                    "" => PropValue::Number(n),
                    "%" => PropValue::Percent(format!("{}%", n)),
                    u => PropValue::Single(format!("{}{}", n, u)),
                }
            }),
            PropValue::Tuple(items) => {
                let mapped: Vec<Option<String>> = items.iter().map(|i| item_value(i, &names, vals)).collect();
                mapped.iter().any(|m| m.is_some()).then(|| {
                    PropValue::Tuple(items.iter().zip(mapped).map(|(i, m)| m.unwrap_or_else(|| i.clone())).collect())
                })
            }
            PropValue::List(items) => {
                let mapped: Vec<Option<String>> = items
                    .iter()
                    .map(|i| if i.starts_with('(') { point_value(i, &names, vals) } else { item_value(i, &names, vals) })
                    .collect();
                mapped.iter().any(|m| m.is_some()).then(|| {
                    PropValue::List(items.iter().zip(mapped).map(|(i, m)| m.unwrap_or_else(|| i.clone())).collect())
                })
            }
            _ => None,
        };
        if let Some(new) = new {
            out.get_or_insert_with(|| fig.clone()).properties.insert(key.clone(), new);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["k".to_string(), "n".to_string()]
    }

    #[test]
    fn expressions_avec_unites() {
        let vals: HashMap<String, f64> = [("k".to_string(), 3.0), ("n".to_string(), 2.0)].into();
        let v = |s: &str| param_expr(s, &names()).map(|(e, u)| (e.eval(0.0, &vals), u));
        assert_eq!(v("2*k"), Some((6.0, "")));
        assert_eq!(v("(k + 1)px"), Some((4.0, "px")));
        assert_eq!(v("k*10%"), Some((30.0, "%")));
        assert_eq!(v("30k deg"), Some((90.0, "deg")));
        assert_eq!(v("k"), Some((3.0, "")));
        // Rien à calculer : nombres, noms, couleurs, formules en x
        for s in ["3", "2px", "50%", "red", "#FF0000", "ease-in-out", "sin(k*x)", "center(a)", "repere(k, 1)"] {
            assert!(v(s).is_none(), "{}", s);
        }
    }

    #[test]
    fn couleur_hsl_parametree() {
        let vals: HashMap<String, f64> = [("k".to_string(), 0.5), ("n".to_string(), 2.0)].into();
        let v = |s: &str| color_value(s, &names(), &vals);
        assert_eq!(v("hsl(360*k, 80%, 50%)"), Some(PropValue::Single("hsl(180, 80%, 50%)".into())));
        assert_eq!(v("hsl(0, (40*n)%, 50%, k)"), Some(PropValue::Single("hsl(0, 80%, 50%, 0.5)".into())));
        assert_eq!(v("hsl(0, 80%, 50%)"), None);
    }

    #[test]
    fn texte_dynamique() {
        let vals: HashMap<String, f64> = [("k".to_string(), 2.0 / 3.0), ("n".to_string(), 12.0)].into();
        let t = |s: &str| interpolate(s, &names(), &vals).unwrap();
        assert_eq!(t("n = #{n}"), "n = 12");
        assert_eq!(t("k = #{k}"), "k = 0.67");
        assert_eq!(t("k = #{k:3}, 2k = #{2k:0}"), "k = 0.667, 2k = 1");
        assert_eq!(t("\\sum_{i=1}^{#{n}} i = #{n(n+1)/2}"), "\\sum_{i=1}^{12} i = 78");
        assert_eq!(t("sans rien"), "sans rien");
        assert!(interpolate("#{k", &names(), &vals).is_err());
        assert!(interpolate("#{z}", &names(), &vals).unwrap_err().contains("nom inconnu"));
        assert!(interpolate("#{k:a}", &names(), &vals).unwrap_err().contains("décimales"));
    }
}
