//! Animations : lecture des appels (`move(to=(80%, 50%), 2s)`), easing, progression.
//!
//! Une animation a un début, une durée et une courbe d'easing. À l'instant t,
//! sa progression va de 0 (pas commencée) à 1 (terminée) ; une animation terminée
//! garde son effet (une figure déplacée reste à sa nouvelle position).
//!
//! L'application concrète (déplacer, colorer, ...) est faite par le renderer,
//! qui connaît la géométrie des figures.

use crate::ast::AnimCall;

/// Courbe d'easing : transforme une progression linéaire x ∈ [0, 1]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ease {
    /// Vitesse constante
    Linear,
    /// Démarre lentement
    EaseIn,
    /// Finit lentement
    EaseOut,
    /// Démarre et finit lentement (défaut, style 3Blue1Brown)
    Smooth,
}

impl Ease {
    pub fn apply(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Ease::Linear => x,
            Ease::EaseIn => x * x * x,
            Ease::EaseOut => 1.0 - (1.0 - x).powi(3),
            Ease::Smooth => x * x * (3.0 - 2.0 * x),
        }
    }

    fn parse(s: &str) -> Result<Ease, String> {
        match s {
            "linear" => Ok(Ease::Linear),
            "ease_in" => Ok(Ease::EaseIn),
            "ease_out" => Ok(Ease::EaseOut),
            "smooth" => Ok(Ease::Smooth),
            other => Err(format!(
                "easing inconnu '{}' (linear, ease_in, ease_out, smooth)",
                other
            )),
        }
    }
}

/// Ce que fait une animation
#[derive(Debug, Clone, PartialEq)]
pub enum AnimKind {
    /// Opacité 0 → 1
    FadeIn,
    /// Opacité 1 → 0
    FadeOut,
    /// Taille 0 → 1
    Grow,
    /// Taille 1 → 0
    Shrink,
    /// Le centre de la figure va vers `to` (position en texte : "(80%,50%)" ou "center(b)")
    Move { to: String },
    /// La couleur du trait va vers `to`
    Color { to: String },
    /// La taille va vers ce facteur (1 = taille d'origine)
    Scale(f64),
    /// Tourne de cet angle en degrés (positif = sens trigonométrique)
    Rotate(f64),
    /// Le centre parcourt le contour d'une figure (de son début à sa fin, à vitesse régulière) ;
    /// `tangent` : la figure s'oriente selon la tangente
    Follow { path: String, tangent: bool },
    /// Prévue par la spec mais pas encore implémentée (ignorée, avec un avertissement)
    Unsupported(String),
}

/// Une animation planifiée sur la timeline
#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    pub kind: AnimKind,
    /// Instant de début (secondes)
    pub start: f64,
    pub duration: f64,
    pub ease: Ease,
    /// Ligne de l'appel dans le fichier
    pub line: usize,
}

impl Anim {
    /// Progression à l'instant t, après easing : 0 avant le début, 1 après la fin
    pub fn progress(&self, t: f64) -> f64 {
        if t <= self.start {
            0.0
        } else if self.duration <= 0.0 || t >= self.start + self.duration {
            1.0
        } else {
            self.ease.apply((t - self.start) / self.duration)
        }
    }

    pub fn end(&self) -> f64 {
        self.start + self.duration
    }
}

/// Animations de la spec pas encore implémentées
const NOT_YET: &[&str] = &["slidein", "slideout", "write", "draw", "transform"];

/// Lit un appel d'animation. Renvoie (ce qu'elle fait, durée, easing).
pub fn parse_call(call: &AnimCall) -> Result<(AnimKind, f64, Ease), String> {
    let name = call.name.as_str();
    let mut ease = Ease::Smooth;
    let mut duration: Option<f64> = None;
    let mut named: Vec<(&str, &str)> = Vec::new();
    let mut positional: Vec<&str> = Vec::new();

    for p in &call.params {
        if let Some((k, v)) = p.split_once('=') {
            match k {
                "ease" => ease = Ease::parse(v)?,
                "duration" => duration = Some(parse_duration(v).ok_or_else(|| format!("durée invalide '{}'", v))?),
                _ => named.push((k, v)),
            }
        } else if let Some(d) = parse_duration(p) {
            duration = Some(d);
        } else {
            positional.push(p);
        }
    }

    if NOT_YET.contains(&name) {
        return Ok((AnimKind::Unsupported(name.to_string()), duration.unwrap_or(0.0), ease));
    }

    let named_value = |key: &str| named.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string());

    let kind = match name {
        "fadein" => AnimKind::FadeIn,
        "fadeout" => AnimKind::FadeOut,
        "grow" => AnimKind::Grow,
        "shrink" => AnimKind::Shrink,
        "move" => match named_value("to") {
            Some(to) => AnimKind::Move { to },
            // move(down, 3cm/s) : forme direction + vitesse, pas encore implémentée
            None if !positional.is_empty() => {
                return Ok((AnimKind::Unsupported("move(direction, vitesse)".to_string()), duration.unwrap_or(0.0), ease))
            }
            None => return Err("move attend une destination : move(to=(x%, y%), durée)".to_string()),
        },
        "color" => match named_value("to") {
            Some(to) => AnimKind::Color { to },
            None => return Err("color attend une couleur : color(to=red, durée)".to_string()),
        },
        "scale" => AnimKind::Scale(parse_number(positional.first(), "scale attend un facteur : scale(2, durée)")?),
        "follow" => {
            if named_value("speed").is_some() {
                return Err("follow : utiliser une durée, ex : follow(path=courbe, 5s)".to_string());
            }
            let path = named_value("path").ok_or("follow attend un chemin : follow(path=figure, durée)")?;
            let tangent = match named_value("rotate").as_deref() {
                None | Some("none") => false,
                Some("tangent") => true,
                Some(other) => return Err(format!("follow : rotate={} invalide (tangent ou none)", other)),
            };
            AnimKind::Follow { path, tangent }
        }
        "rotate" => AnimKind::Rotate(parse_number(
            positional.first(),
            "rotate attend un angle en degrés : rotate(90, durée)",
        )?),
        other => return Err(format!("animation inconnue '{}'", other)),
    };

    let duration = duration.ok_or_else(|| format!("{} : durée manquante (ex : 1s ou 500ms)", name))?;
    Ok((kind, duration, ease))
}

/// "2s" → 2.0, "500ms" → 0.5
fn parse_duration(s: &str) -> Option<f64> {
    if let Some(ms) = s.strip_suffix("ms") {
        ms.parse::<f64>().ok().map(|v| v / 1000.0)
    } else if let Some(sec) = s.strip_suffix('s') {
        sec.parse::<f64>().ok()
    } else {
        None
    }
}

fn parse_number(s: Option<&&str>, err: &str) -> Result<f64, String> {
    s.and_then(|v| v.trim_end_matches("deg").parse::<f64>().ok())
        .ok_or_else(|| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, params: &[&str]) -> AnimCall {
        AnimCall { name: name.into(), params: params.iter().map(|s| s.to_string()).collect(), line: 1 }
    }

    #[test]
    fn easing() {
        for e in [Ease::Linear, Ease::EaseIn, Ease::EaseOut, Ease::Smooth] {
            assert_eq!(e.apply(0.0), 0.0);
            assert_eq!(e.apply(1.0), 1.0);
        }
        assert_eq!(Ease::Smooth.apply(0.5), 0.5);
        assert!(Ease::EaseIn.apply(0.5) < 0.5);
        assert!(Ease::EaseOut.apply(0.5) > 0.5);
    }

    #[test]
    fn lecture_des_appels() {
        assert_eq!(parse_call(&call("fadein", &["1s"])).unwrap(), (AnimKind::FadeIn, 1.0, Ease::Smooth));
        assert_eq!(
            parse_call(&call("move", &["to=(80%,50%)", "500ms", "ease=linear"])).unwrap(),
            (AnimKind::Move { to: "(80%,50%)".into() }, 0.5, Ease::Linear)
        );
        assert_eq!(parse_call(&call("rotate", &["-45", "2s"])).unwrap().0, AnimKind::Rotate(-45.0));
        assert!(parse_call(&call("fadein", &[])).unwrap_err().contains("durée manquante"));
        assert!(parse_call(&call("move", &["2s"])).unwrap_err().contains("destination"));
        assert!(parse_call(&call("fadein", &["1s", "ease=rapide"])).is_err());
        assert_eq!(
            parse_call(&call("follow", &["path=c", "10s", "rotate=tangent"])).unwrap().0,
            AnimKind::Follow { path: "c".into(), tangent: true }
        );
        assert!(parse_call(&call("follow", &["path=c", "speed=1cm/s"])).unwrap_err().contains("durée"));
        assert_eq!(parse_call(&call("draw", &["1s"])).unwrap().0, AnimKind::Unsupported("draw".into()));
    }

    #[test]
    fn progression() {
        let a = Anim { kind: AnimKind::FadeIn, start: 2.0, duration: 2.0, ease: Ease::Linear, line: 1 };
        assert_eq!(a.progress(1.0), 0.0);
        assert_eq!(a.progress(3.0), 0.5);
        assert_eq!(a.progress(10.0), 1.0);
    }
}
