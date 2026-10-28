//! Moteur d'exécution : calcule l'état de la scène à un instant donné.
//!
//! Pour l'instant (Phase 1) : uniquement la visibilité (start / end / destroy),
//! les commentaires et l'audio. Les animations viendront dans `animation.rs`.

pub mod timeline;

pub use timeline::Timeline;
