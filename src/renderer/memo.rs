//! Cache des calculs coûteux d'une image à l'autre (courbes de niveau, régions, surfaces).
//!
//! La clé décrit tout ce dont le résultat dépend (propriétés après paramètres, valeurs des
//! paramètres, repère, taille de l'image) : si rien n'a changé, le résultat précédent est
//! réutilisé tel quel. Le résultat est donc identique avec ou sans cache.

use crate::ast::PropValue;
use std::any::Any;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Nombre d'entrées gardées (vidé entièrement au-delà : simple et suffisant)
const MAX_ENTRIES: usize = 256;

thread_local! {
    static CACHE: RefCell<HashMap<(String, u64), Rc<dyn Any>>> = RefCell::new(HashMap::new());
}

/// Empreinte de propriétés (dans l'ordre des noms) et d'autres éléments décrits en texte.
/// Seuls les paramètres dont le nom apparaît dans les propriétés comptent : animer un
/// paramètre ne fait pas recalculer les figures qui ne l'utilisent pas.
pub fn key(props: &HashMap<String, PropValue>, params: &HashMap<String, f64>, extra: &str) -> u64 {
    let mut h = DefaultHasher::new();
    let mut keys: Vec<&String> = props.keys().collect();
    keys.sort();
    // Texte où un paramètre peut apparaître (pas le type, ni les noms d'autres figures)
    let mut all = String::new();
    for k in keys {
        k.hash(&mut h);
        let v = format!("{:?}", props[k]);
        v.hash(&mut h);
        if !matches!(k.as_str(), "type" | "axes" | "scene" | "style" | "on" | "colormap" | "layout" | "color" | "fill") {
            all.push_str(&v);
            all.push(' ');
        }
    }
    // Simple présence du nom : prudent (« 2k », « ak » utilisent k)
    let used = |name: &str| all.contains(name);
    let mut names: Vec<&String> = params.keys().filter(|n| used(n)).collect();
    names.sort();
    for n in names {
        n.hash(&mut h);
        params[n].to_bits().hash(&mut h);
    }
    extra.hash(&mut h);
    h.finish()
}

/// Résultat en cache pour (nom, clé), sinon calculé puis gardé
pub fn get_or<T: Clone + 'static>(name: &str, key: u64, compute: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let id = (name.to_string(), key);
    if let Some(v) = CACHE.with(|c| c.borrow().get(&id).cloned()) {
        if let Some(v) = v.downcast_ref::<T>() {
            return Ok(v.clone());
        }
    }
    let v = compute()?;
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= MAX_ENTRIES {
            c.clear();
        }
        // Une seule entrée par figure : l'ancienne version ne resservira pas
        c.retain(|k, _| k.0 != name);
        c.insert(id, Rc::new(v.clone()));
    });
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recalcul_seulement_si_la_cle_change() {
        let mut calls = 0;
        let mut run = |k: u64| get_or("t", k, || {
            calls += 1;
            Ok(k * 2)
        }).unwrap();
        assert_eq!(run(1), 2);
        assert_eq!(run(1), 2);
        assert_eq!(run(3), 6);
        assert_eq!(calls, 2);
    }
}
