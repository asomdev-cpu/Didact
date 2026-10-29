//! Moteur de composition : texte et LaTeX → contours vectoriels.
//!
//! Chaîne : LaTeX → Typst (crate `mitex`) → composé par Typst (crate `typst`)
//! → exporté en SVG (`typst-svg`) → lu par `usvg` → nos chemins (`PathCmd`).
//!
//! - Polices : Latin Modern Roman (texte) et Latin Modern Math (maths), embarquées.
//! - Les définitions Typst de MiTeX (assets/typst/mitex, Apache-2.0) donnent le sens
//!   des commandes converties (`\frac`, `pmatrix`, `aligned`, ...).
//! - Le résultat est mis en cache : un même texte n'est composé qu'une fois.
//!
//! Les coordonnées produites sont en points typographiques, pour un texte de
//! `BASE_PT` points ; `place` les met à l'échelle et à la bonne position.

use super::geometry::{PathCmd, Rgba, Shape};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

static FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/lmroman10-regular.otf");
static FONT_MATH: &[u8] = include_bytes!("../../assets/fonts/latinmodern-math.otf");

static MITEX_MOD: &str = include_str!("../../assets/typst/mitex/specs/mod.typ");
static MITEX_PRELUDE: &str = include_str!("../../assets/typst/mitex/specs/prelude.typ");
static MITEX_STANDARD: &str = include_str!("../../assets/typst/mitex/specs/latex/standard.typ");

/// Taille de composition (points) : `place` met ensuite à l'échelle
const BASE_PT: f32 = 10.0;

/// Alignement horizontal par rapport au point d'ancrage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Ce qu'on demande de composer
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Content {
    /// Texte avec maths LaTeX entre `$...$` ; un retour à la ligne = nouvelle ligne
    Text(String),
    /// Formule LaTeX seule (mode mathématique, centrée)
    Math(String),
    /// LaTeX en mode texte (`\begin{align}...`, paragraphes, ...)
    Latex(String),
    /// Tableau : en-tête et lignes ; chaque case est du texte avec `$...$`
    Table { header: Vec<String>, rows: Vec<Vec<String>> },
}

/// Un chemin composé (coordonnées en points, origine en haut à gauche)
#[derive(Debug, Clone)]
struct TPath {
    cmds: Vec<PathCmd>,
    fill: bool,
    stroke: Option<f32>,
}

/// Résultat d'une composition
#[derive(Debug, Clone)]
pub struct Typeset {
    paths: Vec<TPath>,
    width: f32,
    height: f32,
}

// ─────────────────────────────────────────────
//  ENVIRONNEMENT TYPST
// ─────────────────────────────────────────────

/// Ce qui ne change jamais : bibliothèque standard et polices (créées une fois)
struct Engine {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let fonts: Vec<Font> = [FONT_REGULAR, FONT_MATH]
            .iter()
            .flat_map(|data| Font::iter(Bytes::new(data.to_vec())))
            .collect();
        Engine {
            library: LazyHash::new(Library::builder().build()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

fn file_id(path: &str) -> FileId {
    RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).expect("chemin valide")).intern()
}

/// Un « projet » Typst en mémoire : le document principal + les définitions MiTeX
struct DocWorld {
    main_id: FileId,
    files: Vec<(FileId, String)>,
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        &engine().library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &engine().book
    }
    fn main(&self) -> FileId {
        self.main_id
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(f, text)| Source::new(*f, text.clone()))
            .ok_or(FileError::AccessDenied)
    }
    fn file(&self, _id: FileId) -> FileResult<Bytes> {
        Err(FileError::AccessDenied) // pas d'accès aux fichiers (pas d'image, pas de package)
    }
    fn font(&self, index: usize) -> Option<Font> {
        engine().fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

// ─────────────────────────────────────────────
//  LaTeX / TEXTE → SOURCE TYPST
// ─────────────────────────────────────────────

/// Chaîne Typst entre guillemets
fn typst_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Formule LaTeX → appel Typst qui l'évalue avec les définitions MiTeX
fn math_call(tex: &str, block: bool) -> Result<String, String> {
    let converted = mitex::convert_math(tex, None).map_err(|e| format!("LaTeX invalide dans « {} » : {}", tex, e))?;
    Ok(format!(
        "#math.equation(block: {}, eval(\"$\" + {} + \"$\", scope: mitex-scope))",
        block,
        typst_str(&converted)
    ))
}

/// Une ligne de texte avec des `$...$` → contenu Typst
fn text_line(line: &str) -> Result<String, String> {
    let mut out = String::new();
    for (i, part) in line.split('$').enumerate() {
        if part.is_empty() {
            continue;
        }
        if i % 2 == 1 {
            out.push_str(&math_call(part, false)?);
        } else {
            out.push_str(&format!("#{}", typst_str(part)));
        }
    }
    Ok(out)
}

fn text_block(text: &str) -> Result<String, String> {
    let lines: Result<Vec<String>, String> = text.lines().map(text_line).collect();
    Ok(lines?.join(" #linebreak() "))
}

fn document(content: &Content, align: Align) -> Result<String, String> {
    let align = match align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    };
    let body = match content {
        Content::Text(text) => text_block(text)?,
        Content::Math(tex) => math_call(tex, true)?,
        Content::Latex(tex) => {
            let converted =
                mitex::convert_text(tex, None).map_err(|e| format!("LaTeX invalide : {}", e))?;
            format!("#eval({}, mode: \"markup\", scope: mitex-scope)", typst_str(&converted))
        }
        Content::Table { header, rows } => {
            let columns = header.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0)).max(1);
            let mut cells = Vec::new();
            for h in header {
                cells.push(format!("[{}]", text_block(h)?));
            }
            if !header.is_empty() {
                cells.push("table.hline(stroke: 0.8pt)".to_string());
            }
            for row in rows {
                for c in row {
                    cells.push(format!("[{}]", text_block(c)?));
                }
            }
            format!(
                "#table(columns: {}, align: center + horizon, inset: (x: 7pt, y: 5pt), stroke: (x, y) => if x > 0 {{ (left: 0.5pt) }}, {})",
                columns,
                cells.join(", ")
            )
        }
    };
    Ok(format!(
        "#import \"specs/mod.typ\": mitex-scope\n\
         #set page(width: auto, height: auto, margin: 0pt, fill: none)\n\
         #set text(font: \"Latin Modern Roman\", size: {}pt)\n\
         #show math.equation: set text(font: \"Latin Modern Math\")\n\
         #set par(leading: 0.6em)\n\
         #set align({})\n\
         {}\n",
        BASE_PT, align, body
    ))
}

// ─────────────────────────────────────────────
//  COMPOSITION + CACHE
// ─────────────────────────────────────────────

/// Compose (ou relit dans le cache)
pub fn typeset(content: &Content, align: Align) -> Result<Arc<Typeset>, String> {
    static CACHE: OnceLock<Mutex<HashMap<(Content, Align), Result<Arc<Typeset>, String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (content.clone(), align);
    if let Some(r) = cache.lock().unwrap().get(&key) {
        return r.clone();
    }
    let result = compose(content, align).map(Arc::new);
    cache.lock().unwrap().insert(key, result.clone());
    result
}

fn compose(content: &Content, align: Align) -> Result<Typeset, String> {
    let world = DocWorld {
        main_id: file_id("/main.typ"),
        files: vec![
            (file_id("/main.typ"), document(content, align)?),
            (file_id("/specs/mod.typ"), MITEX_MOD.to_string()),
            (file_id("/specs/prelude.typ"), MITEX_PRELUDE.to_string()),
            (file_id("/specs/latex/standard.typ"), MITEX_STANDARD.to_string()),
        ],
    };
    let doc: PagedDocument = typst::compile(&world).output.map_err(|errors| {
        let msg = errors.first().map(|e| e.message.to_string()).unwrap_or_default();
        format!("LaTeX : {}", msg)
    })?;
    let page = doc.pages().first().ok_or("LaTeX : rien à afficher")?;
    let svg = typst_svg::svg(page, &typst_svg::SvgOptions::default());

    // 72 dpi : 1 point = 1 unité
    let options = usvg::Options { dpi: 72.0, ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(&svg, &options).map_err(|e| format!("SVG illisible : {}", e))?;
    let mut paths = Vec::new();
    collect(tree.root(), &mut paths);
    Ok(Typeset { paths, width: tree.size().width(), height: tree.size().height() })
}

/// Parcourt le SVG et récupère les chemins, transformations appliquées
fn collect(group: &usvg::Group, out: &mut Vec<TPath>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, out),
            usvg::Node::Path(p) => {
                let t = p.abs_transform();
                let map = |pt: usvg::tiny_skia_path::Point| {
                    (t.sx * pt.x + t.kx * pt.y + t.tx, t.ky * pt.x + t.sy * pt.y + t.ty)
                };
                let mut cmds = Vec::new();
                let mut last = (0.0, 0.0);
                for seg in p.data().segments() {
                    use usvg::tiny_skia_path::PathSegment as S;
                    match seg {
                        S::MoveTo(a) => {
                            last = map(a);
                            cmds.push(PathCmd::MoveTo(last.0, last.1));
                        }
                        S::LineTo(a) => {
                            last = map(a);
                            cmds.push(PathCmd::LineTo(last.0, last.1));
                        }
                        S::QuadTo(q, a) => {
                            let (q, e) = (map(q), map(a));
                            cmds.push(PathCmd::CubicTo(
                                last.0 + 2.0 / 3.0 * (q.0 - last.0),
                                last.1 + 2.0 / 3.0 * (q.1 - last.1),
                                e.0 + 2.0 / 3.0 * (q.0 - e.0),
                                e.1 + 2.0 / 3.0 * (q.1 - e.1),
                                e.0,
                                e.1,
                            ));
                            last = e;
                        }
                        S::CubicTo(a, b, c) => {
                            let (a, b, c) = (map(a), map(b), map(c));
                            cmds.push(PathCmd::CubicTo(a.0, a.1, b.0, b.1, c.0, c.1));
                            last = c;
                        }
                        S::Close => cmds.push(PathCmd::Close),
                    }
                }
                let scale = (t.sx * t.sy - t.kx * t.ky).abs().sqrt();
                out.push(TPath {
                    cmds,
                    fill: p.fill().is_some(),
                    stroke: p.stroke().map(|s| s.width().get() * scale),
                });
            }
            _ => {}
        }
    }
}

// ─────────────────────────────────────────────
//  PLACEMENT
// ─────────────────────────────────────────────

/// Place une composition : `(x, y)` = point d'ancrage en pixels, centré verticalement,
/// aligné horizontalement selon `align` ; `size` = taille du texte en pixels.
pub fn place(ts: &Typeset, x: f32, y: f32, size: f32, align: Align, color: Rgba) -> Vec<Shape> {
    let k = size / BASE_PT;
    let (w, h) = (ts.width * k, ts.height * k);
    let left = match align {
        Align::Left => x,
        Align::Center => x - w / 2.0,
        Align::Right => x - w,
    };
    let top = y - h / 2.0;
    let m = |px: f32, py: f32| (left + px * k, top + py * k);

    ts.paths
        .iter()
        .map(|p| Shape {
            path: p
                .cmds
                .iter()
                .map(|c| match *c {
                    PathCmd::MoveTo(a, b) => {
                        let (a, b) = m(a, b);
                        PathCmd::MoveTo(a, b)
                    }
                    PathCmd::LineTo(a, b) => {
                        let (a, b) = m(a, b);
                        PathCmd::LineTo(a, b)
                    }
                    PathCmd::CubicTo(a, b, c, d, e, f) => {
                        let ((a, b), (c, d), (e, f)) = (m(a, b), m(c, d), m(e, f));
                        PathCmd::CubicTo(a, b, c, d, e, f)
                    }
                    PathCmd::Close => PathCmd::Close,
                })
                .collect(),
            fill: if p.fill { Some(color) } else { None },
            stroke: p.stroke.map(|w| (color, w * k)),
        })
        .collect()
}

/// Compose et place en une fois
pub fn shapes(content: &Content, x: f32, y: f32, size: f32, align: Align, color: Rgba) -> Result<Vec<Shape>, String> {
    let ts = typeset(content, align)?;
    Ok(place(&ts, x, y, size, align, color))
}

/// Vérifie qu'un contenu se compose (pour le validateur)
pub fn check(content: &Content) -> Result<(), String> {
    typeset(content, Align::Center).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formules() {
        for tex in [
            r"\int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            r"\sum_{i=1}^n x_i^2",
        ] {
            let ts = typeset(&Content::Math(tex.into()), Align::Center).unwrap();
            assert!(!ts.paths.is_empty() && ts.width > 0.0 && ts.height > 0.0, "{}", tex);
        }
    }

    #[test]
    fn fraction_plus_haute_qu_une_lettre() {
        let x = typeset(&Content::Math("x".into()), Align::Center).unwrap();
        let frac = typeset(&Content::Math(r"\frac{a}{b}".into()), Align::Center).unwrap();
        assert!(frac.height > x.height * 1.5); // empilée, pas « a/b » sur la ligne
    }

    #[test]
    fn texte_et_alignement() {
        let s = shapes(&Content::Text("Didact $x^2$".into()), 100.0, 100.0, 40.0, Align::Left, Rgba::BLACK).unwrap();
        let xmin = s
            .iter()
            .flat_map(|sh| sh.path.iter())
            .filter_map(|c| match c {
                PathCmd::MoveTo(x, _) | PathCmd::LineTo(x, _) => Some(*x),
                _ => None,
            })
            .fold(f32::MAX, f32::min);
        assert!(xmin >= 99.0 && xmin < 110.0);
    }

    #[test]
    fn erreurs_lisibles() {
        let e = typeset(&Content::Math(r"\inconnue{x}".into()), Align::Center).unwrap_err();
        assert!(e.contains("LaTeX"), "{}", e);
    }

    #[test]
    fn tableau_et_latex_texte() {
        let t = Content::Table {
            header: vec!["$x$".into(), "$f(x)$".into()],
            rows: vec![vec!["0".into(), "1".into()], vec!["1".into(), "$e$".into()]],
        };
        assert!(typeset(&t, Align::Center).is_ok());
        let l = Content::Latex(r"\begin{align} f(x) &= x^2 + 2x + 1 \\ &= (x+1)^2 \end{align}".into());
        assert!(typeset(&l, Align::Center).is_ok());
    }
}
