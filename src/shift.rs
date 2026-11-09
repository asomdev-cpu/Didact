//! `didact shift` : décale les blocs de la timeline à partir d'un instant.
//!
//! Pour ajouter du temps au milieu d'une animation (ou en retirer) sans réécrire
//! chaque timestamp à la main. Seuls les en-têtes de blocs (`16.000 =>`) sont
//! modifiés ; le reste du fichier (espaces, commentaires, fins de ligne) est gardé tel quel.

/// Un bloc déplacé : ligne, ancien instant, nouvel instant
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub line: usize,
    pub from: f64,
    pub to: f64,
}

/// Les timestamps sont au millième de seconde
fn ms(t: f64) -> f64 {
    (t * 1000.0).round() / 1000.0
}

/// Instants des blocs du fichier, avec leur ligne
fn blocks(source: &str) -> Result<Vec<(usize, f64)>, String> {
    let tokens = crate::lexer::tokenize(source);
    let doc = crate::parser::parse_with_source(tokens, source)?;
    Ok(doc.timeline.iter().map(|e| (e.line, e.timestamp)).collect())
}

/// Décale de `delta` secondes tous les blocs qui commencent à `start` ou après.
/// Renvoie le nouveau texte du fichier et la liste des blocs déplacés.
/// Refuse (sans rien changer) si un bloc passerait avant 0, sur un autre bloc ou avant un bloc non décalé.
pub fn shift(source: &str, start: f64, delta: f64) -> Result<(String, Vec<Moved>), String> {
    let (start, delta) = (ms(start), ms(delta));
    if delta == 0.0 {
        return Err("décalage nul : rien à faire".to_string());
    }
    let events = blocks(source).map_err(|e| format!("le fichier ne se lit pas : {}", e))?;

    let moved: Vec<Moved> = events
        .iter()
        .filter(|(_, t)| ms(*t) >= start)
        .map(|&(line, t)| Moved { line, from: ms(t), to: ms(t + delta) })
        .collect();
    if moved.is_empty() {
        let last = events.iter().map(|e| e.1).fold(f64::NAN, f64::max);
        return Err(match last.is_nan() {
            true => "la timeline est vide".to_string(),
            false => format!("aucun bloc à partir de {:.3} (le dernier est à {:.3})", start, last),
        });
    }
    if let Some(m) = moved.iter().find(|m| m.to < 0.0) {
        return Err(format!("ligne {} : le bloc à {:.3} passerait à {:.3}, avant 0", m.line, m.from, m.to));
    }
    // En reculant, le premier bloc décalé doit rester après le dernier bloc qui ne bouge pas
    if delta < 0.0 {
        let first = moved.iter().min_by(|a, b| a.to.total_cmp(&b.to)).unwrap();
        if let Some(&(line, t)) = events.iter().filter(|(_, t)| ms(*t) < start).max_by(|a, b| a.1.total_cmp(&b.1)) {
            if first.to <= ms(t) {
                return Err(format!(
                    "ligne {} : le bloc à {:.3} passerait à {:.3}, sur ou avant le bloc à {:.3} (ligne {}). Recul maximal : {:.3}",
                    first.line,
                    first.from,
                    first.to,
                    t,
                    line,
                    ms(t) - first.from + 0.001
                ));
            }
        }
    }

    // Réécriture : sur chaque ligne d'un bloc décalé, le nombre juste avant `=>`
    let mut out = String::with_capacity(source.len() + 16);
    let mut replaced = 0;
    for (i, text) in source.split_inclusive('\n').enumerate() {
        let here: Vec<&Moved> = moved.iter().filter(|m| m.line == i + 1).collect();
        if here.is_empty() {
            out.push_str(text);
            continue;
        }
        let (new_text, n) = rewrite_line(text, &here);
        replaced += n;
        out.push_str(&new_text);
    }
    if replaced != moved.len() {
        return Err(format!("{} bloc(s) à décaler, {} trouvé(s) dans le texte : fichier non modifié", moved.len(), replaced));
    }

    // Vérification : le nouveau fichier se relit, avec exactement les instants attendus
    let mut expected: Vec<f64> =
        events.iter().map(|&(_, t)| if ms(t) >= start { ms(t + delta) } else { ms(t) }).collect();
    let mut got: Vec<f64> = blocks(&out).map_err(|e| format!("après décalage, le fichier ne se lit plus : {}", e))?.iter().map(|e| ms(e.1)).collect();
    expected.sort_by(f64::total_cmp);
    got.sort_by(f64::total_cmp);
    if expected != got {
        return Err("après décalage, les instants ne sont pas ceux attendus : fichier non modifié".to_string());
    }
    Ok((out, moved))
}

/// Remplace, sur une ligne, les timestamps suivis de `=>` qui correspondent à un bloc décalé
fn rewrite_line(text: &str, moved: &[&Moved]) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 8);
    let (mut i, mut last, mut count) = (0, 0, 0);
    while i < bytes.len() {
        let starts_number = bytes[i].is_ascii_digit()
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.' || bytes[i - 1] == b'_'));
        if !starts_number {
            i += 1;
            continue;
        }
        let begin = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        let mut j = i;
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        if !text[j..].starts_with("=>") {
            continue;
        }
        let Ok(t) = text[begin..i].parse::<f64>() else { continue };
        if let Some(m) = moved.iter().find(|m| m.from == ms(t)) {
            out.push_str(&text[last..begin]);
            out.push_str(&format!("{:.3}", m.to));
            last = i;
            count += 1;
        }
    }
    out.push_str(&text[last..]);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "[figures]\r\ndeffigure a =\r\n    type : line   # 2.000 => dans un commentaire\r\n[timeline]\r\n\r\n0.000 => { start : [a] }\r\n\r\n5.000 => {\r\n    anim : [a fadeout(1s)]\r\n}\r\n10.5   =>  { end : [a] }  # fin\r\n";

    fn times(src: &str) -> Vec<f64> {
        blocks(src).unwrap().iter().map(|e| e.1).collect()
    }

    #[test]
    fn ajoute_du_temps_au_milieu() {
        let (out, moved) = shift(SRC, 5.0, 2.0).unwrap();
        assert_eq!(times(&out), [0.0, 7.0, 12.5]);
        assert_eq!(moved, [Moved { line: 8, from: 5.0, to: 7.0 }, Moved { line: 11, from: 10.5, to: 12.5 }]);
        // Le reste du fichier ne bouge pas : fins de ligne, espaces, commentaires
        assert_eq!(out, SRC.replace("5.000 => {\r\n", "7.000 => {\r\n").replace("10.5   =>", "12.500   =>"));
    }

    #[test]
    fn retire_du_temps() {
        let (out, _) = shift(SRC, 5.0, -4.5).unwrap();
        assert_eq!(times(&out), [0.0, 0.5, 6.0]);
        // depuis un instant entre deux blocs
        let (out, moved) = shift(SRC, 6.0, -1.0).unwrap();
        assert_eq!(times(&out), [0.0, 5.0, 9.5]);
        assert_eq!(moved.len(), 1);
    }

    #[test]
    fn refuse_ce_qui_casserait_le_fichier() {
        let e = shift(SRC, 5.0, -5.0).unwrap_err();
        assert!(e.contains("sur ou avant le bloc à 0.000") && e.contains("Recul maximal : -4.999"), "{}", e);
        assert!(shift(SRC, 0.0, -1.0).unwrap_err().contains("avant 0"));
        assert!(shift(SRC, 11.0, 1.0).unwrap_err().contains("aucun bloc à partir de 11.000"));
        assert!(shift(SRC, 5.0, 0.0).unwrap_err().contains("nul"));
        assert!(shift("[timeline]\n0.000 { }", 0.0, 1.0).unwrap_err().contains("ne se lit pas"));
    }

    #[test]
    fn plusieurs_blocs_sur_une_ligne() {
        let src = "[figures]\ndeffigure a =\n    type : line\n[timeline]\n1.000 => { start : [a] } 2.000 => { end : [a] }\n";
        let (out, _) = shift(src, 1.5, 0.25).unwrap();
        assert!(out.contains("1.000 => { start : [a] } 2.250 => { end : [a] }"), "{}", out);
    }
}
