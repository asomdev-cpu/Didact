mod lexer;
mod ast;
mod parser;

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

    println!("=== Didact Parser v0.1 ===");
    println!("Fichier : {}\n", filename);

    // Étape 1 : Lexer
    let tokens = lexer::tokenize(&content);
    println!("--- Tokens ({} trouvés) ---", tokens.len());
    for tok in &tokens {
        println!("  {:?}", tok);
    }

    // Étape 2 : Parser
    println!("\n--- Parsing ---");
    match parser::parse(tokens) {
        Ok(doc) => {
            println!("✓ Parsing réussi !\n");
            println!("--- Document parsé ---");
            println!("{:#?}", doc);
        }
        Err(e) => {
            eprintln!("✗ Erreur de parsing : {}", e);
            std::process::exit(1);
        }
    }
}