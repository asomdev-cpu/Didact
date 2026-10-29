//! Texte : point d'entrée simple vers le moteur de composition (`typeset`).
//!
//! Le texte peut contenir des maths LaTeX entre `$...$` ; un vrai retour à la ligne
//! dans la chaîne commence une nouvelle ligne.

pub use super::typeset::Align;
use super::geometry::{Rgba, Shape};
use super::typeset::{self, Content};

/// Formes d'un texte : `(x, y)` est le point d'ancrage (pixels), centré verticalement,
/// aligné horizontalement selon `align`. `size` = taille de la police en pixels.
pub fn text_shapes(content: &str, x: f32, y: f32, size: f32, align: Align, color: Rgba) -> Result<Vec<Shape>, String> {
    typeset::shapes(&Content::Text(content.to_string()), x, y, size, align, color)
}
