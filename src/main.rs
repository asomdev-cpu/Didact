mod lexer;
mod ast;
mod parser;
mod validator;
mod resolver;
mod runtime;

use std::env;
use std::fs;

const USAGE: &str = "Usage: didact <fichier.dct> [--lang XX] [--debug]
  --lang XX   langue des commentaires (ex: --lang EN), sinon default_lang
  --debug     affiche aussi les tokens et l'AST complets";

fn main() {
    // ── Arguments ──
    let args: Vec<String> = env::args().skip(1).collect();
    let mut filename = None;
    let mut lang: Option<String> = None;
    let mut debug = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--debug" => debug = true,
            "--lang" => {
                i += 1;
                match args.get(i) {
                    Some(l) => lang = Some(l.clone()),
                    None => fail(&format!("--lang attend une langue\n{}", USAGE)),
                }
            }
            other if other.starts_with("--") => fail(&format!("Option inconnue '{}'\n{}", other, USAGE)),
            other => filename = Some(other.to_string()),
        }
        i += 1;
    }

    let filename = filename.unwrap_or_else(|| fail(USAGE));

    let content = fs::read_to_string(&filename).unwrap_or_else(|e| {
        fail(&format!("Erreur lors de la lecture du fichier '{}': {}", filename, e))
    });

    println!("=== Didact v0.2 ===");
    println!("Fichier : {}\n", filename);

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
    let doc = parser::parse(tokens).unwrap_or_else(|e| fail(&format!("✗ Erreur de parsing : {}", e)));
    println!("✓ Parsing");
    if debug {
        println!("\n--- Document parsé ---\n{:#?}\n", doc);
    }

    // ── Étape 3 : Validateur ──
    if let Err(errors) = validator::validate(&doc) {
        eprintln!("✗ {} erreur(s) de validation :", errors.len());
        for e in &errors {
            eprintln!("  - {}", e);
        }
        std::process::exit(1);
    }
    println!("✓ Validation");

    // ── Étape 4 : Résolveur ──
    let resolved = resolver::resolve(&doc);
    println!("✓ Résolution\n");
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

    // ── Étape 5 : Runtime (simulation texte, en attendant le renderer) ──
    let timeline = runtime::Timeline::new(&resolved);
    println!("\n--- Timeline (durée {:.3}s) ---", timeline.duration());
    for &t in timeline.times() {
        let frame = timeline.frame_at(t, lang.as_deref());
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
