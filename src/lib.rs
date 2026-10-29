//! Didact — moteur du langage d'animations éducatives.
//!
//! Chaîne : lexer → parser → validateur → résolveur → runtime → renderer.
//!
//! - En natif, le programme `didact` (src/main.rs) ajoute la fenêtre et les exports.
//! - En WebAssembly, le module `web` expose un lecteur à JavaScript.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod renderer;
pub mod resolver;
pub mod runtime;
pub mod validator;
pub mod web;

/// Fenêtre desktop (natif seulement)
#[cfg(not(target_arch = "wasm32"))]
pub mod output;

/// Charge un source Didact : lexer → parser → validateur → résolveur → timeline.
/// En cas d'erreur, renvoie la liste des messages (avec numéros de ligne).
pub fn load(source: &str) -> Result<(resolver::ResolvedDocument, runtime::Timeline), Vec<String>> {
    let tokens = lexer::tokenize(source);
    let doc = parser::parse_with_source(tokens, source).map_err(|e| vec![e])?;
    validator::validate(&doc)?;
    let resolved = resolver::resolve(&doc);
    let timeline = runtime::Timeline::new(&resolved);
    Ok((resolved, timeline))
}
