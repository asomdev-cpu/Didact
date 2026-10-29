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
use std::collections::HashSet;
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
}

#[wasm_bindgen]
impl Player {
    /// Charge le texte d'un fichier `.dct`. En cas d'erreur, l'exception JavaScript
    /// contient tous les messages, un par ligne (« ligne 12 : ... »).
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<Player, JsError> {
        let (resolved, timeline) = crate::load(source).map_err(|errors| JsError::new(&errors.join("\n")))?;
        Ok(Player {
            timeline,
            config: resolved.config.clone(),
            backend: None,
            pending_warnings: Vec::new(),
            seen_warnings: HashSet::new(),
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

    /// Avertissements apparus depuis le dernier appel (chaque message une seule fois)
    #[wasm_bindgen(js_name = takeWarnings)]
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}
