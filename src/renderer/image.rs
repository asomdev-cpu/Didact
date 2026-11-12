//! Type `image` : un fichier PNG ou SVG posé sur la scène.
//!
//! ```text
//! deffigure schema =
//!     type   : image
//!     file   : schema.svg      # chemin relatif au fichier .dct
//!     pos    : (50%, 50%)
//!     size   : 40%             # plus grand côté, en % du plus petit côté de l'écran (ou en px)
//!     anchor : center          # défaut
//! ```
//!
//! - Les proportions du fichier sont gardées.
//! - Un SVG est dessiné par resvg à la taille de l'écran (net à toute taille), gardé en cache ;
//!   ses textes sont composés avec les polices embarquées (Latin Modern).
//! - Fichiers : lus à côté du `.dct` (natif, `set_base_dir`), ou donnés par le lecteur web
//!   (`register`), qui les télécharge d'après `files`.

use super::geometry::{length, point, require, Raster, Refs, Rgba, Scene, Shape};
use crate::ast::PropValue;
use crate::renderer::geometry::PathCmd;
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// Contenu des fichiers : donnés par le lecteur web, ou lus sur le disque une fois
fn store() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    static FILES: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    FILES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn base_dir() -> &'static Mutex<Option<PathBuf>> {
    static DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    DIR.get_or_init(|| Mutex::new(None))
}

/// Dossier du `.dct` : les chemins des images sont relatifs à lui (natif)
pub fn set_base_dir(dir: &std::path::Path) {
    let mut d = base_dir().lock().unwrap();
    if d.as_deref() != Some(dir) {
        *d = Some(dir.to_path_buf());
        // Autre fichier .dct : on oublie les images lues
        store().lock().unwrap().clear();
        rasters().lock().unwrap().clear();
    }
}

/// Oublie les fichiers lus (le `.dct` a été rechargé : une image a pu changer)
pub fn forget() {
    let dir = base_dir().lock().unwrap().is_some();
    if dir {
        store().lock().unwrap().clear();
        rasters().lock().unwrap().clear();
    }
}

/// Contenu d'un fichier donné directement (lecteur web, page autonome)
pub fn register(name: &str, bytes: Vec<u8>) {
    store().lock().unwrap().insert(name.to_string(), Arc::new(bytes));
    rasters().lock().unwrap().retain(|k, _| k.0 != name);
}

/// Les fichiers images utilisés par des figures (pour que le lecteur web les télécharge)
pub fn files<'a>(figures: impl IntoIterator<Item = &'a ResolvedFigure>) -> Vec<String> {
    let mut out: Vec<String> = figures
        .into_iter()
        .filter(|f| f.ftype == "image")
        .filter_map(|f| f.properties.get("file").map(fmt_value))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn bytes(name: &str) -> Result<Arc<Vec<u8>>, String> {
    if let Some(b) = store().lock().unwrap().get(name) {
        return Ok(b.clone());
    }
    let dir = base_dir().lock().unwrap().clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = dir.unwrap_or_default().join(name);
        let b = std::fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("image '{}' : fichier introuvable", path.display()),
            _ => format!("image '{}' : fichier illisible ({})", path.display(), e),
        })?;
        let b = Arc::new(b);
        store().lock().unwrap().insert(name.to_string(), b.clone());
        Ok(b)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dir;
        Err(format!("image '{}' : fichier pas encore chargé (ou introuvable)", name))
    }
}

fn is_svg(name: &str, data: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".svg") || lower.ends_with(".svgz") || (!data.starts_with(b"\x89PNG") && data.iter().take(512).any(|&c| c == b'<'))
}

/// Polices des textes d'un SVG : celles de Didact
fn svg_options() -> &'static usvg::Options<'static> {
    static OPT: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPT.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for data in super::typeset::TEXT_FONTS {
            db.load_font_data(data.to_vec());
        }
        db.set_serif_family("Latin Modern Roman");
        db.set_sans_serif_family("Latin Modern Roman");
        db.set_monospace_family("Latin Modern Roman");
        let mut opt = usvg::Options { font_family: "Latin Modern Roman".to_string(), ..Default::default() };
        opt.fontdb = Arc::new(db);
        opt
    })
}

fn parse_svg(name: &str, data: &[u8]) -> Result<usvg::Tree, String> {
    usvg::Tree::from_data(data, svg_options()).map_err(|e| format!("image '{}' : SVG illisible ({})", name, e))
}

/// Images prêtes à dessiner : (fichier, largeur, hauteur) → pixels (0 × 0 : taille d'origine d'un PNG)
type RasterKey = (String, u32, u32);

fn rasters() -> &'static Mutex<HashMap<RasterKey, Arc<Raster>>> {
    static CACHE: OnceLock<Mutex<HashMap<RasterKey, Arc<Raster>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Taille naturelle (largeur, hauteur) d'un fichier, en unités du fichier
fn natural_size(name: &str) -> Result<(f32, f32), String> {
    static SIZES: OnceLock<Mutex<HashMap<String, (f32, f32)>>> = OnceLock::new();
    let sizes = SIZES.get_or_init(|| Mutex::new(HashMap::new()));
    let data = bytes(name)?;
    // La clé contient la longueur du contenu : un fichier remplacé est relu
    let key = format!("{}:{}", name, data.len());
    if let Some(s) = sizes.lock().unwrap().get(&key) {
        return Ok(*s);
    }
    let size = if is_svg(name, &data) {
        let t = parse_svg(name, &data)?;
        (t.size().width(), t.size().height())
    } else {
        let r = png(name)?;
        (r.width as f32, r.height as f32)
    };
    sizes.lock().unwrap().insert(key, size);
    Ok(size)
}

/// PNG décodé (RGBA prémultiplié, comme le reste du dessin)
fn png(name: &str) -> Result<Arc<Raster>, String> {
    let key = (name.to_string(), 0, 0);
    if let Some(r) = rasters().lock().unwrap().get(&key) {
        return Ok(r.clone());
    }
    let data = bytes(name)?;
    let pm = tiny_skia::Pixmap::decode_png(&data)
        .map_err(|e| format!("image '{}' : PNG illisible ({}) ; formats acceptés : PNG et SVG", name, e))?;
    let r = Arc::new(Raster { width: pm.width(), height: pm.height(), data: pm.data().to_vec() });
    rasters().lock().unwrap().insert(key, r.clone());
    Ok(r)
}

/// SVG dessiné à w × h pixels
fn svg(name: &str, w: u32, h: u32) -> Result<Arc<Raster>, String> {
    let key = (name.to_string(), w, h);
    if let Some(r) = rasters().lock().unwrap().get(&key) {
        return Ok(r.clone());
    }
    let data = bytes(name)?;
    let tree = parse_svg(name, &data)?;
    let mut pm = tiny_skia::Pixmap::new(w, h).ok_or_else(|| format!("image '{}' : taille invalide", name))?;
    let s = tree.size();
    resvg::render(&tree, tiny_skia::Transform::from_scale(w as f32 / s.width(), h as f32 / s.height()), &mut pm.as_mut());
    let r = Arc::new(Raster { width: w, height: h, data: pm.take() });
    let mut cache = rasters().lock().unwrap();
    // Une animation de taille demande beaucoup de tailles : on ne garde pas tout
    if cache.len() > 64 {
        cache.retain(|k, _| k.1 == 0);
    }
    cache.insert(key, r.clone());
    Ok(r)
}

pub fn shapes(f: &ResolvedFigure, scene: &Scene, refs: &Refs, opacity: f32) -> Result<Vec<Shape>, String> {
    let name = fmt_value(require(f, "file")?);
    let (nw, nh) = natural_size(&name)?;
    if !(nw > 0.0 && nh > 0.0) {
        return Err(format!("image '{}' : taille nulle", name));
    }
    // `size` : le plus grand côté
    let size = length(require(f, "size")?, scene, scene.min_side())?;
    let (w, h) = if nw >= nh { (size, size * nh / nw) } else { (size * nw / nh, size) };
    let (x, y) = point(require(f, "pos")?, scene, refs)?;
    let (fx, fy) = super::geometry::anchor_of(f)?.unwrap_or((0.5, 0.5));
    let (left, top) = (x - fx * w, y - fy * h);
    // Coins dans l'ordre haut-gauche, haut-droit, bas-droit, bas-gauche : l'image suit
    // ce cadre même tourné (rotate) ou agrandi (scale, grow)
    let path = vec![
        PathCmd::MoveTo(left, top),
        PathCmd::LineTo(left + w, top),
        PathCmd::LineTo(left + w, top + h),
        PathCmd::LineTo(left, top + h),
        PathCmd::Close,
    ];
    let image = if super::pixels::geometry_only() {
        None
    } else if is_svg(&name, &bytes(&name)?) {
        let (pw, ph) = ((w.round() as u32).clamp(1, 8192), (h.round() as u32).clamp(1, 8192));
        Some(svg(&name, pw, ph)?)
    } else {
        Some(png(&name)?)
    };
    Ok(vec![Shape {
        path,
        fill: Some(Rgba { r: 1.0, g: 1.0, b: 1.0, a: opacity }),
        stroke: None,
        glow: 0.0,
        image,
    }])
}

/// Vérifications sans lire le fichier (le validateur ne connaît pas le dossier) : propriétés
pub fn check(props: &HashMap<String, PropValue>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for key in ["file", "pos", "size"] {
        if !props.contains_key(key) {
            out.push((key.to_string(), format!("propriété `{}` manquante (ex : file : schema.svg, pos : (50%, 50%), size : 40%)", key)));
        }
    }
    if let Some(v) = props.get("file") {
        let name = fmt_value(v).to_ascii_lowercase();
        if !(name.ends_with(".png") || name.ends_with(".svg") || name.ends_with(".svgz")) {
            out.push(("file".to_string(), format!("image '{}' : formats acceptés : .png et .svg", fmt_value(v))));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_et_png() {
        let svg_src = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
            <rect x="100" y="0" width="100" height="100" fill="#0000ff"/></svg>"##;
        register("test_image_a.svg", svg_src.to_vec());
        assert_eq!(natural_size("test_image_a.svg").unwrap(), (200.0, 100.0));
        let r = svg("test_image_a.svg", 40, 20).unwrap();
        assert_eq!((r.width, r.height), (40, 20));
        let px = |x: u32, y: u32| &r.data[((y * r.width + x) * 4) as usize..][..4];
        assert_eq!(px(5, 10), [255, 0, 0, 255]);
        assert_eq!(px(35, 10), [0, 0, 255, 255]);

        // PNG produit par tiny-skia : 3 × 2, vert
        let mut pm = tiny_skia::Pixmap::new(3, 2).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(0, 255, 0, 255));
        register("test_image_b.png", pm.encode_png().unwrap());
        assert_eq!(natural_size("test_image_b.png").unwrap(), (3.0, 2.0));
        assert_eq!(&png("test_image_b.png").unwrap().data[..4], [0, 255, 0, 255]);

        register("test_image_c.png", b"pas une image".to_vec());
        assert!(png("test_image_c.png").unwrap_err().contains("PNG illisible"));
        assert!(check(&HashMap::from([("file".to_string(), PropValue::Single("a.jpg".into()))]))
            .iter()
            .any(|(k, m)| k == "file" && m.contains(".png")));
    }
}
