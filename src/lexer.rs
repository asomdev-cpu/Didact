/// Les tokens du langage Didact
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // --- Valeurs ---
    Number(f64),         // 0.000  61.500  -3.14
    Percentage(String),  // 50%  20%  100%
    StringLit(String),   // "bonjour"
    Ident(String),       // ma_ligne  auto  red  sin(x)

    // --- Symboles ---
    Arrow,      // =>   (timestamp vers bloc)
    Equals,     // =    (définition d'un objet)
    Colon,      // :    (assignation d'une propriété)
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    LParen,     // (
    RParen,     // )
    Comma,      // ,
    Pipe,       // |    (bloc multilignes)

    // --- Fin ---
    EOF,
}

/// Transforme le texte brut en liste de tokens
pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            // --- Espaces et sauts de ligne ignorés ---
            ' ' | '\t' | '\r' | '\n' => {
                chars.next();
            }

            // --- Commentaires : tout jusqu'à la fin de la ligne ---
            '#' => {
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        break;
                    }
                }
            }

            // --- Chaînes de caractères "..." ---
            '"' => {
                chars.next(); // consomme le "
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                tokens.push(Token::StringLit(s));
            }

            // --- Nombres (et pourcentages) ---
            '0'..='9' => {
                let mut num = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' {
                        num.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // Si suivi de %, c'est un pourcentage
                if chars.peek() == Some(&'%') {
                    chars.next();
                    tokens.push(Token::Percentage(format!("{}%", num)));
                } else {
                    tokens.push(Token::Number(num.parse().unwrap_or(0.0)));
                }
            }

            // --- Nombres négatifs ---
            '-' => {
                chars.next();
                if chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    let mut num = String::from("-");
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() || c == '.' {
                            num.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    tokens.push(Token::Number(num.parse().unwrap_or(0.0)));
                } else {
                    // Juste un tiret seul, on l'ignore
                }
            }

            // --- => ou = ---
            '=' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token::Arrow);
                } else {
                    tokens.push(Token::Equals);
                }
            }

            // --- Symboles simples ---
            ':' => { chars.next(); tokens.push(Token::Colon); }
            '{' => { chars.next(); tokens.push(Token::LBrace); }
            '}' => { chars.next(); tokens.push(Token::RBrace); }
            '[' => { chars.next(); tokens.push(Token::LBracket); }
            ']' => { chars.next(); tokens.push(Token::RBracket); }
            '(' => { chars.next(); tokens.push(Token::LParen); }
            ')' => { chars.next(); tokens.push(Token::RParen); }
            ',' => { chars.next(); tokens.push(Token::Comma); }
            '|' => { chars.next(); tokens.push(Token::Pipe); }

            // --- Identifiants ---
            // Peuvent contenir : lettres, chiffres, _, ., /, \, ^ (pour LaTeX)
            'a'..='z' | 'A'..='Z' | '_' | '\\' => {
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric()
                        || c == '_'
                        || c == '.'
                        || c == '/'
                        || c == '\\'
                        || c == '^'
                        || c == '{'  // pour les expressions LaTeX inline
                        || c == '}'
                        || c == '+'
                        || c == '*'
                        || c == '!'
                    {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Ident(ident));
            }

            // --- Caractères inconnus ignorés ---
            _ => {
                chars.next();
            }
        }
    }

    tokens.push(Token::EOF);
    tokens
}