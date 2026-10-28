//! Moteur d'exécution : calcule l'état de la scène à un instant donné.
//!
//! - `timeline` : visibilité (start / end / destroy), commentaires, audio, planification des animations
//! - `animation` : lecture des appels d'animation, easing, progression

pub mod animation;
pub mod timeline;

pub use timeline::Timeline;
