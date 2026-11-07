//! Modèles de figures : `defmodel` et `defrepeat`.
//!
//! Un modèle est un bout de texte avec des trous : `@nom` est remplacé par la valeur de
//! l'argument, `@(i * 0.5)` par le résultat du calcul. Ce module ne fait que ce remplacement ;
//! le parser relit ensuite le texte obtenu comme une figure ordinaire.

use crate::renderer::expr;
use std::collections::HashMap;

/// defmodel nom(a, b) = + ses lignes
#[derive(Debug, Clone)]
pub struct Model {
    pub params: Vec<String>,
    pub body: Vec<String>,
}

/// Arguments d'un appel : séparés par des virgules, sauf dans des parenthèses ou des guillemets.
/// Les guillemets autour d'un argument sont retirés.
pub fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let (mut depth, mut quoted) = (0i32, false);
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                args.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() || !args.is_empty() {
        args.push(current);
    }
    args.iter()
        .map(|a| {
            let a = a.trim();
            a.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(a).to_string()
        })
        .collect()
}

/// Lit `nom(arguments)` : renvoie le nom et le texte entre les parenthèses
pub fn split_call(text: &str) -> Option<(&str, &str)> {
    let (name, rest) = text.trim().split_once('(')?;
    Some((name.trim(), rest.trim_end().strip_suffix(')')?))
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Évalue `@(...)` : les valeurs doivent être des nombres
fn compute(src: &str, values: &HashMap<String, String>) -> Result<String, String> {
    let mut numbers: HashMap<String, f64> = HashMap::new();
    for (name, value) in values {
        if let Ok(n) = value.trim().parse::<f64>() {
            numbers.insert(name.clone(), n);
        }
    }
    let names: Vec<String> = numbers.keys().cloned().collect();
    let e = expr::parse_with(src, &names).map_err(|err| format!("@({}) : {} (seuls les nombres se calculent)", src, err))?;
    let v = e.eval(0.0, &numbers);
    if !v.is_finite() {
        return Err(format!("@({}) ne donne pas un nombre", src));
    }
    // Arrondi : évite 0.30000000000000004 dans les fichiers relus
    Ok(((v * 1e9).round() / 1e9).to_string())
}

/// Remplace les `@nom` et `@(calcul)` d'une ligne. Un `@nom` inconnu est une erreur.
pub fn substitute(line: &str, values: &HashMap<String, String>) -> Result<String, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if chars.get(i) == Some(&'(') {
            let start = i + 1;
            let mut depth = 0;
            let mut end = None;
            for (k, c) in chars.iter().enumerate().skip(i) {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(k);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let end = end.ok_or_else(|| "@( sans parenthèse fermante".to_string())?;
            let src: String = chars[start..end].iter().collect();
            out.push_str(&compute(&src, values)?);
            i = end + 1;
        } else {
            let start = i;
            while i < chars.len() && is_name_char(chars[i]) {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            match values.get(&name) {
                Some(v) => out.push_str(v),
                None if name.is_empty() => return Err("@ seul : écrire @nom ou @(calcul)".to_string()),
                None => return Err(format!("@{} n'est pas un argument", name)),
            }
        }
    }
    Ok(out)
}

/// Texte d'une figure `name` créée à partir d'un modèle
pub fn expand(model_name: &str, model: &Model, name: &str, args: &[String]) -> Result<String, String> {
    if args.len() != model.params.len() {
        return Err(format!(
            "le modèle '{}' attend {} argument(s) ({}), {} reçu(s)",
            model_name,
            model.params.len(),
            model.params.join(", "),
            args.len()
        ));
    }
    let values: HashMap<String, String> = model.params.iter().cloned().zip(args.iter().cloned()).collect();
    let mut text = format!("deffigure {} =\n", name);
    for line in &model.body {
        let line = substitute(line, &values).map_err(|e| format!("modèle '{}' : {}", model_name, e))?;
        text.push_str(&line);
        text.push('\n');
    }
    Ok(text)
}

/// Lit `i in 0..7` : variable, début et fin (incluse)
pub fn parse_range(text: &str) -> Option<(String, i64, i64)> {
    let (var, range) = text.split_once(" in ")?;
    let (a, b) = range.trim().split_once("..")?;
    Some((var.trim().to_string(), a.trim().parse().ok()?, b.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn arguments() {
        assert_eq!(split_args(r#"1, "a, b", f(x, y), #FF0000"#), ["1", "a, b", "f(x, y)", "#FF0000"]);
        assert!(split_args("  ").is_empty());
    }

    #[test]
    fn remplacement() {
        let v = values(&[("c", "red"), ("i", "3"), ("i2", "4")]);
        assert_eq!(substitute("color : @c", &v).unwrap(), "color : red");
        assert_eq!(substitute("pos : r(@(i/2), @(i + 1))", &v).unwrap(), "pos : r(1.5, 4)");
        assert_eq!(substitute("a @i2 @i", &v).unwrap(), "a 4 3"); // nom entier
        assert_eq!(substitute(r"\frac{x^@i}{@(i*2)}", &v).unwrap(), r"\frac{x^3}{6}");
        assert_eq!(substitute("@(0.1 + 0.2)", &v).unwrap(), "0.3");
        assert!(substitute("@z", &v).unwrap_err().contains("@z"));
        assert!(substitute("@(c + 1)", &v).is_err()); // red n'est pas un nombre
    }

    #[test]
    fn intervalle() {
        assert_eq!(parse_range("i in 0..7"), Some(("i".to_string(), 0, 7)));
        assert_eq!(parse_range("i de 0 à 7"), None);
    }
}
