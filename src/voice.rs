//! Voix : les `comment` lus à voix haute.
//!
//! `didact cours.dct --voice` fait lire chaque commentaire par un programme de synthèse
//! vocale choisi par l'utilisateur (Piper, Kokoro, …) et range un fichier audio par phrase
//! dans le dossier `cours_voix/` :
//!
//! ```text
//! cours_voix/
//!   index.txt        liste des fichiers générés (et commande utilisée par langue)
//!   FR/3f2a…c1.wav   une phrase, nommée d'après son texte
//!   EN/…
//! ```
//!
//! Ensuite, ces fichiers sont joués comme des pistes audio ordinaires, au moment où
//! chaque commentaire apparaît (fenêtre, web, MP4, page autonome).
//!
//! Ce module ne fait que les calculs communs (noms de fichiers, texte à prononcer) ;
//! il compile aussi pour le web.

use crate::runtime::Timeline;

/// Nom du dossier des voix pour un fichier `cours.dct` → `cours_voix`
pub fn dir_name(stem: &str) -> String {
    format!("{}_voix", stem)
}

/// Nom du fichier d'une phrase (relatif au dossier des voix) : `FR/<empreinte>.wav`.
/// L'empreinte dépend de la langue et du texte prononcé : changer une phrase
/// ne régénère que celle-là.
pub fn file_for(lang: &str, text: &str) -> String {
    format!("{}/{:016x}.wav", lang, fnv1a(&format!("{}\n{}", lang, spoken_text(text, lang))))
}

/// Une phrase à dire : instant, ligne du `comment`, texte affiché, fichier
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub t: f64,
    pub line: usize,
    pub text: String,
    pub file: String,
    /// Instant du commentaire suivant (ou fin de l'animation) : la phrase doit être finie avant
    pub next: f64,
}

/// Les phrases d'une langue, dans l'ordre (les commentaires vides sont ignorés)
pub fn lines(timeline: &Timeline, lang: &str) -> Vec<Line> {
    let index = timeline.lang_index(Some(lang));
    let comments = timeline.comments();
    comments
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let text = c.texts.get(index)?;
            if spoken_text(text, lang).trim().is_empty() {
                return None;
            }
            let next = comments.get(i + 1).map(|n| n.t).unwrap_or_else(|| timeline.duration());
            Some(Line { t: c.t, line: c.line, text: text.clone(), file: file_for(lang, text), next })
        })
        .collect()
}

/// Silence laissé entre la fin d'une phrase et le commentaire suivant (secondes)
pub const BREATH: f64 = 0.25;

/// Pauses de l'animation pour laisser finir les phrases.
///
/// Quand une phrase lue dure plus longtemps que l'écart avant le commentaire suivant,
/// l'animation s'arrête juste avant ce commentaire, le temps que la phrase se termine.
/// Il y a donc deux temps :
/// - le temps de la **timeline** (celui écrit dans le .dct) ;
/// - le temps **réel** (celui de la vidéo), qui inclut les pauses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Holds {
    /// (instant de la timeline, durée de la pause), triés
    list: Vec<(f64, f64)>,
}

impl Holds {
    /// Pauses d'une langue, d'après la durée de chaque fichier de voix
    pub fn new(lines: &[Line], duration_of: impl Fn(&str) -> Option<f64>) -> Holds {
        let mut list: Vec<(f64, f64)> = Vec::new();
        for l in lines {
            let Some(d) = duration_of(&l.file) else { continue };
            let wait = l.t + d + BREATH - l.next;
            if wait > 0.0 {
                match list.iter_mut().find(|(at, _)| (*at - l.next).abs() < 1e-9) {
                    Some(h) => h.1 = h.1.max(wait),
                    None => list.push((l.next, wait)),
                }
            }
        }
        list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Holds { list }
    }

    pub fn list(&self) -> &[(f64, f64)] {
        &self.list
    }

    /// Durée totale des pauses
    pub fn extra(&self) -> f64 {
        self.list.iter().map(|h| h.1).sum()
    }

    /// Temps réel → temps de la timeline. Pendant une pause, l'image reste juste avant le commentaire.
    pub fn to_timeline(&self, real: f64) -> f64 {
        let mut offset = 0.0;
        for &(at, d) in &self.list {
            if real < at + offset {
                break;
            }
            if real < at + offset + d {
                return (at - 1e-6).max(0.0);
            }
            offset += d;
        }
        real - offset
    }

    /// Temps de la timeline → temps réel (un événement à l'instant d'une pause a lieu après elle)
    pub fn to_real(&self, t: f64) -> f64 {
        t + self.list.iter().filter(|(at, _)| *at <= t + 1e-9).map(|h| h.1).sum::<f64>()
    }
}

/// Empreinte FNV-1a 64 bits (stable d'une version à l'autre, sans dépendance)
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Texte à prononcer : les maths `$...$` sont transformées en mots simples
/// (« x au carré », « a sur b », …), le reste du LaTeX est retiré.
pub fn spoken_text(text: &str, lang: &str) -> String {
    let fr = lang.eq_ignore_ascii_case("FR");
    let mut out = String::new();
    let mut in_math = false;
    let mut math = String::new();
    for c in text.chars() {
        if c == '$' {
            if in_math {
                out.push_str(&speak_math(&math, fr));
                math.clear();
            }
            in_math = !in_math;
        } else if in_math {
            math.push(c);
        } else {
            out.push(c);
        }
    }
    out.push_str(&speak_math(&math, fr));
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Une formule LaTeX simple → mots
fn speak_math(m: &str, fr: bool) -> String {
    let mut s = m.to_string();
    // \frac{a}{b} → a sur b
    while let Some(i) = s.find("\\frac{") {
        let rest = &s[i + 6..];
        let Some((num, after)) = take_group(rest) else { break };
        let Some((den, after2)) = after.strip_prefix('{').and_then(take_group) else { break };
        let word = if fr { " sur " } else { " over " };
        s = format!("{}{}{}{}{}", &s[..i], num, word, den, after2);
    }
    // \sqrt{x} → racine de x
    while let Some(i) = s.find("\\sqrt{") {
        let Some((inner, after)) = take_group(&s[i + 6..]) else { break };
        let word = if fr { "racine de " } else { "square root of " };
        s = format!("{}{}{}{}", &s[..i], word, inner, after);
    }
    let words: &[(&str, &str, &str)] = &[
        ("^2", " au carré", " squared"),
        ("^{2}", " au carré", " squared"),
        ("^3", " au cube", " cubed"),
        ("^{3}", " au cube", " cubed"),
        ("^", " puissance ", " to the power "),
        ("\\cdot", " fois ", " times "),
        ("\\times", " fois ", " times "),
        ("\\leq", " inférieur ou égal à ", " less than or equal to "),
        ("\\geq", " supérieur ou égal à ", " greater than or equal to "),
        ("\\neq", " différent de ", " not equal to "),
        ("\\infty", " l'infini ", " infinity "),
        ("\\pi", " pi ", " pi "),
        ("\\theta", " thêta ", " theta "),
        ("\\alpha", " alpha ", " alpha "),
        ("\\beta", " bêta ", " beta "),
        ("\\lambda", " lambda ", " lambda "),
        ("\\int", " intégrale ", " integral "),
        ("\\sum", " somme ", " sum "),
        ("=", " égale ", " equals "),
        ("+", " plus ", " plus "),
        ("<", " inférieur à ", " less than "),
        (">", " supérieur à ", " greater than "),
    ];
    for (from, to_fr, to_en) in words {
        s = s.replace(from, if fr { to_fr } else { to_en });
    }
    // reste du LaTeX : commandes inconnues et accolades retirées
    let mut clean = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                while chars.peek().is_some_and(|c| c.is_alphabetic()) {
                    chars.next();
                }
                clean.push(' ');
            }
            '{' | '}' | '_' => clean.push(' '),
            c => clean.push(c),
        }
    }
    format!(" {} ", clean)
}

/// `a}reste` → ("a", "reste") en tenant compte des accolades imbriquées
fn take_group(s: &str) -> Option<(String, &str)> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some((s[..i].to_string(), &s[i + 1..])),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maths_prononcees() {
        assert_eq!(spoken_text("La dérivée de $x^2$ est $2x$.", "FR"), "La dérivée de x au carré est 2x .");
        assert_eq!(spoken_text("$\\frac{a}{b}$ and $\\sqrt{2}$", "EN"), "a over b and square root of 2");
        assert_eq!(spoken_text("Angle $\\theta$", "FR"), "Angle thêta");
    }

    #[test]
    fn fichiers_stables() {
        let a = file_for("FR", "Bonjour");
        assert!(a.starts_with("FR/") && a.ends_with(".wav"));
        assert_eq!(a, file_for("FR", "Bonjour"));
        assert_ne!(a, file_for("FR", "Bonsoir"));
        assert_ne!(a.split('/').nth(1), file_for("EN", "Bonjour").split('/').nth(1));
    }

    #[test]
    fn pauses() {
        let line = |t: f64, next: f64, file: &str| Line { t, line: 1, text: String::new(), file: file.into(), next };
        // phrase de 3 s à t=1, commentaire suivant à t=2 → pause de 2.25 s à t=2
        let lines = [line(1.0, 2.0, "a"), line(2.0, 10.0, "b")];
        let h = Holds::new(&lines, |f| Some(if f == "a" { 3.0 } else { 1.0 }));
        assert_eq!(h.list(), &[(2.0, 2.0 + BREATH)]);
        assert_eq!(h.to_timeline(1.5), 1.5);
        assert!(h.to_timeline(3.0) < 2.0); // en pause, juste avant le commentaire
        assert!((h.to_timeline(5.0) - (5.0 - 2.25)).abs() < 1e-9);
        assert_eq!(h.to_real(2.0), 4.25); // le commentaire suivant arrive après la pause
        assert_eq!(h.to_real(1.0), 1.0);
        assert_eq!(h.extra(), 2.25);
    }

    #[test]
    fn phrases_par_langue() {
        let (_, tl) = crate::load(
            "[config]\nlanguages : [FR, EN]\ndefault_lang : FR\n[figures]\n[timeline]
            1.000 => { comment : [\"un\", \"one\"] }
            3.000 => { comment : [\"\", \"two\"] }",
        )
        .unwrap();
        let fr = lines(&tl, "FR");
        assert_eq!(fr.len(), 1);
        assert_eq!((fr[0].t, fr[0].text.as_str()), (1.0, "un"));
        assert_eq!(lines(&tl, "EN").len(), 2);
    }
}
