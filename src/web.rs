//! Interface JavaScript (WebAssembly).
//!
//! Utilisation côté JavaScript (voir web/didact.js qui l'enveloppe dans une balise) :
//!
//! ```js
//! import init, { Player } from "./pkg/didact.js";
//! await init();
//! const player = new Player(texteDuFichierDct);   // erreur JS si le fichier est invalide
//! const pixels = player.render(t, largeur, hauteur); // RGBA, à mettre dans un canvas
//! const sousTitre = player.comment(t, "FR");
//! ```
//!
//! En natif, ce module compile aussi (les fonctions ne sont simplement pas utilisées).

use crate::ast::{Config, WindowConfig};
use crate::renderer::{self, skia::SkiaBackend, Scene};
use crate::runtime::Timeline;
use crate::voice::{self, Holds};
use std::collections::{HashMap, HashSet};
use wasm_bindgen::prelude::*;

/// Un fichier Didact chargé, prêt à être joué
#[wasm_bindgen]
pub struct Player {
    timeline: Timeline,
    config: Config,
    backend: Option<SkiaBackend>,
    /// Avertissements pas encore lus par JavaScript (figures non dessinables, ...)
    pending_warnings: Vec<String>,
    seen_warnings: HashSet<String>,
    /// Pauses de l'animation par langue, pour laisser finir les voix (vide sans voix)
    holds: HashMap<String, Holds>,
    /// Fichiers des figures `image`
    images: Vec<String>,
}

#[wasm_bindgen]
impl Player {
    /// Charge le texte d'un fichier `.dct`. En cas d'erreur, l'exception JavaScript
    /// contient tous les messages, un par ligne (« ligne 12 : ... »).
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<Player, JsError> {
        let (resolved, timeline) = crate::load(source).map_err(|errors| JsError::new(&errors.join("\n")))?;
        Ok(Player {
            images: crate::renderer::image::files(&resolved.figures),
            timeline,
            config: resolved.config.clone(),
            backend: None,
            pending_warnings: Vec::new(),
            seen_warnings: HashSet::new(),
            holds: HashMap::new(),
        })
    }

    /// Durée totale en secondes
    pub fn duration(&self) -> f64 {
        self.timeline.duration()
    }

    /// Langues déclarées dans [config], dans l'ordre
    pub fn languages(&self) -> Vec<String> {
        self.config.languages.clone()
    }

    #[wasm_bindgen(js_name = defaultLanguage)]
    pub fn default_language(&self) -> String {
        self.config.default_lang.clone()
    }

    /// Rapport largeur / hauteur imposé par `window` (undefined si `auto`)
    #[wasm_bindgen(js_name = aspectRatio)]
    pub fn aspect_ratio(&self) -> Option<f64> {
        match self.config.window {
            WindowConfig::Size(w, h) if w > 0 && h > 0 => Some(w as f64 / h as f64),
            WindowConfig::Ratio(a, b) if a > 0 && b > 0 => Some(a as f64 / b as f64),
            _ => None,
        }
    }

    /// Dessine l'instant `t` dans une image `width` × `height`.
    /// Renvoie les pixels RGBA (4 octets par pixel, ligne par ligne), prêts pour `ImageData`.
    pub fn render(&mut self, t: f64, width: u32, height: u32) -> Vec<u8> {
        let (width, height) = (width.max(1), height.max(1));
        if self.backend.as_ref().map(|b| (b.width(), b.height())) != Some((width, height)) {
            self.backend = SkiaBackend::new(width, height);
        }
        let Some(backend) = self.backend.as_mut() else { return Vec::new() };

        let frame = self.timeline.frame_at(t, None);
        let scene = Scene::new(&self.config, width, height);
        for (name, reason) in renderer::render_frame(&frame, &scene, backend) {
            let msg = format!("'{}' : {}", name, reason);
            if self.seen_warnings.insert(msg.clone()) {
                self.pending_warnings.push(msg);
            }
        }

        // tiny-skia stocke des couleurs prémultipliées ; le canvas attend des couleurs « droites »
        backend
            .pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect()
    }

    /// Sous-titre à l'instant `t` dans la langue `lang` (sinon la langue par défaut)
    pub fn comment(&self, t: f64, lang: Option<String>) -> Option<String> {
        self.timeline.frame_at(t, lang.as_deref()).comment.map(|s| s.to_string())
    }

    /// Pistes audio, en JSON : `[{"file": "musique.mp3", "volume": 0.5, "start": 2, "end": 8, "loop": false}, ...]`
    /// (`end` vaut null si la piste joue jusqu'à la fin). Le lecteur web les joue avec `<audio>`.
    /// Fichiers des figures `image`, à télécharger puis à donner avec `addFile`
    #[wasm_bindgen(js_name = imageFiles)]
    pub fn image_files(&self) -> Vec<String> {
        self.images.clone()
    }

    /// Contenu d'un fichier image (téléchargé par le lecteur)
    #[wasm_bindgen(js_name = addFile)]
    pub fn add_file(&mut self, name: &str, bytes: Vec<u8>) {
        crate::renderer::image::register(name, bytes);
    }

    #[wasm_bindgen(js_name = audioSegments)]
    pub fn audio_segments(&self) -> String {
        let items: Vec<String> = self
            .timeline
            .audio_segments()
            .iter()
            .map(|s| {
                format!(
                    "{{\"file\":{},\"volume\":{},\"start\":{},\"end\":{},\"loop\":{}}}",
                    json_string(&s.file),
                    s.volume,
                    s.start,
                    s.end.map(|e| e.to_string()).unwrap_or("null".to_string()),
                    s.looping
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    /// Voix d'une langue, en JSON : `[{"file": "FR/3f2a….wav", "start": 1.5}, ...]`.
    /// Les fichiers sont relatifs au dossier des voix (`cours_voix/`, créé par `didact --voice`).
    #[wasm_bindgen(js_name = voiceSegments)]
    pub fn voice_segments(&self, lang: Option<String>) -> String {
        let index = self.timeline.lang_index(lang.as_deref());
        let lang = self.timeline.languages().get(index).cloned().unwrap_or_default();
        let items: Vec<String> = voice::lines(&self.timeline, &lang)
            .iter()
            .map(|l| format!("{{\"file\":{},\"start\":{}}}", json_string(&l.file), l.t))
            .collect();
        format!("[{}]", items.join(","))
    }

    /// Donne le contenu de `cours_voix/index.txt` (lignes « FR/….wav 3.42 ») :
    /// calcule les pauses de l'animation pour laisser finir les phrases trop longues.
    #[wasm_bindgen(js_name = setVoiceIndex)]
    pub fn set_voice_index(&mut self, index: &str) {
        let durations: HashMap<&str, f64> = index
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("commande "))
            .filter_map(|l| {
                let mut p = l.split_whitespace();
                Some((p.next()?, p.next()?.parse().ok()?))
            })
            .collect();
        self.holds = self
            .timeline
            .languages()
            .iter()
            .map(|lang| {
                let lines = voice::lines(&self.timeline, lang);
                (lang.clone(), Holds::new(&lines, |f| durations.get(f).copied()))
            })
            .collect();
    }

    fn holds_of(&self, lang: Option<String>) -> Holds {
        let index = self.timeline.lang_index(lang.as_deref());
        let lang = self.timeline.languages().get(index).cloned().unwrap_or_default();
        self.holds.get(&lang).cloned().unwrap_or_default()
    }

    /// Durée réelle (pauses comprises) dans la langue `lang`
    #[wasm_bindgen(js_name = realDuration)]
    pub fn real_duration(&self, lang: Option<String>) -> f64 {
        self.timeline.duration() + self.holds_of(lang).extra()
    }

    /// Temps réel → temps de la timeline (à donner à `render` et `comment`)
    #[wasm_bindgen(js_name = toTimeline)]
    pub fn to_timeline(&self, real: f64, lang: Option<String>) -> f64 {
        self.holds_of(lang).to_timeline(real)
    }

    /// Temps de la timeline → temps réel (début des pistes audio)
    #[wasm_bindgen(js_name = toReal)]
    pub fn to_real(&self, t: f64, lang: Option<String>) -> f64 {
        self.holds_of(lang).to_real(t)
    }

    /// Avertissements apparus depuis le dernier appel (chaque message une seule fois)
    #[wasm_bindgen(js_name = takeWarnings)]
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

/// Chaîne JSON entre guillemets
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pistes_en_json() {
        let p = Player::new(
            "[figures]\n[timeline]\n1.000 => { start : [audio musique.mp3 volume=0.5] }\n3.000 => { end : [audio musique.mp3] }",
        )
        .unwrap_or_else(|_| panic!("chargement"));
        assert_eq!(p.audio_segments(), r#"[{"file":"musique.mp3","volume":0.5,"start":1,"end":3,"loop":false}]"#);
        assert_eq!(json_string("x\"y"), r#""x\"y""#);
    }
}
