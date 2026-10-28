mod lexer;
mod ast;
mod parser;
mod validator;
mod resolver;

use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: didact <fichier.dct>");
        eprintln!("Exemple: didact mon_cours.dct");
        std::process::exit(1);
    }

    let filename = &args[1];

    let content = match fs::read_to_string(filename) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Erreur lors de la lecture du fichier '{}': {}", filename, e);
            std::process::exit(1);
        }
    };

    println!("=== Didact Parser v0.2 ===");
    println!("Fichier : {}\n", filename);

    // Étape 1 : Lexer
    let tokens = lexer::tokenize(&content);
    println!("--- Tokens ({} trouvés) ---", tokens.len());
    for tok in &tokens {
        println!("  ligne {:>3} : {:?}", tok.line, tok.token);
    }

    // Étape 2 : Parser
    println!("\n--- Parsing ---");
    match parser::parse(tokens) {
        Ok(doc) => {
            println!("✓ Parsing réussi !\n");
            println!("--- Document parsé ---");
            println!("{:#?}", doc);

            // Étape 3 : Validateur
            println!("\n--- Validation ---");
            match validator::validate(&doc) {
                Ok(()) => {
                    println!("✓ Validation réussie !");

                    // Étape 4 : Résolveur
                    println!("\n--- Résolution (figures dans l'ordre de dessin) ---");
                    let resolved = resolver::resolve(&doc);
                    for f in &resolved.figures {
                        let mut keys: Vec<&String> = f.properties.keys().collect();
                        keys.sort();
                        let props: Vec<String> = keys
                            .iter()
                            .map(|k| format!("{}={}", k, resolver::fmt_value(&f.properties[*k])))
                            .collect();
                        println!("  {} [{}, layer {}] : {}", f.name, f.ftype, f.layer, props.join(", "));
                    }
                }
                Err(errors) => {
                    eprintln!("✗ {} erreur(s) de validation :", errors.len());
                    for e in &errors {
                        eprintln!("  - {}", e);
                    }
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Erreur de parsing : {}", e);
            std::process::exit(1);
        }
    }
}