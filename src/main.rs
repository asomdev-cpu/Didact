mod lexer;
mod ast;
mod parser;
mod validator;
mod resolver;
mod runtime;
mod renderer;
mod output;

use std::env;
use std::fs;

const USAGE: &str = "Usage: didact <fichier.dct> [options]

Sans option : ouvre une fenêtre et joue l'animation.

Options :
  --png <t> <image.png>   enregistre l'image à l'instant t (en secondes), sans fenêtre
  --summary               affiche les figures et la timeline dans la console, sans fenêtre
  --lang <XX>             langue des commentaires (ex: --lang EN), sinon default_lang
  --debug                 affiche aussi les tokens et l'AST complets";

fn main() {
    // ── Arguments ──
    let args: Vec<String> = env::args().skip(1).collect();
    let mut filename = None;
    let mut lang: Option<String> = None;
    let mut debug = false;
    let mut summary = false;
    let mut png: Option<(f64, String)> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--debug" => debug = true,
            "--summary" => summary = true,
            "--lang" => {
                i += 1;
                lang = Some(args.get(i).cloned().unwrap_or_else(|| fail("--lang attend une langue")));
            }
            "--png" => {
                let t = args
                    .get(i + 1)
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or_else(|| fail("--png attend un temps en secondes, ex: --png 2.5 image.png"));
                let path = args
                    .get(i + 2)
                    .cloned()
                    .unwrap_or_else(|| fail("--png attend un nom de fichier, ex: --png 2.5 image.png"));
                png = Some((t, path));
                i += 2;
            }
            other if other.starts_with("--") => fail(&format!("Option inconnue '{}'\n\n{}", other, USAGE)),
            other => filename = Some(other.to_string()),
        }
        i += 1;
    }

    let filename = filename.unwrap_or_else(|| fail(USAGE));

    let content = fs::read_to_string(&filename).unwrap_or_else(|e| {
        fail(&format!("Erreur lors de la lecture du fichier '{}': {}", filename, e))
    });

    // ── Étape 1 : Lexer ──
    let tokens = lexer::tokenize(&content);
    if debug {
        println!("--- Tokens ({} trouvés) ---", tokens.len());
        for tok in &tokens {
            println!("  ligne {:>3} : {:?}", tok.line, tok.token);
        }
        println!();
    }

    // ── Étape 2 : Parser ──
    let doc = parser::parse_with_source(tokens, &content).unwrap_or_else(|e| fail(&format!("✗ Erreur de parsing : {}", e)));
    if debug {
        println!("--- Document parsé ---\n{:#?}\n", doc);
    }

    // ── Étape 3 : Validateur ──
    if let Err(errors) = validator::validate(&doc) {
        eprintln!("✗ {} erreur(s) de validation :", errors.len());
        for e in &errors {
            eprintln!("  - {}", e);
        }
        std::process::exit(1);
    }

    // ── Étape 4 : Résolveur ──
    let resolved = resolver::resolve(&doc);

    // ── Étape 5 : Runtime ──
    let timeline = runtime::Timeline::new(&resolved);

    // ── Sortie ──
    if summary {
        print_summary(&resolved, &timeline, lang.as_deref());
    } else if let Some((t, path)) = png {
        export_png(&resolved, &timeline, t, &path, lang.as_deref());
    } else {
        let title = std::path::Path::new(&filename)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(filename.clone());
        if let Err(e) = output::window::run(timeline, resolved.config.clone(), lang, &title) {
            fail(&format!("✗ Impossible d'ouvrir la fenêtre : {}", e));
        }
    }
}

/// Enregistre l'image de l'instant t en PNG
fn export_png(resolved: &resolver::ResolvedDocument, timeline: &runtime::Timeline, t: f64, path: &str, lang: Option<&str>) {
    let (w, h) = renderer::Scene::default_size(&resolved.config);
    let mut backend = renderer::skia::SkiaBackend::new(w, h).unwrap_or_else(|| fail("taille d'image invalide"));
    let scene = renderer::Scene::new(&resolved.config, w, h);
    let frame = timeline.frame_at(t, lang);
    for (name, reason) in renderer::render_frame(&frame, &scene, &mut backend) {
        eprintln!("Attention : '{}' non dessinée : {}", name, reason);
    }
    backend.save_png(path).unwrap_or_else(|e| fail(&format!("✗ Écriture de '{}' impossible : {}", path, e)));
    println!("✓ Image t={:.3}s enregistrée dans {} ({}×{})", t, path, w, h);
}

/// Résumé texte : figures résolues et état après chaque bloc de la timeline
fn print_summary(resolved: &resolver::ResolvedDocument, timeline: &runtime::Timeline, lang: Option<&str>) {
    println!("✓ Parsing, validation, résolution\n");
    println!("--- Figures (ordre de dessin) ---");
    for f in &resolved.figures {
        let mut keys: Vec<&String> = f.properties.keys().collect();
        keys.sort();
        let props: Vec<String> = keys
            .iter()
            .filter(|k| k.as_str() != "type")
            .map(|k| format!("{}={}", k, resolver::fmt_value(&f.properties[*k])))
            .collect();
        println!("  {} [{}, layer {}] : {}", f.name, f.ftype, f.layer, props.join(", "));
    }

    println!("\n--- Timeline (durée {:.3}s) ---", timeline.duration());
    for &t in timeline.times() {
        let frame = timeline.frame_at(t, lang);
        let names: Vec<&str> = frame.visible.iter().map(|f| f.name.as_str()).collect();
        println!("  t={:>8.3}  visibles : [{}]", t, names.join(", "));
        if let Some(c) = frame.comment {
            println!("              comment  : \"{}\"", c);
        }
        for a in &frame.audio {
            println!("              audio    : {} (volume {}, à {:.3}s)", a.file, a.volume, a.position);
        }
    }
}

/// Affiche un message d'erreur et quitte
fn fail(msg: &str) -> ! {
    eprintln!("{}", msg);
    std::process::exit(1);
}
