//! Sorties natives : fenêtre desktop (avec le son), export MP4, export HTML autonome.
//! (Le lecteur web lui-même est dans `web.rs`, compilé en WASM.)

pub mod audio;
pub mod html;
pub mod reload;
pub mod speech;
pub mod video;
pub mod window;
