//! Expressions mathématiques pour `plot` : `sin(x)`, `x^2 + 1`, `2x`, `exp(-x/3)`, ...
//!
//! L'expression est lue une fois (compilée en arbre), puis évaluée pour chaque x.
//!
//! Supporté :
//! - nombres, `x`, constantes `pi` et `e` ;
//! - `+ - * / ^` (puissance, associative à droite), parenthèses, moins unaire ;
//! - multiplication implicite : `2x`, `3(x+1)`, `2sin(x)`, `(x+1)(x-1)` ;
//! - fonctions : sin cos tan asin acos atan sinh cosh tanh exp ln log sqrt abs floor ceil ;
//! - paramètres (`defparam`) : leur valeur est donnée à chaque évaluation.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    X,
    /// Paramètre (`defparam`), lu dans `vars` à l'évaluation
    Var(String),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(fn(f64) -> f64, Box<Expr>),
}

impl Expr {
    pub fn eval(&self, x: f64, vars: &HashMap<String, f64>) -> f64 {
        match self {
            Expr::Num(n) => *n,
            Expr::X => x,
            Expr::Var(name) => vars.get(name).copied().unwrap_or(f64::NAN),
            Expr::Neg(e) => -e.eval(x, vars),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(x, vars), b.eval(x, vars));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => a.powf(b),
                }
            }
            Expr::Call(f, e) => f(e.eval(x, vars)),
        }
    }

    /// Paramètres utilisés par l'expression
    pub fn vars(&self, out: &mut Vec<String>) {
        match self {
            Expr::Var(name) if !out.contains(name) => out.push(name.clone()),
            Expr::Neg(e) | Expr::Call(_, e) => e.vars(out),
            Expr::Bin(_, a, b) => {
                a.vars(out);
                b.vars(out);
            }
            _ => {}
        }
    }

    /// Vrai si l'expression dépend de `x`
    pub fn uses_x(&self) -> bool {
        match self {
            Expr::X => true,
            Expr::Neg(e) | Expr::Call(_, e) => e.uses_x(),
            Expr::Bin(_, a, b) => a.uses_x() || b.uses_x(),
            _ => false,
        }
    }
}

/// Lit une expression ; l'erreur explique ce qui ne va pas
pub fn parse(src: &str) -> Result<Expr, String> {
    parse_with(src, &[])
}

/// Comme `parse`, en reconnaissant aussi les noms de paramètres `params`
pub fn parse_with(src: &str, params: &[String]) -> Result<Expr, String> {
    let chars: Vec<char> = src.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = P { c: &chars, i: 0, params };
    let e = p.sum()?;
    if p.i < chars.len() {
        return Err(format!("expression '{}' : caractère inattendu '{}'", src, chars[p.i]));
    }
    Ok(e)
}

/// Noms reconnus (fonctions, variable, constantes)
const NAMES: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "exp", "ln", "log",
    "sqrt", "abs", "floor", "ceil", "x", "pi", "e",
];

/// Noms réservés par les expressions : un paramètre ne peut pas s'appeler ainsi
pub fn is_reserved(name: &str) -> bool {
    NAMES.contains(&name)
}

struct P<'a> {
    c: &'a [char],
    i: usize,
    params: &'a [String],
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    // somme := produit (('+' | '-') produit)*
    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.product()?));
        }
        Ok(e)
    }

    // produit := unaire (('*' | '/' | implicite) unaire)*
    fn product(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            match self.peek() {
                Some(op @ ('*' | '/')) => {
                    self.i += 1;
                    e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
                }
                // Multiplication implicite : 2x, 2(x+1), 2sin(x), (x)(x)
                Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '(' => {
                    e = Expr::Bin('*', Box::new(e), Box::new(self.power()?));
                }
                _ => return Ok(e),
            }
        }
    }

    // unaire := '-' unaire | puissance
    fn unary(&mut self) -> Result<Expr, String> {
        if self.peek() == Some('-') {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.peek() == Some('+') {
            self.i += 1;
        }
        self.power()
    }

    // puissance := atome ('^' unaire)?   (2^-x permis, et a^b^c = a^(b^c))
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.atom()?;
        if self.peek() == Some('^') {
            self.i += 1;
            return Ok(Expr::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some('(') => {
                self.i += 1;
                let e = self.sum()?;
                if self.peek() != Some(')') {
                    return Err("parenthèse fermante manquante".to_string());
                }
                self.i += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.') {
                    self.i += 1;
                }
                let s: String = self.c[start..self.i].iter().collect();
                s.parse().map(Expr::Num).map_err(|_| format!("nombre invalide '{}'", s))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                // Un paramètre écrit en entier (il peut contenir des chiffres et des _)
                let word: String =
                    self.c[self.i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '_').collect();
                if self.params.contains(&word) {
                    self.i += word.chars().count();
                    return Ok(Expr::Var(word));
                }
                // Plus long nom connu au début : « sinx » → sin, « xsin » → x puis sin, « kx » → k puis x
                let rest: String = self.c[self.i..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
                let builtin = NAMES.iter().copied();
                let params = self.params.iter().map(|p| p.as_str());
                let known = builtin.chain(params).filter(|n| rest.starts_with(n)).max_by_key(|n| n.len());
                match known {
                    Some(name) => {
                        self.i += name.len();
                        let name = name.to_string();
                        self.name(&name)
                    }
                    None => Err(format!("nom inconnu '{}' (variable : x ; constantes : pi, e)", rest)),
                }
            }
            Some(c) => Err(format!("caractère inattendu '{}'", c)),
            None => Err("expression incomplète".to_string()),
        }
    }

    /// Nom lu : fonction (suivie de parenthèses), x, pi, e ; ou suite de x/pi/e collés (« xx »)
    fn name(&mut self, name: &str) -> Result<Expr, String> {
        let func: Option<fn(f64) -> f64> = match name {
            "sin" => Some(f64::sin),
            "cos" => Some(f64::cos),
            "tan" => Some(f64::tan),
            "asin" => Some(f64::asin),
            "acos" => Some(f64::acos),
            "atan" => Some(f64::atan),
            "sinh" => Some(f64::sinh),
            "cosh" => Some(f64::cosh),
            "tanh" => Some(f64::tanh),
            "exp" => Some(f64::exp),
            "ln" => Some(f64::ln),
            "log" => Some(f64::log10),
            "sqrt" => Some(f64::sqrt),
            "abs" => Some(f64::abs),
            "floor" => Some(f64::floor),
            "ceil" => Some(f64::ceil),
            _ => None,
        };
        if let Some(f) = func {
            if self.peek() != Some('(') {
                return Err(format!("{} attend des parenthèses : {}(x)", name, name));
            }
            return Ok(Expr::Call(f, Box::new(self.atom()?)));
        }
        match name {
            "x" => Ok(Expr::X),
            "pi" => Ok(Expr::Num(std::f64::consts::PI)),
            "e" => Ok(Expr::Num(std::f64::consts::E)),
            _ if self.params.iter().any(|p| p == name) => Ok(Expr::Var(name.to_string())),
            _ => Err(format!("nom inconnu '{}' (variable : x ; constantes : pi, e)", name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(src: &str, x: f64) -> f64 {
        parse(src).unwrap().eval(x, &HashMap::new())
    }

    #[test]
    fn operations() {
        assert_eq!(f("x^2 + 1", 3.0), 10.0);
        assert_eq!(f("2x", 4.0), 8.0);
        assert_eq!(f("3(x+1)", 1.0), 6.0);
        assert_eq!(f("-x^2", 2.0), -4.0); // le moins s'applique après la puissance
        assert_eq!(f("2^3^2", 0.0), 512.0); // associativité à droite
        assert_eq!(f("2^-1", 0.0), 0.5);
        assert_eq!(f("(x+1)(x-1)", 3.0), 8.0);
        assert!((f("sin(pi/2)", 0.0) - 1.0).abs() < 1e-12);
        assert!((f("2sin(x)", std::f64::consts::FRAC_PI_2) - 2.0).abs() < 1e-12);
        assert!((f("exp(-x/3)", 0.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn parametres() {
        let names = vec!["k".to_string(), "a_1".to_string()];
        let vars: HashMap<String, f64> = [("k".to_string(), 3.0), ("a_1".to_string(), 2.0)].into();
        let f = |src: &str, x: f64| parse_with(src, &names).unwrap().eval(x, &vars);
        assert_eq!(f("kx", 2.0), 6.0); // multiplication implicite
        assert_eq!(f("2k + a_1", 0.0), 8.0);
        assert_eq!(f("x^k", 2.0), 8.0);
        assert!((f("sin(k*x)", 0.5) - 1.5f64.sin()).abs() < 1e-12);
        assert_eq!(f("a_1*k", 0.0), 6.0);
        let mut used = Vec::new();
        parse_with("sin(kx)", &names).unwrap().vars(&mut used);
        assert_eq!(used, ["k"]);
        assert!(parse_with("sin(kx)", &names).unwrap().uses_x());
        assert!(!parse_with("sin(k)", &names).unwrap().uses_x());
        // sans le paramètre déclaré : nom inconnu
        assert!(parse("k + 1").is_err());
    }

    #[test]
    fn erreurs() {
        assert!(parse("sin x").unwrap_err().contains("parenthèses"));
        assert!(parse("y + 1").unwrap_err().contains("nom inconnu 'y'"));
        assert_eq!(f("xsin(x)", 0.0), 0.0); // x · sin(x)
        assert!(parse("(x+1").unwrap_err().contains("parenthèse"));
        assert!(parse("x +").is_err());
    }
}
