//! Texte : chaque lettre devient un chemin vectoriel (contour de la police).
//!
//! Avantages : le texte se dessine avec le même backend que les formes, reste net
//! à toutes les tailles, et s'anime comme n'importe quelle figure (move, rotate, ...).
//!
//! Polices : Latin Modern Roman (texte) et Latin Modern Math (maths), les polices de
//! LaTeX, embarquées dans le programme. Licence GUST (libre), voir assets/fonts/.
//!
//! Maths en ligne, entre `$...$` (sous-ensemble de LaTeX, en attendant le vrai moteur) :
//! - lettres en italique mathématique, chiffres droits, espaces de la source ignorés ;
//! - commandes grecques et symboles courants (`\theta`, `\pi`, `\infty`, `\leq`, ...) ;
//! - exposants et indices : `x^2`, `x^{n+1}`, `a_n`, `a_{ij}` ;
//! - espacement automatique autour de + − = < > ...
//!
//! Plusieurs lignes : un vrai retour à la ligne dans la chaîne.

use super::geometry::{PathCmd, Rgba, Shape};
use ttf_parser::{Face, OutlineBuilder};

static FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/lmroman10-regular.otf");
static FONT_MATH: &[u8] = include_bytes!("../../assets/fonts/latinmodern-math.otf");

/// Alignement horizontal par rapport au point d'ancrage
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Font {
    Regular,
    Math,
}

/// Un caractère à placer : police, taille relative, décalage vertical (en em), espace avant (em)
#[derive(Debug, Clone, PartialEq)]
struct Item {
    c: char,
    font: Font,
    /// 1.0 = taille normale, 0.7 = exposant / indice
    scale: f32,
    /// Décalage vers le haut, en em de la taille normale
    rise: f32,
    /// Espace ajouté avant, en em
    space_before: f32,
    /// Espace ajouté après, en em
    space_after: f32,
}

// ─────────────────────────────────────────────
//  CONVERSION TEXTE → CARACTÈRES À PLACER
// ─────────────────────────────────────────────

/// Découpe une ligne : le texte normal en police droite, les maths `$...$` en police mathématique
fn line_items(line: &str) -> Vec<Item> {
    let mut items = Vec::new();
    for (i, part) in line.split('$').enumerate() {
        if i % 2 == 1 {
            math_items(part, &mut items);
        } else {
            for c in part.chars() {
                items.push(Item { c, font: Font::Regular, scale: 1.0, rise: 0.0, space_before: 0.0, space_after: 0.0 });
            }
        }
    }
    items
}

/// Lettre latine ou grecque → italique mathématique Unicode (comme LaTeX)
fn math_italic(c: char) -> char {
    let code = match c {
        'h' => 0x210E, // trou dans le bloc Unicode : ℎ
        'a'..='z' => 0x1D44E + (c as u32 - 'a' as u32),
        'A'..='Z' => 0x1D434 + (c as u32 - 'A' as u32),
        'α'..='ω' => 0x1D6FC + (c as u32 - 'α' as u32),
        'ϑ' => 0x1D717,
        'ϕ' => 0x1D719,
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}

/// Commande LaTeX → caractère (`\theta` → θ)
fn command(name: &str) -> Option<char> {
    const TABLE: &[(&str, char)] = &[
        ("alpha", 'α'), ("beta", 'β'), ("gamma", 'γ'), ("delta", 'δ'), ("epsilon", 'ϵ'),
        ("varepsilon", 'ε'), ("zeta", 'ζ'), ("eta", 'η'), ("theta", 'θ'), ("vartheta", 'ϑ'),
        ("iota", 'ι'), ("kappa", 'κ'), ("lambda", 'λ'), ("mu", 'μ'), ("nu", 'ν'), ("xi", 'ξ'),
        ("pi", 'π'), ("rho", 'ρ'), ("sigma", 'σ'), ("tau", 'τ'), ("upsilon", 'υ'), ("phi", 'ϕ'),
        ("varphi", 'φ'), ("chi", 'χ'), ("psi", 'ψ'), ("omega", 'ω'),
        ("Gamma", 'Γ'), ("Delta", 'Δ'), ("Theta", 'Θ'), ("Lambda", 'Λ'), ("Xi", 'Ξ'), ("Pi", 'Π'),
        ("Sigma", 'Σ'), ("Phi", 'Φ'), ("Psi", 'Ψ'), ("Omega", 'Ω'),
        ("infty", '∞'), ("cdot", '⋅'), ("times", '×'), ("pm", '±'), ("leq", '≤'), ("le", '≤'),
        ("geq", '≥'), ("ge", '≥'), ("neq", '≠'), ("approx", '≈'), ("to", '→'), ("rightarrow", '→'),
        ("in", '∈'), ("partial", '∂'), ("nabla", '∇'), ("sum", '∑'), ("int", '∫'), ("prod", '∏'),
        ("sqrt", '√'), ("degree", '°'), ("circ", '∘'),
    ];
    TABLE.iter().find(|(k, _)| *k == name).map(|(_, c)| *c)
}

/// Espace (en em) autour d'un caractère mathématique : opérateurs et relations (comme TeX)
fn math_spacing(c: char) -> f32 {
    match c {
        '+' | '−' | '×' | '⋅' | '±' => 4.0 / 18.0,
        '=' | '<' | '>' | '≤' | '≥' | '≠' | '≈' | '→' | '∈' => 5.0 / 18.0,
        _ => 0.0,
    }
}

/// Lit une formule `$...$` (sans les $)
fn math_items(src: &str, out: &mut Vec<Item>) {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    parse_math(&chars, &mut i, 1.0, 0.0, out, false);
}

/// Lit des symboles jusqu'à la fin, ou jusqu'à `}` si `in_group`
fn parse_math(chars: &[char], i: &mut usize, scale: f32, rise: f32, out: &mut Vec<Item>, in_group: bool) {
    while *i < chars.len() {
        let c = chars[*i];
        *i += 1;
        match c {
            '}' if in_group => return,
            ' ' | '\t' => {} // les espaces de la source sont ignorés en maths
            '{' => parse_math(chars, i, scale, rise, out, true),
            '^' | '_' => {
                // Exposant / indice : plus petit, monté ou descendu
                let (s, r) = if c == '^' { (scale * 0.7, rise + 0.45 * scale) } else { (scale * 0.7, rise - 0.22 * scale) };
                parse_script(chars, i, s, r, out);
            }
            '\\' => command_items(chars, i, scale, rise, out),
            _ => push_math_char(c, scale, rise, out),
        }
    }
}

/// Noms de fonctions écrits en droit, comme en LaTeX : \sin(x), \ln x, \lim ...
const OPERATORS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "exp", "ln", "log", "lim", "max", "min", "sup", "inf", "det", "deg", "dim", "gcd", "ker",
];

/// Après `\` : commande à un symbole, nom d'opérateur, ou commande de structure
fn command_items(chars: &[char], i: &mut usize, scale: f32, rise: f32, out: &mut Vec<Item>) {
    let start = *i;
    while *i < chars.len() && chars[*i].is_ascii_alphabetic() {
        *i += 1;
    }
    let name: String = chars[start..*i].iter().collect();
    match name.as_str() {
        // \, \; \! \{ \} ... : un seul caractère
        "" => {
            if *i < chars.len() {
                *i += 1;
                match chars[*i - 1] {
                    ',' | ';' | ' ' | ':' => out.push(spacer(4.0 / 18.0)),
                    '!' => {}
                    other => push_math_char(other, scale, rise, out),
                }
            }
        }
        op if OPERATORS.contains(&op) => {
            for c in op.chars() {
                out.push(Item { c, font: Font::Math, scale, rise, space_before: 0.0, space_after: 0.0 });
            }
            out.push(spacer(1.0 / 6.0 * scale));
        }
        // \text{...} / \mathrm{...} : texte droit, espaces conservés
        "text" | "mathrm" | "textrm" | "operatorname" => {
            let keep_spaces = name == "text" || name == "textrm";
            for c in read_group(chars, i) {
                if c == ' ' && !keep_spaces {
                    continue;
                }
                let font = if name == "text" || name == "textrm" { Font::Regular } else { Font::Math };
                out.push(Item { c, font, scale, rise, space_before: 0.0, space_after: 0.0 });
            }
        }
        // \frac{a}{b} : écrit a/b sur la ligne (en attendant le vrai moteur LaTeX)
        "frac" | "dfrac" | "tfrac" => {
            for (k, group) in [read_group(chars, i), read_group(chars, i)].iter().enumerate() {
                let mut items = Vec::new();
                let mut j = 0;
                parse_math(group, &mut j, scale, rise, &mut items, false);
                let wrap = items.len() > 1 && items.iter().any(|it| it.space_before > 0.0);
                if k == 1 {
                    out.push(Item { c: '/', font: Font::Math, scale, rise, space_before: 0.0, space_after: 0.0 });
                }
                if wrap {
                    push_math_char('(', scale, rise, out);
                }
                out.extend(items);
                if wrap {
                    push_math_char(')', scale, rise, out);
                }
            }
        }
        // \sqrt{x} : √x
        "sqrt" => {
            push_math_char('√', scale, rise, out);
            let group = read_group(chars, i);
            let mut j = 0;
            parse_math(&group, &mut j, scale, rise, out, false);
        }
        // Délimiteurs automatiques : ignorés, on garde le délimiteur lui-même
        "left" | "right" | "big" | "Big" | "displaystyle" => {}
        "quad" => out.push(spacer(1.0)),
        "qquad" => out.push(spacer(2.0)),
        _ => {
            if let Some(sym) = command(&name) {
                push_math_char(sym, scale, rise, out);
            }
        }
    }
}

/// Lit `{...}` (ou un seul caractère) et renvoie son contenu
fn read_group(chars: &[char], i: &mut usize) -> Vec<char> {
    while *i < chars.len() && chars[*i] == ' ' {
        *i += 1;
    }
    if *i >= chars.len() {
        return Vec::new();
    }
    if chars[*i] != '{' {
        *i += 1;
        return vec![chars[*i - 1]];
    }
    *i += 1;
    let mut depth = 1;
    let mut out = Vec::new();
    while *i < chars.len() {
        let c = chars[*i];
        *i += 1;
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// Espace vide de `em` cadratins
fn spacer(em: f32) -> Item {
    Item { c: '\u{200B}', font: Font::Math, scale: 0.0, rise: 0.0, space_before: em, space_after: 0.0 }
}

/// Après ^ ou _ : un seul symbole, ou un groupe {…}
fn parse_script(chars: &[char], i: &mut usize, scale: f32, rise: f32, out: &mut Vec<Item>) {
    while *i < chars.len() && chars[*i] == ' ' {
        *i += 1;
    }
    if *i >= chars.len() {
        return;
    }
    let c = chars[*i];
    *i += 1;
    match c {
        '{' => parse_math(chars, i, scale, rise, out, true),
        '\\' => command_items(chars, i, scale, rise, out),
        _ => push_math_char(c, scale, rise, out),
    }
}

fn push_math_char(c: char, scale: f32, rise: f32, out: &mut Vec<Item>) {
    let c = if c == '-' { '−' } else { c };
    // Pas d'espacement d'opérateur dans les exposants (comme TeX)
    let space = if scale < 1.0 { 0.0 } else { math_spacing(c) };
    out.push(Item {
        c: math_italic(c),
        font: Font::Math,
        scale,
        rise,
        space_before: space,
        space_after: space,
    });
}

// ─────────────────────────────────────────────
//  CONTOURS DES LETTRES
// ─────────────────────────────────────────────

/// Collecte les commandes de tracé d'une lettre, en pixels
struct GlyphPath<'a> {
    out: &'a mut Vec<PathCmd>,
    /// Position de la ligne de base et échelle (unités de police → pixels)
    x: f32,
    y: f32,
    scale: f32,
    /// Dernier point, pour convertir les courbes quadratiques en cubiques
    last: (f32, f32),
}

impl GlyphPath<'_> {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        // y de la police vers le haut → y pixel vers le bas
        (self.x + x * self.scale, self.y - y * self.scale)
    }
}

impl OutlineBuilder for GlyphPath<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.out.push(PathCmd::MoveTo(x, y));
        self.last = (x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.out.push(PathCmd::LineTo(x, y));
        self.last = (x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // Quadratique → cubique équivalente
        let (qx, qy) = self.p(x1, y1);
        let (ex, ey) = self.p(x, y);
        let (sx, sy) = self.last;
        self.out.push(PathCmd::CubicTo(
            sx + 2.0 / 3.0 * (qx - sx),
            sy + 2.0 / 3.0 * (qy - sy),
            ex + 2.0 / 3.0 * (qx - ex),
            ey + 2.0 / 3.0 * (qy - ey),
            ex,
            ey,
        ));
        self.last = (ex, ey);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (ax, ay) = self.p(x1, y1);
        let (bx, by) = self.p(x2, y2);
        let (ex, ey) = self.p(x, y);
        self.out.push(PathCmd::CubicTo(ax, ay, bx, by, ex, ey));
        self.last = (ex, ey);
    }
    fn close(&mut self) {
        self.out.push(PathCmd::Close);
    }
}

struct Fonts<'a> {
    regular: Face<'a>,
    math: Face<'a>,
}

impl Fonts<'_> {
    /// Police et glyphe d'un caractère ; si la police texte ne l'a pas, on essaie la police maths
    fn glyph(&self, item: &Item) -> Option<(&Face<'_>, ttf_parser::GlyphId)> {
        let primary = match item.font {
            Font::Regular => &self.regular,
            Font::Math => &self.math,
        };
        primary
            .glyph_index(item.c)
            .map(|g| (primary, g))
            .or_else(|| self.math.glyph_index(item.c).map(|g| (&self.math, g)))
    }

    /// Avance horizontale d'un caractère en pixels (espaces compris)
    fn advance(&self, item: &Item, size: f32) -> f32 {
        let s = size * item.scale;
        let glyph_w = match self.glyph(item) {
            Some((face, g)) => face.glyph_hor_advance(g).unwrap_or(0) as f32 * s / face.units_per_em() as f32,
            None => s * 0.5,
        };
        glyph_w + (item.space_before + item.space_after) * size
    }
}

/// Formes d'un texte : `(x, y)` est le point d'ancrage (pixels), centré verticalement,
/// aligné horizontalement selon `align`. `size` = taille de la police en pixels.
pub fn text_shapes(content: &str, x: f32, y: f32, size: f32, align: Align, color: Rgba) -> Result<Vec<Shape>, String> {
    let fonts = Fonts {
        regular: Face::parse(FONT_REGULAR, 0).map_err(|e| format!("police illisible : {}", e))?,
        math: Face::parse(FONT_MATH, 0).map_err(|e| format!("police illisible : {}", e))?,
    };

    let lines: Vec<&str> = content.lines().collect();
    let line_height = size * 1.3;
    // Centrage vertical : on place le milieu des majuscules (≈ 0,35 em) sur y
    let total = line_height * (lines.len().max(1) as f32 - 1.0);
    let first_baseline = y - total / 2.0 + size * 0.35;

    let mut path = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let items = line_items(line);
        let width: f32 = items.iter().map(|it| fonts.advance(it, size)).sum();
        let mut pen_x = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        let baseline = first_baseline + i as f32 * line_height;

        for item in &items {
            pen_x += item.space_before * size;
            let s = size * item.scale;
            match fonts.glyph(item) {
                Some((face, glyph)) => {
                    let scale = s / face.units_per_em() as f32;
                    let base_y = baseline - item.rise * size;
                    let mut builder = GlyphPath { out: &mut path, x: pen_x, y: base_y, scale, last: (pen_x, base_y) };
                    face.outline_glyph(glyph, &mut builder);
                    pen_x += face.glyph_hor_advance(glyph).unwrap_or(0) as f32 * scale;
                }
                None => pen_x += s * 0.5, // caractère absent des polices : un espace
            }
            pen_x += item.space_after * size;
        }
    }

    if path.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![Shape { path, fill: Some(color), stroke: None }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(src: &str) -> Vec<(char, Font)> {
        line_items(src).iter().map(|i| (i.c, i.font)).collect()
    }

    #[test]
    fn maths_en_ligne() {
        // texte droit, puis θ en italique mathématique
        let items = chars(r"angle $\theta$");
        assert_eq!(items[0], ('a', Font::Regular));
        assert_eq!(items.last().unwrap(), &('𝜃', Font::Math));
    }

    #[test]
    fn exposants_et_indices() {
        let items = line_items("$x^{n+1} - a_2$");
        let x = &items[0];
        assert_eq!((x.c, x.scale, x.rise), ('𝑥', 1.0, 0.0));
        assert!(items[1].scale < 1.0 && items[1].rise > 0.0); // n en exposant
        assert_eq!(items[2].space_before, 0.0); // pas d'espace autour du + en exposant
        let moins = items.iter().find(|i| i.c == '−').unwrap();
        assert!(moins.space_before > 0.0); // espace autour du − normal
        assert!(items.last().unwrap().rise < 0.0); // 2 en indice
    }

    #[test]
    fn operateurs_et_fractions() {
        let txt: String = line_items(r"$\sin(x) = \frac{a+b}{2}$").iter().map(|i| i.c).filter(|c| *c != '\u{200B}').collect();
        assert_eq!(txt, "sin(𝑥)=(𝑎+𝑏)/2");
        let t: String = line_items(r"$\text{si } x$").iter().map(|i| i.c).collect();
        assert_eq!(t, "si 𝑥");
    }

    #[test]
    fn alignement() {
        let x_min = |s: &[Shape]| {
            s[0].path.iter().filter_map(|c| match c {
                PathCmd::MoveTo(x, _) | PathCmd::LineTo(x, _) => Some(*x),
                _ => None,
            }).fold(f32::MAX, f32::min)
        };
        let gauche = text_shapes("Didact", 100.0, 100.0, 40.0, Align::Left, Rgba::BLACK).unwrap();
        let centre = text_shapes("Didact", 100.0, 100.0, 40.0, Align::Center, Rgba::BLACK).unwrap();
        assert!(x_min(&gauche) >= 100.0);
        assert!(x_min(&centre) < 100.0);
    }
}
