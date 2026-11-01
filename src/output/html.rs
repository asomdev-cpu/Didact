//! Export HTML autonome : un seul fichier .html qui contient tout
//! (la source .dct, le lecteur `didact.js`, et le moteur WebAssembly compressé).
//! Il s'ouvre d'un double-clic, sans serveur et sans connexion.
//!
//! Le moteur WASM n'est pas dans l'exécutable : il faut l'avoir compilé une fois
//! (`.\web\build.ps1`, qui crée `web/pkg/`).

use base64::Engine as _;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Le lecteur web, inclus dans l'exécutable à la compilation
const PLAYER_JS: &str = include_str!("../../web/didact.js");

/// Fichiers du moteur produits par wasm-bindgen (`web/pkg/`)
const GLUE: &str = "didact.js";
const WASM: &str = "didact_bg.wasm";

/// Réglages de l'export
pub struct HtmlOptions {
    pub output: PathBuf,
    pub title: String,
    /// Langue des sous-titres au démarrage (sinon default_lang)
    pub lang: Option<String>,
    /// Dossier du moteur (sinon recherché : ./web/pkg, ./pkg, puis à côté de l'exécutable)
    pub engine: Option<PathBuf>,
    /// Largeur / hauteur de la scène (pour dimensionner le lecteur dans la page)
    pub ratio: f64,
}

pub struct HtmlReport {
    pub engine_dir: PathBuf,
    /// Taille du fichier écrit (octets)
    pub size: usize,
}

/// Cherche le dossier `pkg` du moteur
pub fn find_engine(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let ok = |dir: &Path| dir.join(GLUE).is_file() && dir.join(WASM).is_file();
    if let Some(dir) = explicit {
        return if ok(dir) {
            Ok(dir.to_path_buf())
        } else {
            Err(format!("{} ou {} introuvable dans {}", GLUE, WASM, dir.display()))
        };
    }
    let mut candidates = vec![PathBuf::from("web/pkg"), PathBuf::from("pkg")];
    // target/release/didact → ../../web/pkg
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            candidates.push(dir.join("web").join("pkg"));
        }
    }
    candidates.into_iter().find(|d| ok(d)).ok_or_else(|| {
        "moteur web introuvable (web/pkg/didact_bg.wasm).\n\
         Compilez-le une fois depuis la racine du projet : .\\web\\build.ps1\n\
         ou indiquez son dossier avec --engine <dossier>"
            .to_string()
    })
}

/// Écrit la page autonome
pub fn export(source: &str, opts: &HtmlOptions) -> Result<HtmlReport, String> {
    let engine_dir = find_engine(opts.engine.as_deref())?;
    let read = |name: &str| {
        let path = engine_dir.join(name);
        std::fs::read(&path).map_err(|e| format!("lecture de {} impossible : {}", path.display(), e))
    };
    let glue = gzip_base64(&read(GLUE)?)?;
    let wasm = gzip_base64(&read(WASM)?)?;

    let page = page(source, &glue, &wasm, opts);
    std::fs::write(&opts.output, &page)
        .map_err(|e| format!("écriture de {} impossible : {}", opts.output.display(), e))?;
    Ok(HtmlReport { engine_dir, size: page.len() })
}

fn gzip_base64(bytes: &[u8]) -> Result<String, String> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    gz.write_all(bytes).map_err(|e| e.to_string())?;
    let compressed = gz.finish().map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(compressed))
}

/// La page HTML. Les blocs `<script type="application/gzip">` ne sont pas exécutés :
/// ce sont des données (base64), lues par `__DIDACT_ENGINE__`.
fn page(source: &str, glue_b64: &str, wasm_b64: &str, opts: &HtmlOptions) -> String {
    let lang_attr = opts.lang.as_ref().map(|l| format!(" lang=\"{}\"", escape_attr(l))).unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="Didact">
<title>{title}</title>
<style>
  html, body {{ margin: 0; height: 100%; background: #111; }}
  body {{ display: grid; place-items: center; }}
  didact-player {{ width: min(100vw, calc((100vh - 40px) * {ratio:.5})); }}
</style>
<script>
// Moteur Didact inclus dans la page (décompressé au chargement)
globalThis.__DIDACT_ENGINE__ = async () => {{
  const unzip = async (id) => {{
    const b64 = document.getElementById(id).textContent.trim();
    const response = await fetch("data:application/gzip;base64," + b64);
    const stream = response.body.pipeThrough(new DecompressionStream("gzip"));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  }};
  const glue = new Blob([await unzip("didact-glue")], {{ type: "text/javascript" }});
  const url = URL.createObjectURL(glue);
  const engine = await import(url);
  URL.revokeObjectURL(url);
  await engine.default({{ module_or_path: await unzip("didact-wasm") }});
  return engine;
}};
</script>
<script type="module">
{player}
</script>
</head>
<body>
<didact-player{lang}>
{source}
</didact-player>
<script type="application/gzip" id="didact-glue">{glue}</script>
<script type="application/gzip" id="didact-wasm">{wasm}</script>
</body>
</html>
"#,
        title = escape_text(&opts.title),
        ratio = if opts.ratio.is_finite() && opts.ratio > 0.0 { opts.ratio } else { 16.0 / 9.0 },
        player = inline_js(PLAYER_JS),
        lang = lang_attr,
        source = source_script(source),
        glue = glue_b64,
        wasm = wasm_b64,
    )
}

/// La source dans un `<script type="text/didact">` : telle quelle si possible (lisible),
/// sinon en base64 (si elle contient `</script` ou `<!--`, qui casseraient la page)
fn source_script(source: &str) -> String {
    let lower = source.to_lowercase();
    if lower.contains("</script") || lower.contains("<!--") {
        format!(
            "<script type=\"text/didact\" data-encoding=\"base64\">{}</script>",
            base64::engine::general_purpose::STANDARD.encode(source.as_bytes())
        )
    } else {
        format!("<script type=\"text/didact\">\n{}\n</script>", source)
    }
}

/// Du JavaScript placé dans un `<script>` : `</script` (même dans un commentaire)
/// fermerait la balise trop tôt. `<\/` est équivalent à `</` dans une chaîne JS.
fn inline_js(js: &str) -> String {
    js.replace("</script", "<\\/script").replace("<!--", "<\\!--")
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_lisible_ou_base64() {
        assert!(source_script("[figures]\n").contains("<script type=\"text/didact\">\n[figures]"));
        let s = source_script("comment : [\"</script> piège\"]");
        assert!(s.contains("data-encoding=\"base64\""));
        assert!(!s.contains("piège"));
    }

    #[test]
    fn page_complete() {
        let opts = HtmlOptions {
            output: "x.html".into(),
            title: "Cours <1>".into(),
            lang: Some("EN".into()),
            engine: None,
            ratio: 16.0 / 9.0,
        };
        let p = page("[figures]", "R0xVRQ==", "V0FTTQ==", &opts);
        assert!(p.contains("<title>Cours &lt;1&gt;</title>"));
        assert!(p.contains("<didact-player lang=\"EN\">"));
        assert!(p.contains("__DIDACT_ENGINE__"));
        assert!(p.contains("customElements.define(\"didact-player\""));
        assert!(p.contains("id=\"didact-wasm\">V0FTTQ==</script>"));
        // un seul </script> par bloc : celui du lecteur ne doit pas fermer la balise trop tôt
        let module = p.split("<script type=\"module\">").nth(1).unwrap();
        assert!(module.split("</script>").next().unwrap().contains("customElements.define"));
    }

    #[test]
    fn moteur_absent() {
        let err = find_engine(Some(Path::new("/dossier/inexistant"))).unwrap_err();
        assert!(err.contains("introuvable"));
    }
}
