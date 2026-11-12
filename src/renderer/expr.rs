//! Expressions mathématiques : `sin(x)`, `x^2 + 1`, `2x`, `sum(k, 1, n, sin(k*x)/k)`, ...
//!
//! L'expression est lue une fois (compilée en arbre), puis évaluée autant de fois que nécessaire.
//!
//! Supporté :
//! - nombres, `x`, constantes `pi` et `e` ;
//! - `+ - * / ^` (puissance, associative à droite), parenthèses, moins unaire ;
//! - comparaisons `< > <= >= == !=` (valent 1 ou 0) ;
//! - multiplication implicite : `2x`, `3(x+1)`, `2sin(x)`, `(x+1)(x-1)` ;
//! - fonctions : voir `FUNCTIONS` (dont `min`, `max`, `if(cond, a, b)`, `mod`, `atan2`, `clamp`) ;
//! - sommes et produits : `sum(k, 1, n, expr)`, `prod(k, 1, n, expr)`. Si `n` n'est pas entier
//!   (paramètre animé), le dernier terme d'une somme compte pour sa partie fractionnaire :
//!   les termes apparaissent en douceur ;
//! - paramètres (`defparam`) : leur valeur est donnée à chaque évaluation ;
//! - nombres complexes (`parse_complex`) : la variable est `z`, `i` est l'unité imaginaire.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    /// La variable : x (réel) ou z (complexe)
    X,
    /// Unité imaginaire i (complexe seulement)
    I,
    /// Paramètre (`defparam`) ou variable locale, lu dans `vars` à l'évaluation
    Var(String),
    /// Variable d'une somme ou d'un produit (indice dans la pile)
    Bound(usize),
    Neg(Box<Expr>),
    /// + - * / ^ ; comparaisons : < > l (<=) g (>=) = (==) ! (!=)
    Bin(char, Box<Expr>, Box<Expr>),
    /// Fonction, par nom
    Call(&'static str, Vec<Expr>),
    /// sum(k, a, b, corps) ou prod(...)
    Sum { prod: bool, from: Box<Expr>, to: Box<Expr>, body: Box<Expr> },
}

/// Fonctions : nom, nombre d'arguments (min, max ; 0 = illimité), disponible en complexe
pub(crate) const FUNCTIONS: &[(&str, usize, usize, bool)] = &[
    ("sin", 1, 1, true),
    ("cos", 1, 1, true),
    ("tan", 1, 1, true),
    ("asin", 1, 1, false),
    ("acos", 1, 1, false),
    ("atan", 1, 1, false),
    ("sinh", 1, 1, true),
    ("cosh", 1, 1, true),
    ("tanh", 1, 1, true),
    ("exp", 1, 1, true),
    ("ln", 1, 1, true),
    ("log", 1, 1, true),
    ("sqrt", 1, 1, true),
    ("abs", 1, 1, true),
    ("floor", 1, 1, false),
    ("ceil", 1, 1, false),
    ("round", 1, 1, false),
    ("sign", 1, 1, false),
    ("atan2", 2, 2, false),
    ("mod", 2, 2, false),
    ("hypot", 2, 2, false),
    ("min", 1, 0, false),
    ("max", 1, 0, false),
    ("clamp", 3, 3, false),
    ("if", 3, 3, true),
    ("and", 2, 0, false),
    ("or", 2, 0, false),
    ("not", 1, 1, false),
    ("re", 1, 1, true),
    ("im", 1, 1, true),
    ("arg", 1, 1, true),
    ("conj", 1, 1, true),
    ("rand", 1, 2, false),
    ("randn", 1, 2, false),
    ("fact", 1, 1, false),
    ("binom", 2, 2, false),
    ("gamma", 1, 1, false),
    ("erf", 1, 1, false),
];

/// Limite de termes d'une somme (protège d'une boucle sans fin)
const MAX_TERMS: i64 = 100_000;

// ─────────────────────────────────────────────
//  ÉVALUATION RÉELLE
// ─────────────────────────────────────────────

impl Expr {
    pub fn eval(&self, x: f64, vars: &HashMap<String, f64>) -> f64 {
        self.ev(x, vars, &mut Vec::new())
    }

    fn ev(&self, x: f64, vars: &HashMap<String, f64>, stack: &mut Vec<f64>) -> f64 {
        match self {
            Expr::Num(n) => *n,
            Expr::X => x,
            Expr::I => f64::NAN,
            Expr::Var(name) => vars.get(name).copied().unwrap_or(f64::NAN),
            Expr::Bound(i) => stack.get(*i).copied().unwrap_or(f64::NAN),
            Expr::Neg(e) => -e.ev(x, vars, stack),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.ev(x, vars, stack), b.ev(x, vars, stack));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    '^' => a.powf(b),
                    '<' => bool_f(a < b),
                    '>' => bool_f(a > b),
                    'l' => bool_f(a <= b),
                    'g' => bool_f(a >= b),
                    '=' => bool_f(a == b),
                    _ => bool_f(a != b),
                }
            }
            Expr::Call(name, args) => {
                // if : seule la branche choisie est calculée
                if *name == "if" {
                    let c = args[0].ev(x, vars, stack);
                    return if c != 0.0 && !c.is_nan() { args[1].ev(x, vars, stack) } else { args[2].ev(x, vars, stack) };
                }
                let v: Vec<f64> = args.iter().map(|a| a.ev(x, vars, stack)).collect();
                call_real(name, &v)
            }
            Expr::Sum { prod, from, to, body } => {
                let (a, b) = (from.ev(x, vars, stack).round(), to.ev(x, vars, stack));
                if !a.is_finite() || !b.is_finite() {
                    return f64::NAN;
                }
                let last = b.floor();
                let mut acc = if *prod { 1.0 } else { 0.0 };
                let mut k = a;
                let mut n = 0;
                stack.push(0.0);
                let slot = stack.len() - 1;
                while k <= last && n < MAX_TERMS {
                    stack[slot] = k;
                    let t = body.ev(x, vars, stack);
                    if *prod { acc *= t } else { acc += t }
                    k += 1.0;
                    n += 1;
                }
                // Partie fractionnaire de la borne : le terme suivant entre en douceur
                let frac = b - last;
                if !*prod && frac > 0.0 && k == last + 1.0 && n < MAX_TERMS {
                    stack[slot] = k;
                    acc += frac * body.ev(x, vars, stack);
                }
                stack.pop();
                acc
            }
        }
    }

    /// Paramètres utilisés par l'expression
    pub fn vars(&self, out: &mut Vec<String>) {
        match self {
            Expr::Var(name) if !out.contains(name) => out.push(name.clone()),
            Expr::Neg(e) => e.vars(out),
            Expr::Call(_, args) => args.iter().for_each(|a| a.vars(out)),
            Expr::Bin(_, a, b) => {
                a.vars(out);
                b.vars(out);
            }
            Expr::Sum { from, to, body, .. } => {
                from.vars(out);
                to.vars(out);
                body.vars(out);
            }
            _ => {}
        }
    }

    /// Vrai si l'expression dépend de `x` (ou de `z`)
    pub fn uses_x(&self) -> bool {
        match self {
            Expr::X => true,
            Expr::Neg(e) => e.uses_x(),
            Expr::Call(_, args) => args.iter().any(|a| a.uses_x()),
            Expr::Bin(_, a, b) => a.uses_x() || b.uses_x(),
            Expr::Sum { from, to, body, .. } => from.uses_x() || to.uses_x() || body.uses_x(),
            _ => false,
        }
    }
}

fn bool_f(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

pub(crate) fn call_real(name: &str, v: &[f64]) -> f64 {
    match name {
        "sin" => v[0].sin(),
        "cos" => v[0].cos(),
        "tan" => v[0].tan(),
        "asin" => v[0].asin(),
        "acos" => v[0].acos(),
        "atan" => v[0].atan(),
        "sinh" => v[0].sinh(),
        "cosh" => v[0].cosh(),
        "tanh" => v[0].tanh(),
        "exp" => v[0].exp(),
        "ln" => v[0].ln(),
        "log" => v[0].log10(),
        "sqrt" => v[0].sqrt(),
        "abs" => v[0].abs(),
        "floor" => v[0].floor(),
        "ceil" => v[0].ceil(),
        "round" => v[0].round(),
        "sign" => {
            if v[0] > 0.0 { 1.0 } else if v[0] < 0.0 { -1.0 } else { 0.0 }
        }
        "atan2" => v[0].atan2(v[1]),
        "mod" => v[0].rem_euclid(v[1]),
        "hypot" => v[0].hypot(v[1]),
        "min" => v.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "clamp" => v[0].max(v[1]).min(v[2]),
        "and" => bool_f(v.iter().all(|a| *a != 0.0)),
        "or" => bool_f(v.iter().any(|a| *a != 0.0)),
        "not" => bool_f(v[0] == 0.0),
        "re" | "conj" => v[0],
        "im" => 0.0,
        "arg" => {
            if v[0] < 0.0 { std::f64::consts::PI } else { 0.0 }
        }
        "rand" => uniform(v[0], v.get(1).copied().unwrap_or(0.0), 0),
        "randn" => {
            // Box–Muller : deux tirages uniformes indépendants
            let seed = v.get(1).copied().unwrap_or(0.0);
            let (u1, u2) = (uniform(v[0], seed, 1).max(1e-300), uniform(v[0], seed, 2));
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        }
        "fact" => gamma(v[0] + 1.0),
        "gamma" => gamma(v[0]),
        "binom" => {
            let (n, k) = (v[0], v[1]);
            if k < 0.0 || k > n {
                0.0
            } else {
                (ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)).exp().round()
            }
        }
        "erf" => erf(v[0]),
        _ => f64::NAN,
    }
}

/// Tirage uniforme dans [0, 1[ déterminé par (k, graine, flux) : même formule, même valeur.
/// `rand(k)` pour k = 1, 2, 3, ... donne une suite de tirages indépendants (splitmix64).
fn uniform(k: f64, seed: f64, stream: u64) -> f64 {
    // Entiers : leur valeur exacte ; sinon les bits du nombre
    let bits = |v: f64| if v.fract() == 0.0 && v.abs() < 9e15 { v as i64 as u64 } else { v.to_bits() };
    let mix = |mut x: u64| {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^ (x >> 31)
    };
    let x = mix(mix(mix(bits(k)) ^ bits(seed)) ^ stream);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// ln Γ(x), x > 0 (Lanczos, g = 7)
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin().abs()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let t = x + 7.5;
    let mut a = G[0];
    for (i, g) in G.iter().enumerate().skip(1) {
        a += g / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// Γ(x) (et Γ(n + 1) = n!)
fn gamma(x: f64) -> f64 {
    if x <= 0.0 && x.fract() == 0.0 {
        return f64::NAN;
    }
    if x.fract() == 0.0 && x <= 171.0 {
        return (1..x as u64).fold(1.0, |a, k| a * k as f64);
    }
    let pi = std::f64::consts::PI;
    if x < 0.5 {
        return pi / ((pi * x).sin() * gamma(1.0 - x));
    }
    ln_gamma(x).exp()
}

/// Fonction d'erreur (précision ~1e-7)
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t + 0.254_829_592) * t * (-x * x).exp();
    if x >= 0.0 { y } else { -y }
}

/// Formule compilée : une fonction des variables `slots` (dans l'ordre), les paramètres
/// devenant des constantes. Bien plus rapide que `eval` pour des millions d'évaluations.
pub type Compiled = Box<dyn Fn(&[f64]) -> f64 + Send + Sync>;

/// Compile `e` ; `x` (la variable des formules) est la variable nommée "x" de `slots`
pub fn compile(e: &Expr, slots: &[String], params: &HashMap<String, f64>) -> Result<Compiled, String> {
    compile_at(e, slots, params, 0)
}

/// `depth` : nombre de sommes englobantes ; leurs variables suivent `slots` dans le tableau
fn compile_at(e: &Expr, slots: &[String], params: &HashMap<String, f64>, depth: usize) -> Result<Compiled, String> {
    let slot = |name: &str| slots.iter().position(|s| s == name);
    let c = |e: &Expr| compile_at(e, slots, params, depth);
    Ok(match e {
        Expr::Num(n) => {
            let n = *n;
            Box::new(move |_| n)
        }
        Expr::X => match slot("x") {
            Some(i) => Box::new(move |v| v[i]),
            None => return Err("x n'est pas une variable ici".to_string()),
        },
        Expr::Var(name) => match slot(name) {
            Some(i) => Box::new(move |v| v[i]),
            None => {
                let n = params.get(name).copied().unwrap_or(f64::NAN);
                Box::new(move |_| n)
            }
        },
        Expr::Bound(i) => {
            if *i >= depth {
                return Err("variable de somme hors de sa somme".to_string());
            }
            let at = slots.len() + i;
            Box::new(move |v| v[at])
        }
        Expr::Neg(a) => {
            let a = c(a)?;
            Box::new(move |v| -a(v))
        }
        Expr::Bin(op, a, b) => {
            let (a, b) = (c(a)?, c(b)?);
            match op {
                '+' => Box::new(move |v| a(v) + b(v)),
                '-' => Box::new(move |v| a(v) - b(v)),
                '*' => Box::new(move |v| a(v) * b(v)),
                '/' => Box::new(move |v| a(v) / b(v)),
                '^' => Box::new(move |v| a(v).powf(b(v))),
                op => {
                    let op = *op;
                    Box::new(move |v| {
                        let (p, q) = (a(v), b(v));
                        bool_f(match op {
                            '<' => p < q,
                            '>' => p > q,
                            'l' => p <= q,
                            'g' => p >= q,
                            '=' => p == q,
                            _ => p != q,
                        })
                    })
                }
            }
        }
        Expr::Call(name, args) => {
            let fs = args.iter().map(&c).collect::<Result<Vec<_>, _>>()?;
            let name: &'static str = name;
            if name == "if" {
                let mut it = fs.into_iter();
                let (cond, a, b) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
                Box::new(move |v| {
                    let k = cond(v);
                    if k != 0.0 && !k.is_nan() { a(v) } else { b(v) }
                })
            } else if fs.len() == 1 {
                let a = fs.into_iter().next().unwrap();
                Box::new(move |v| call_real(name, &[a(v)]))
            } else {
                Box::new(move |v| {
                    let args: Vec<f64> = fs.iter().map(|f| f(v)).collect();
                    call_real(name, &args)
                })
            }
        }
        // Somme, produit : même règle que `eval` (borne fractionnaire : dernier terme partiel)
        Expr::Sum { prod, from, to, body } => {
            let (from, to) = (c(from)?, c(to)?);
            let body = compile_at(body, slots, params, depth + 1)?;
            let prod = *prod;
            let width = slots.len() + depth;
            Box::new(move |v| {
                let (a, b) = (from(v).round(), to(v));
                if !a.is_finite() || !b.is_finite() {
                    return f64::NAN;
                }
                let last = b.floor();
                let mut buf: Vec<f64> = v[..width].to_vec();
                buf.push(0.0);
                let mut acc = if prod { 1.0 } else { 0.0 };
                let (mut k, mut n) = (a, 0);
                while k <= last && n < MAX_TERMS {
                    buf[width] = k;
                    let t = body(&buf);
                    if prod { acc *= t } else { acc += t }
                    k += 1.0;
                    n += 1;
                }
                let frac = b - last;
                if !prod && frac > 0.0 && k == last + 1.0 && n < MAX_TERMS {
                    buf[width] = k;
                    acc += frac * body(&buf);
                }
                acc
            })
        }
        Expr::I => return Err("i n'existe qu'en complexe".to_string()),
    })
}

// ─────────────────────────────────────────────
//  NOMBRES COMPLEXES
// ─────────────────────────────────────────────

/// Nombre complexe (re, im)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct C(pub f64, pub f64);

impl C {
    pub(crate) fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    pub(crate) fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
    pub(crate) fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    pub(crate) fn div(self, o: C) -> C {
        let d = o.0 * o.0 + o.1 * o.1;
        C((self.0 * o.0 + self.1 * o.1) / d, (self.1 * o.0 - self.0 * o.1) / d)
    }
    pub(crate) fn abs(self) -> f64 {
        self.0.hypot(self.1)
    }
    pub(crate) fn exp(self) -> C {
        let r = self.0.exp();
        C(r * self.1.cos(), r * self.1.sin())
    }
    pub(crate) fn ln(self) -> C {
        C(self.abs().ln(), self.1.atan2(self.0))
    }
    pub(crate) fn pow(self, o: C) -> C {
        // Puissance entière : multiplications exactes (z^2 reste exact en 0)
        if o.1 == 0.0 && o.0.fract() == 0.0 && o.0.abs() <= 64.0 {
            let mut r = C(1.0, 0.0);
            for _ in 0..o.0.abs() as u32 {
                r = r.mul(self);
            }
            return if o.0 < 0.0 { C(1.0, 0.0).div(r) } else { r };
        }
        if self.0 == 0.0 && self.1 == 0.0 {
            return C(0.0, 0.0);
        }
        o.mul(self.ln()).exp()
    }
    pub(crate) fn sin(self) -> C {
        C(self.0.sin() * self.1.cosh(), self.0.cos() * self.1.sinh())
    }
    pub(crate) fn cos(self) -> C {
        C(self.0.cos() * self.1.cosh(), -self.0.sin() * self.1.sinh())
    }
    pub(crate) fn sinh(self) -> C {
        C(self.0.sinh() * self.1.cos(), self.0.cosh() * self.1.sin())
    }
    pub(crate) fn cosh(self) -> C {
        C(self.0.cosh() * self.1.cos(), self.0.sinh() * self.1.sin())
    }
}

/// Fonction d'une variable complexe
pub(crate) fn call_c(name: &str, v: C) -> C {
    let real = |x: f64| C(x, 0.0);
    match name {
        "sin" => v.sin(),
        "cos" => v.cos(),
        "tan" => v.sin().div(v.cos()),
        "sinh" => v.sinh(),
        "cosh" => v.cosh(),
        "tanh" => v.sinh().div(v.cosh()),
        "exp" => v.exp(),
        "ln" => v.ln(),
        "log" => v.ln().div(real(std::f64::consts::LN_10)),
        "sqrt" => v.pow(real(0.5)),
        "abs" => real(v.abs()),
        "re" => real(v.0),
        "im" => real(v.1),
        "arg" => real(v.1.atan2(v.0)),
        "conj" => C(v.0, -v.1),
        _ => real(f64::NAN),
    }
}

impl Expr {
    /// Évalue en complexe : `z` est la variable, les paramètres sont réels
    pub fn eval_c(&self, z: C, vars: &HashMap<String, f64>) -> C {
        self.ev_c(z, vars, &mut Vec::new())
    }

    fn ev_c(&self, z: C, vars: &HashMap<String, f64>, stack: &mut Vec<f64>) -> C {
        let real = |v: f64| C(v, 0.0);
        match self {
            Expr::Num(n) => real(*n),
            Expr::X => z,
            Expr::I => C(0.0, 1.0),
            Expr::Var(name) => real(vars.get(name).copied().unwrap_or(f64::NAN)),
            Expr::Bound(i) => real(stack.get(*i).copied().unwrap_or(f64::NAN)),
            Expr::Neg(e) => {
                let v = e.ev_c(z, vars, stack);
                C(-v.0, -v.1)
            }
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.ev_c(z, vars, stack), b.ev_c(z, vars, stack));
                match op {
                    '+' => a.add(b),
                    '-' => a.sub(b),
                    '*' => a.mul(b),
                    '/' => a.div(b),
                    '^' => a.pow(b),
                    // Comparaisons : sur les parties réelles
                    c => {
                        let r = Expr::Bin(*c, Box::new(Expr::Num(a.0)), Box::new(Expr::Num(b.0)));
                        real(r.ev(0.0, vars, stack))
                    }
                }
            }
            Expr::Call(name, args) => {
                if *name == "if" {
                    let c = args[0].ev_c(z, vars, stack);
                    return if c.0 != 0.0 { args[1].ev_c(z, vars, stack) } else { args[2].ev_c(z, vars, stack) };
                }
                call_c(name, args[0].ev_c(z, vars, stack))
            }
            Expr::Sum { prod, from, to, body } => {
                let (a, b) = (from.ev(0.0, vars, stack).round(), to.ev(0.0, vars, stack));
                if !a.is_finite() || !b.is_finite() {
                    return real(f64::NAN);
                }
                let last = b.floor();
                let mut acc = if *prod { real(1.0) } else { real(0.0) };
                let mut k = a;
                let mut n = 0;
                stack.push(0.0);
                let slot = stack.len() - 1;
                while k <= last && n < MAX_TERMS {
                    stack[slot] = k;
                    let t = body.ev_c(z, vars, stack);
                    acc = if *prod { acc.mul(t) } else { acc.add(t) };
                    k += 1.0;
                    n += 1;
                }
                let frac = b - last;
                if !*prod && frac > 0.0 && k == last + 1.0 && n < MAX_TERMS {
                    stack[slot] = k;
                    acc = acc.add(real(frac).mul(body.ev_c(z, vars, stack)));
                }
                stack.pop();
                acc
            }
        }
    }
}

// ─────────────────────────────────────────────
//  LECTURE
// ─────────────────────────────────────────────

// ─────────────────────────────────────────────
//  FONCTIONS DE L'UTILISATEUR (deffunction)
// ─────────────────────────────────────────────

/// `deffunction f(x, y) = corps` : arguments et corps (texte)
struct UserFn {
    params: Vec<String>,
    body: String,
}

fn functions() -> &'static std::sync::RwLock<HashMap<String, std::sync::Arc<UserFn>>> {
    static F: std::sync::OnceLock<std::sync::RwLock<HashMap<String, std::sync::Arc<UserFn>>>> = std::sync::OnceLock::new();
    F.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

/// Oublie les fonctions (nouveau document)
pub fn clear_functions() {
    functions().write().unwrap().clear();
}

/// Définit (ou redéfinit) une fonction ; le corps est vérifié avec ses arguments et les
/// paramètres `params`
pub fn define_function(name: &str, args: &[String], body: &str, params: &[String]) -> Result<(), String> {
    if is_reserved(name) || FUNCTIONS.iter().any(|f| f.0 == name) {
        return Err(format!("'{}' est un nom réservé des formules : choisir un autre nom", name));
    }
    for a in args {
        if a != "x" && is_reserved(a) {
            return Err(format!("argument '{}' : nom réservé des formules", a));
        }
    }
    let mut names: Vec<String> = args.to_vec();
    names.extend(params.iter().filter(|p| !args.contains(p)).cloned());
    // Vérifie le corps sans la fonction elle-même (pas de récursion)
    parse_with(body, &names).map_err(|e| format!("{}({}) = {} : {}", name, args.join(", "), body, e))?;
    functions()
        .write()
        .unwrap()
        .insert(name.to_string(), std::sync::Arc::new(UserFn { params: args.to_vec(), body: body.to_string() }));
    Ok(())
}

/// Le texte de toutes les fonctions (le cache de calcul en dépend)
pub fn functions_text() -> String {
    let f = functions().read().unwrap();
    let mut v: Vec<String> = f.iter().map(|(n, u)| format!("{}({})={}", n, u.params.join(","), u.body)).collect();
    v.sort();
    v.join(";")
}

impl Expr {
    /// Remplace les variables `Var(nom)` par des expressions
    fn subst(&self, map: &HashMap<String, Expr>) -> Expr {
        let b = |e: &Expr| Box::new(e.subst(map));
        match self {
            Expr::Var(n) => map.get(n).cloned().unwrap_or_else(|| self.clone()),
            Expr::Neg(a) => Expr::Neg(b(a)),
            Expr::Bin(op, l, r) => Expr::Bin(*op, b(l), b(r)),
            Expr::Call(name, args) => Expr::Call(name, args.iter().map(|a| a.subst(map)).collect()),
            Expr::Sum { prod, from, to, body } => Expr::Sum { prod: *prod, from: b(from), to: b(to), body: b(body) },
            other => other.clone(),
        }
    }
}

/// Lit une expression ; l'erreur explique ce qui ne va pas
pub fn parse(src: &str) -> Result<Expr, String> {
    parse_with(src, &[])
}

/// Comme `parse`, en reconnaissant aussi les noms de paramètres `params`
pub fn parse_with(src: &str, params: &[String]) -> Result<Expr, String> {
    parse_mode(src, params, false)
}

/// Expression complexe : la variable est `z`, `i` est l'unité imaginaire
pub fn parse_complex(src: &str, params: &[String]) -> Result<Expr, String> {
    parse_mode(src, params, true)
}

fn parse_mode(src: &str, params: &[String], complex: bool) -> Result<Expr, String> {
    let chars: Vec<char> = src.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = P { c: &chars, i: 0, params, bound: Vec::new(), complex, depth: 0 };
    let e = p.expr()?;
    if p.i < chars.len() {
        return Err(format!("expression '{}' : caractère inattendu '{}'", src, chars[p.i]));
    }
    Ok(e)
}

/// Noms reconnus (fonctions, variable, constantes)
fn builtin_names(complex: bool) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = FUNCTIONS.iter().map(|f| f.0).collect();
    v.extend(["sum", "prod", "pi", "e"]);
    if complex {
        v.extend(["z", "i"]);
    } else {
        v.push("x");
    }
    v
}

/// Noms réservés par les expressions : un paramètre ne peut pas s'appeler ainsi
pub fn is_reserved(name: &str) -> bool {
    builtin_names(false).contains(&name)
}

struct P<'a> {
    c: &'a [char],
    i: usize,
    params: &'a [String],
    /// Variables des sommes en cours (la dernière est la plus intérieure)
    bound: Vec<String>,
    complex: bool,
    /// Profondeur d'appels de fonctions de l'utilisateur (une fonction peut en appeler une autre)
    depth: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.c.get(self.i + 1).copied()
    }

    // expr := somme (comparaison somme)?
    fn expr(&mut self) -> Result<Expr, String> {
        let a = self.sum()?;
        let op = match (self.peek(), self.peek2()) {
            (Some('<'), Some('=')) => Some(('l', 2)),
            (Some('>'), Some('=')) => Some(('g', 2)),
            (Some('='), Some('=')) => Some(('=', 2)),
            (Some('!'), Some('=')) => Some(('!', 2)),
            (Some('<'), _) => Some(('<', 1)),
            (Some('>'), _) => Some(('>', 1)),
            _ => None,
        };
        match op {
            Some((op, len)) => {
                self.i += len;
                Ok(Expr::Bin(op, Box::new(a), Box::new(self.sum()?)))
            }
            None => Ok(a),
        }
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
                let e = self.expr()?;
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
                // Un nom écrit en entier : variable de somme, paramètre (il peut contenir des chiffres et des _)
                let word: String =
                    self.c[self.i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '_').collect();
                if let Some(idx) = self.bound.iter().rposition(|b| *b == word) {
                    self.i += word.chars().count();
                    return Ok(Expr::Bound(idx));
                }
                if self.params.contains(&word) {
                    self.i += word.chars().count();
                    return Ok(Expr::Var(word));
                }
                // Fonction de l'utilisateur au nom complet (peut contenir des _ et des chiffres)
                if functions().read().unwrap().contains_key(&word) {
                    self.i += word.chars().count();
                    return self.name(&word);
                }
                // Plus long nom connu au début : « sinx » → sin, « xsin » → x puis sin, « kx » → k puis x
                let rest: String = self.c[self.i..].iter().take_while(|c| c.is_ascii_alphanumeric()).collect();
                let builtin = builtin_names(self.complex);
                let user: Vec<String> = functions().read().unwrap().keys().cloned().collect();
                let known = builtin
                    .iter()
                    .copied()
                    .chain(user.iter().map(|u| u.as_str()))
                    .chain(self.params.iter().map(|p| p.as_str()))
                    .chain(self.bound.iter().map(|b| b.as_str()))
                    .filter(|n| rest.starts_with(n))
                    .max_by_key(|n| n.len())
                    .map(|n| n.to_string());
                match known {
                    Some(name) => {
                        self.i += name.len();
                        self.name(&name)
                    }
                    None => Err(self.unknown(rest.trim_end_matches(|c: char| c.is_ascii_digit()))),
                }
            }
            Some(c) => Err(format!("caractère inattendu '{}'", c)),
            None => Err("expression incomplète".to_string()),
        }
    }

    fn unknown(&self, name: &str) -> String {
        if self.complex {
            if name.starts_with('x') || name.starts_with('y') {
                return format!("nom inconnu '{}' : en complexe, la variable est z = x + iy (re(z), im(z))", name);
            }
            format!("nom inconnu '{}' (variable : z ; constantes : i, pi, e)", name)
        } else {
            format!("nom inconnu '{}' (variable : x ; constantes : pi, e)", name)
        }
    }

    /// Arguments entre parenthèses, séparés par des virgules
    fn args(&mut self, name: &str) -> Result<Vec<Expr>, String> {
        if self.peek() != Some('(') {
            return Err(format!("{} attend des parenthèses : {}(...)", name, name));
        }
        self.i += 1;
        let mut args = vec![self.expr()?];
        while self.peek() == Some(',') {
            self.i += 1;
            args.push(self.expr()?);
        }
        if self.peek() != Some(')') {
            return Err(format!("{} : parenthèse fermante manquante", name));
        }
        self.i += 1;
        Ok(args)
    }

    /// Appel d'une fonction de l'utilisateur : son corps, arguments remplacés
    fn user_call(&mut self, name: &str, f: &UserFn) -> Result<Expr, String> {
        if self.depth > 24 {
            return Err(format!("{} : trop d'appels imbriqués (une fonction qui s'appelle elle-même ?)", name));
        }
        let args = self.args(name)?;
        if args.len() != f.params.len() {
            return Err(format!("{} attend {} argument(s) ({}), {} donné(s)", name, f.params.len(), f.params.join(", "), args.len()));
        }
        // Le corps voit ses arguments et les paramètres ; les variables de sommes en cours
        // gardent leur place (les indices restent justes)
        let mut names: Vec<String> = f.params.clone();
        names.extend(self.params.iter().filter(|p| !f.params.contains(p)).cloned());
        let chars: Vec<char> = f.body.chars().filter(|c| !c.is_whitespace()).collect();
        let hidden: Vec<String> = self.bound.iter().map(|_| "\u{1}".to_string()).collect();
        let mut sub = P { c: &chars, i: 0, params: &names, bound: hidden, complex: self.complex, depth: self.depth + 1 };
        let body = sub.expr().map_err(|e| format!("{} : {}", name, e))?;
        if sub.i < chars.len() {
            return Err(format!("{} : corps illisible", name));
        }
        let map: HashMap<String, Expr> = f.params.iter().cloned().zip(args).collect();
        Ok(body.subst(&map))
    }

    /// sum(k, début, fin, corps)
    fn sum_call(&mut self, prod: bool) -> Result<Expr, String> {
        let name = if prod { "prod" } else { "sum" };
        let form = format!("{}(k, 1, n, expression)", name);
        if self.peek() != Some('(') {
            return Err(format!("{} attend : {}", name, form));
        }
        self.i += 1;
        let var: String = self.c[self.i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '_').collect();
        if var.is_empty()
            || !var.starts_with(|c: char| c.is_ascii_alphabetic())
            || self.c.get(self.i + var.len()) != Some(&',')
        {
            return Err(format!("{} : la variable vient d'abord : {}", name, form));
        }
        self.i += var.len() + 1;
        let from = self.expr()?;
        if self.peek() != Some(',') {
            return Err(format!("{} attend : {}", name, form));
        }
        self.i += 1;
        let to = self.expr()?;
        if self.peek() != Some(',') {
            return Err(format!("{} attend : {}", name, form));
        }
        self.i += 1;
        self.bound.push(var);
        let body = self.expr();
        self.bound.pop();
        let body = body?;
        if self.peek() != Some(')') {
            return Err(format!("{} : parenthèse fermante manquante ({})", name, form));
        }
        self.i += 1;
        Ok(Expr::Sum { prod, from: Box::new(from), to: Box::new(to), body: Box::new(body) })
    }

    /// Nom lu : fonction (suivie de parenthèses), variable, constante, paramètre
    fn name(&mut self, name: &str) -> Result<Expr, String> {
        if let Some(idx) = self.bound.iter().rposition(|b| b == name) {
            return Ok(Expr::Bound(idx));
        }
        if self.params.iter().any(|p| p == name) {
            return Ok(Expr::Var(name.to_string()));
        }
        if name == "sum" || name == "prod" {
            return self.sum_call(name == "prod");
        }
        let user = functions().read().unwrap().get(name).cloned();
        if let Some(f) = user {
            return self.user_call(name, &f);
        }
        if let Some(&(fname, min, max, complex_ok)) = FUNCTIONS.iter().find(|f| f.0 == name) {
            if self.complex && !complex_ok {
                return Err(format!("{} n'est pas disponible avec les nombres complexes", fname));
            }
            let args = self.args(fname)?;
            let n = args.len();
            if n < min || (max > 0 && n > max) {
                let count = if min == max {
                    format!("{}", min)
                } else if max == 0 {
                    format!("au moins {}", min)
                } else {
                    format!("{} à {}", min, max)
                };
                return Err(format!("{} attend {} argument(s), {} donné(s)", fname, count, n));
            }
            return Ok(Expr::Call(fname, args));
        }
        match name {
            "x" if !self.complex => Ok(Expr::X),
            "z" if self.complex => Ok(Expr::X),
            "i" if self.complex => Ok(Expr::I),
            "pi" => Ok(Expr::Num(std::f64::consts::PI)),
            "e" => Ok(Expr::Num(std::f64::consts::E)),
            _ => Err(self.unknown(name)),
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
    fn fonctions_utilisateur() {
        let p = vec!["a".to_string()];
        define_function("tfu_f", &["x".to_string()], "x^2 + a", &p).unwrap();
        define_function("tfu_g", &["u".to_string(), "v".to_string()], "tfu_f(u) - v", &p).unwrap();
        let vals: HashMap<String, f64> = [("a".to_string(), 1.0)].into();
        let ev = |s: &str, x: f64| parse_with(s, &p).unwrap().eval(x, &vals);
        assert_eq!(ev("tfu_f(x)", 3.0), 10.0);
        assert_eq!(ev("tfu_f(2) + 1", 0.0), 6.0);
        assert_eq!(ev("tfu_g(3, 2)", 0.0), 8.0);
        assert_eq!(ev("2tfu_f(x)", 1.0), 4.0);
        // dans une somme : l'indice reste juste, même si le corps a sa propre somme
        define_function("tfu_s", &["n".to_string()], "sum(j, 1, n, j)", &[]).unwrap();
        assert_eq!(ev("sum(k, 1, 3, tfu_f(k))", 0.0), 1.0 + 1.0 + 4.0 + 1.0 + 9.0 + 1.0);
        assert_eq!(ev("sum(k, 1, 3, tfu_s(k))", 0.0), 1.0 + 3.0 + 6.0);
        // pas de x dans f(2) ; erreurs claires
        assert!(!parse_with("tfu_f(2)", &p).unwrap().uses_x());
        assert!(parse_with("tfu_f(1, 2)", &p).unwrap_err().contains("attend 1 argument"));
        assert!(define_function("sin", &["x".to_string()], "x", &[]).unwrap_err().contains("réservé"));
        assert!(define_function("tfu_bad", &["x".to_string()], "x + q", &[]).unwrap_err().contains("q"));
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
    fn fonctions_a_plusieurs_arguments() {
        assert_eq!(f("min(3, x, 7)", 1.0), 1.0);
        assert_eq!(f("max(x, 2)", 1.0), 2.0);
        assert_eq!(f("mod(x, 3)", -1.0), 2.0);
        assert_eq!(f("clamp(x, 0, 1)", 5.0), 1.0);
        assert!((f("atan2(1, 1)", 0.0) - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        // Par morceaux
        assert_eq!(f("if(x < 0, -x, x^2)", -3.0), 3.0);
        assert_eq!(f("if(x < 0, -x, x^2)", 3.0), 9.0);
        assert_eq!(f("(x >= 1) * 5", 1.0), 5.0);
        assert_eq!(f("and(x > 0, x < 2)", 1.0), 1.0);
        assert_eq!(f("x == 2", 2.0), 1.0);
        assert!(parse("min()").is_err());
        assert!(parse("atan2(1)").unwrap_err().contains("2 argument"));
    }

    #[test]
    fn sommes_et_produits() {
        assert_eq!(f("sum(k, 1, 4, k)", 0.0), 10.0);
        assert_eq!(f("prod(k, 1, 5, k)", 0.0), 120.0);
        // Série de Taylor de exp(x) à 20 termes
        assert!((f("sum(k, 0, 20, x^k / prod(j, 1, k, j))", 1.0) - std::f64::consts::E).abs() < 1e-12);
        // Variables imbriquées et multiplication implicite
        assert_eq!(f("sum(i, 1, 3, sum(j, 1, i, 1))", 0.0), 6.0);
        assert_eq!(f("sum(k, 1, 3, 2k)", 0.0), 12.0);
        // Borne non entière : le dernier terme entre en douceur
        let names = vec!["n".to_string()];
        let e = parse_with("sum(k, 1, n, k)", &names).unwrap();
        let at = |n: f64| e.eval(0.0, &[("n".to_string(), n)].into());
        assert_eq!(at(3.0), 6.0);
        assert_eq!(at(3.5), 8.0); // 1 + 2 + 3 + 0.5 × 4
        assert_eq!(at(4.0), 10.0);
        // La variable n'existe qu'à l'intérieur
        assert!(parse("sum(k, 1, 3, k) + k").is_err());
        assert!(parse("sum(1, 3, k)").unwrap_err().contains("variable vient d'abord"));
    }

    #[test]
    fn compilation() {
        let slots = vec!["x".to_string(), "y".to_string(), "t".to_string()];
        let params: HashMap<String, f64> = [("k".to_string(), 2.0)].into();
        let names: Vec<String> = ["y", "t", "k"].iter().map(|s| s.to_string()).collect();
        for src in ["sin(k*x)*y - t^2", "if(x < y, min(x, y, t), 3)", "sum(j, 1, 4, j*x) + y", "rand(3) + mod(x, 2)", "sum(i, 1, 3, prod(j, 1, i, j + x)) + sum(j, 1, t + 0.5, j)"] {
            let e = parse_with(src, &names).unwrap();
            let c = compile(&e, &slots, &params).unwrap();
            let v = [0.7, -1.3, 2.0];
            let mut vars = params.clone();
            vars.insert("y".into(), v[1]);
            vars.insert("t".into(), v[2]);
            assert!((c(&v) - e.eval(v[0], &vars)).abs() < 1e-12, "{}", src);
        }
    }

    #[test]
    fn probabilites() {
        assert_eq!(f("fact(5)", 0.0), 120.0);
        assert_eq!(f("binom(10, 3)", 0.0), 120.0);
        assert!((f("gamma(0.5)", 0.0) - std::f64::consts::PI.sqrt()).abs() < 1e-10);
        assert!((f("erf(1)", 0.0) - 0.842_700_79).abs() < 1e-6);
        // Tirages : reproductibles, dans [0, 1[, de moyenne 1/2 ; randn de moyenne 0 et variance 1
        assert_eq!(f("rand(7)", 0.0), f("rand(7)", 0.0));
        assert_ne!(f("rand(7)", 0.0), f("rand(8)", 0.0));
        assert_ne!(f("rand(7, 1)", 0.0), f("rand(7)", 0.0));
        let n = 20000.0;
        let mean = f("sum(k, 1, 20000, rand(k))", 0.0) / n;
        assert!((mean - 0.5).abs() < 0.01, "{}", mean);
        let m2 = f("sum(k, 1, 20000, randn(k)^2)", 0.0) / n;
        assert!((m2 - 1.0).abs() < 0.04, "{}", m2);
        // Monte-Carlo : π ≈ 4 × proportion de points dans le quart de disque
        let pi = 4.0 * f("sum(k, 1, 20000, rand(2k)^2 + rand(2k+1)^2 <= 1)", 0.0) / n;
        assert!((pi - std::f64::consts::PI).abs() < 0.05, "{}", pi);
    }

    #[test]
    fn complexes() {
        let ev = |src: &str, z: C| parse_complex(src, &[]).unwrap().eval_c(z, &HashMap::new());
        let close = |a: C, b: C| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        assert!(close(ev("z^2", C(1.0, 1.0)), C(0.0, 2.0)));
        assert!(close(ev("1/z", C(0.0, 2.0)), C(0.0, -0.5)));
        assert!(close(ev("exp(i*pi)", C(0.0, 0.0)), C(-1.0, 0.0)));
        assert!(close(ev("2i + z", C(1.0, 0.0)), C(1.0, 2.0)));
        assert!(close(ev("abs(z)", C(3.0, 4.0)), C(5.0, 0.0)));
        assert!(close(ev("sum(k, 0, 30, z^k / prod(j, 1, k, j))", C(0.0, 1.0)), C(1f64.cos(), 1f64.sin())));
        assert!(close(ev("sqrt(z)", C(-4.0, 0.0)), C(0.0, 2.0)));
        assert!(parse_complex("asin(z)", &[]).unwrap_err().contains("complexes"));
        assert!(parse_complex("x + 1", &[]).unwrap_err().contains("z = x + iy"));
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

