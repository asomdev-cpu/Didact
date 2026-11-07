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

/// Direction pour slidein / slideout / move(direction, distance, durée)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    fn parse(s: &str) -> Option<Dir> {
        match s {
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            "up" | "top" => Some(Dir::Up),
            "down" | "bottom" => Some(Dir::Down),
            _ => None,
        }
    }

    /// Vecteur unité en pixels (y pixel vers le bas)
    pub fn unit(self) -> (f32, f32) {
        match self {
            Dir::Left => (-1.0, 0.0),
            Dir::Right => (1.0, 0.0),
            Dir::Up => (0.0, -1.0),
            Dir::Down => (0.0, 1.0),
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
    /// Arrive depuis `from` (glissé court + fondu)
    SlideIn { from: Dir },
    /// Part vers `to` (glissé court + fondu)
    SlideOut { to: Dir },
    /// Se dessine progressivement (contour tracé, puis remplissage)
    Draw,
    /// S'écrit progressivement, morceau par morceau (texte, équations)
    Write,
    /// Le centre de la figure va vers `to` (position en texte : "(80%,50%)" ou "center(b)")
    Move { to: String },
    /// Se déplace de `distance` % (de la largeur pour left/right, de la hauteur pour up/down)
    MoveBy { dir: Dir, distance: f64 },
    /// Se transforme en la figure `to`, qui la remplace à la fin
    Transform { to: String },
    /// La couleur du trait va vers `to` ; avec `part`, seulement ce morceau d'un texte ou d'une équation
    Color { to: String, part: Option<String> },
    /// La taille va vers ce facteur (1 = taille d'origine)
    Scale(f64),
    /// Tourne de cet angle en degrés (positif = sens trigonométrique)
    Rotate(f64),
    /// Le centre parcourt le contour d'une figure (de son début à sa fin, à vitesse régulière) ;
    /// `tangent` : la figure s'oriente selon la tangente
    Follow { path: String, tangent: bool },
    /// Un paramètre (`defparam`) va vers cette valeur
    To(f64),
}

impl AnimKind {
    /// Animations d'apparition / disparition : elles ne comptent plus après un nouveau `start`
    pub fn is_entry_or_exit(&self) -> bool {
        matches!(
            self,
            AnimKind::FadeIn
                | AnimKind::FadeOut
                | AnimKind::Grow
                | AnimKind::Shrink
                | AnimKind::SlideIn { .. }
                | AnimKind::SlideOut { .. }
                | AnimKind::Draw
                | AnimKind::Write
                | AnimKind::Transform { .. }
        )
    }
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

    let named_value = |key: &str| named.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string());

    let kind = match name {
        "fadein" => AnimKind::FadeIn,
        "fadeout" => AnimKind::FadeOut,
        "grow" => AnimKind::Grow,
        "shrink" => AnimKind::Shrink,
        "draw" => AnimKind::Draw,
        "write" => AnimKind::Write,
        "slidein" | "slideout" => {
            let dir = positional
                .first()
                .and_then(|d| Dir::parse(d))
                .ok_or_else(|| format!("{} attend une direction (left, right, up, down), ex : {}(left, 1s)", name, name))?;
            if name == "slidein" { AnimKind::SlideIn { from: dir } } else { AnimKind::SlideOut { to: dir } }
        }
        "move" => match named_value("to") {
            Some(to) => AnimKind::Move { to },
            None if !positional.is_empty() => {
                const FORM: &str = "move(direction, distance, durée), ex : move(down, 20%, 2s)";
                let dir = Dir::parse(positional[0])
                    .ok_or_else(|| format!("direction inconnue '{}' (left, right, up, down) : {}", positional[0], FORM))?;
                let distance = positional
                    .get(1)
                    .and_then(|d| d.strip_suffix('%'))
                    .and_then(|d| d.trim().parse::<f64>().ok())
                    .ok_or_else(|| format!("distance manquante ou invalide (en %) : {}", FORM))?;
                AnimKind::MoveBy { dir, distance }
            }
            None => return Err("move attend une destination : move(to=(x%, y%), durée) ou move(down, 20%, durée)".to_string()),
        },
        "transform" => match named_value("to") {
            Some(to) => AnimKind::Transform { to },
            None => return Err("transform attend une figure : transform(to=autre_figure, durée)".to_string()),
        },
        "color" => match named_value("to") {
            Some(to) => AnimKind::Color { to, part: named_value("part") },
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
        "to" => AnimKind::To(parse_number(positional.first(), "to attend une valeur : to(3, durée)")?),
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
        assert!(parse_call(&call("move", &[])).unwrap_err().contains("destination"));
        assert_eq!(
            parse_call(&call("move", &["down", "20%", "2s"])).unwrap().0,
            AnimKind::MoveBy { dir: Dir::Down, distance: 20.0 }
        );
        assert!(parse_call(&call("move", &["down", "3cm/s"])).unwrap_err().contains("move(down, 20%, 2s)"));
        assert_eq!(parse_call(&call("slidein", &["left", "1s"])).unwrap().0, AnimKind::SlideIn { from: Dir::Left });
        assert!(parse_call(&call("slideout", &["dehors", "1s"])).is_err());
        assert_eq!(
            parse_call(&call("transform", &["to=b", "2s"])).unwrap().0,
            AnimKind::Transform { to: "b".into() }
        );
        assert!(parse_call(&call("fadein", &["1s", "ease=rapide"])).is_err());
        assert_eq!(
            parse_call(&call("follow", &["path=c", "10s", "rotate=tangent"])).unwrap().0,
            AnimKind::Follow { path: "c".into(), tangent: true }
        );
        assert!(parse_call(&call("follow", &["path=c", "speed=1cm/s"])).unwrap_err().contains("durée"));
        assert_eq!(parse_call(&call("draw", &["1s"])).unwrap().0, AnimKind::Draw);
        assert!(parse_call(&call("write", &[])).unwrap_err().contains("durée manquante"));
    }

    #[test]
    fn progression() {
        let a = Anim { kind: AnimKind::FadeIn, start: 2.0, duration: 2.0, ease: Ease::Linear, line: 1 };
        assert_eq!(a.progress(1.0), 0.0);
        assert_eq!(a.progress(3.0), 0.5);
        assert_eq!(a.progress(10.0), 1.0);
    }
}
