//! Renderer : transforme l'état de la scène à l'instant t en dessin.
//!
//! Deux étages :
//! - `geometry` : figure Didact → formes en pixels (chemins + remplissage + trait).
//!   Indépendant de la bibliothèque de dessin.
//! - un backend (`RenderBackend`) qui dessine ces formes. Pour l'instant : `skia`
//!   (tiny-skia, dessin sur processeur dans une image en mémoire).
//!
//! Changer de bibliothèque de dessin = écrire un autre backend, sans toucher au reste.

pub mod geometry;
pub mod skia;

pub use geometry::{Rgba, Scene, Shape};

use crate::runtime::timeline::Frame;

/// Ce qu'une bibliothèque de dessin doit savoir faire
pub trait RenderBackend {
    /// Remplit toute l'image avec une couleur
    fn clear(&mut self, color: Rgba);
    /// Dessine une forme (remplissage puis trait)
    fn draw(&mut self, shape: &Shape);
}

/// Dessine une frame complète.
/// Retourne les figures visibles qui n'ont pas pu être dessinées, avec la raison
/// (type pas encore supporté, référence dynamique, ...).
pub fn render_frame(frame: &Frame, scene: &Scene, backend: &mut dyn RenderBackend) -> Vec<(String, String)> {
    backend.clear(scene.background);
    let mut skipped = Vec::new();

    // frame.visible est déjà dans l'ordre de dessin (layer croissant)
    for fig in &frame.visible {
        match geometry::figure_shapes(fig, scene) {
            Ok(shapes) => {
                for shape in &shapes {
                    backend.draw(shape);
                }
            }
            Err(reason) => skipped.push((fig.name.clone(), reason)),
        }
    }
    skipped
}
