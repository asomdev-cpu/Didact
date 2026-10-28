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

/// Un token accompagné du numéro de ligne (à partir de 1) où il commence.
/// Permet d'afficher "ligne 12 : ..." dans les messages d'erreur.
#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub line: usize,
}

fn push(tokens: &mut Vec<Spanned>, line: usize, token: Token) {
    tokens.push(Spanned { token, line });
}

/// Vrai si `chars` (positionné sur un #) commence une couleur hexadécimale :
/// 3, 6 ou 8 chiffres hexa, suivis d'un caractère qui ne prolonge pas un mot.
/// Ex : `#FF0000` → couleur ; `# commentaire` ou `#titre` → commentaire.
fn is_hex_color(chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    chars.next(); // le #
    let mut n = 0;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_hexdigit() {
            n += 1;
            chars.next();
        } else if c.is_alphanumeric() || c == '_' {
            return false; // ex: #fabrique → mot, donc commentaire
        } else {
            break;
        }
    }
    matches!(n, 3 | 6 | 8)
}

/// Transforme le texte brut en liste de tokens (avec leur ligne)
pub fn tokenize(input: &str) -> Vec<Spanned> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    let mut line = 1;

    while let Some(&c) = chars.peek() {
        // Ligne où commence le token courant
        let start = line;
        match c {
            // --- Espaces et sauts de ligne ignorés ---
            ' ' | '\t' | '\r' | '\n' => {
                if c == '\n' {
                    line += 1;
                }
                chars.next();
            }

            // --- Couleur hexadécimale : #RGB, #RRGGBB ou #RRGGBBAA ---
            // (sinon, # est un commentaire)
            '#' if is_hex_color(&mut chars.clone()) => {
                let mut color = String::new();
                color.push(c);
                chars.next();
                while let Some(&h) = chars.peek() {
                    if h.is_ascii_hexdigit() {
                        color.push(h);
                        chars.next();
                    } else {
                        break;
                    }
                }
                push(&mut tokens, start, Token::Ident(color));
            }

            // --- Commentaires : tout jusqu'à la fin de la ligne ---
            '#' => {
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        line += 1;
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
                    if c == '\n' {
                        line += 1;
                    }
                    s.push(c);
                }
                push(&mut tokens, start, Token::StringLit(s));
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
                    push(&mut tokens, start, Token::Percentage(format!("{}%", num)));
                } else {
                    push(&mut tokens, start, Token::Number(num.parse().unwrap_or(0.0)));
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
                    push(&mut tokens, start, Token::Number(num.parse().unwrap_or(0.0)));
                } else {
                    // Juste un tiret seul, on l'ignore
                }
            }

            // --- => ou = ---
            '=' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    push(&mut tokens, start, Token::Arrow);
                } else {
                    push(&mut tokens, start, Token::Equals);
                }
            }

            // --- Symboles simples ---
            ':' => { chars.next(); push(&mut tokens, start, Token::Colon); }
            '{' => { chars.next(); push(&mut tokens, start, Token::LBrace); }
            '}' => { chars.next(); push(&mut tokens, start, Token::RBrace); }
            '[' => { chars.next(); push(&mut tokens, start, Token::LBracket); }
            ']' => { chars.next(); push(&mut tokens, start, Token::RBracket); }
            '(' => { chars.next(); push(&mut tokens, start, Token::LParen); }
            ')' => { chars.next(); push(&mut tokens, start, Token::RParen); }
            ',' => { chars.next(); push(&mut tokens, start, Token::Comma); }
            '|' => { chars.next(); push(&mut tokens, start, Token::Pipe); }

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
                push(&mut tokens, start, Token::Ident(ident));
            }

            // --- Caractères inconnus ignorés ---
            _ => {
                chars.next();
            }
        }
    }

    tokens.push(Spanned { token: Token::EOF, line });
    tokens
}
#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str) -> Vec<Token> {
        tokenize(src).into_iter().map(|s| s.token).collect()
    }

    #[test]
    fn couleur_hex_vs_commentaire() {
        assert_eq!(
            tokens("color : #FF0000 # rouge vif"),
            [Token::Ident("color".into()), Token::Colon, Token::Ident("#FF0000".into()), Token::EOF]
        );
        assert_eq!(tokens("#fab"), [Token::Ident("#fab".into()), Token::EOF]);
        // mot qui commence par des lettres hexa → commentaire
        assert_eq!(tokens("#fabrique"), [Token::EOF]);
        assert_eq!(tokens("# --- Styles ---"), [Token::EOF]);
    }

    #[test]
    fn numeros_de_ligne() {
        let t = tokenize("a\n# commentaire\n\nb");
        assert_eq!(t[0].line, 1);
        assert_eq!(t[1].line, 4);
    }
}
