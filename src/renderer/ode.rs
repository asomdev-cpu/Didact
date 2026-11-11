//! `trajectory` : solutions d'équations différentielles, x' = f(x, t), résolues par Runge–Kutta 4.
//!
//! ```text
//! deffigure pendule =
//!     type     : trajectory
//!     axes     : r              # ou scene : espace (3 variables)
//!     ode      : (y, -sin(x) - 0.2*y)
//!     init     : (2.5, 0)
//!     duration : t              # temps d'intégration (un paramètre animé la fait grandir)
//! ```
//!
//! Avec `count : n`, n trajectoires, `init` dépendant de `k` (1 à n) : portrait de phase,
//! nuage de particules (`init : (randn(k), randn(k, 1))`, `trail : 0.5`, `head : 4px`).
//!
//! Les solutions sont gardées d'une image à l'autre : quand `duration` grandit, elles sont
//! recalculées seulement quand elle dépasse la durée déjà calculée (qui double alors).

use super::expr::{self, compile, Compiled};
use crate::ast::PropValue;
use crate::resolver::{fmt_value, ResolvedFigure};
use std::collections::HashMap;
use std::sync::Arc;

pub type V3 = [f64; 3];

/// Points calculés au plus (trajectoires × pas)
const MAX_POINTS: usize = 6_000_000;

/// Éléments d'un tuple `(a, b, c)` (découpé au niveau 0 des parenthèses)
fn parts(v: &PropValue) -> Vec<String> {
    match v {
        PropValue::Tuple(items) | PropValue::List(items) => items.iter().map(|s| s.trim().to_string()).collect(),
        other => {
            let s = fmt_value(other);
            let s = s.trim();
            let inner = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')).unwrap_or(s);
            let call = format!("t({})", inner);
            super::geometry::split_call(&call).1.iter().map(|s| s.trim().to_string()).collect()
        }
    }
}

fn number(f: &ResolvedFigure, key: &str, default: f64) -> Result<f64, String> {
    match f.properties.get(key) {
        None => Ok(default),
        Some(PropValue::Number(n)) => Ok(*n),
        Some(v) => fmt_value(v).trim().parse().map_err(|_| format!("{} : nombre attendu, trouvé '{}'", key, fmt_value(v))),
    }
}

/// Le système : noms des variables, dérivées, conditions initiales (fonctions de k)
struct System {
    vars: Vec<String>,
    deriv: Vec<Compiled>,
    init: Vec<Compiled>,
    count: usize,
    dt: f64,
}

fn system(f: &ResolvedFigure, params: &HashMap<String, f64>, dims: usize) -> Result<System, String> {
    let vars: Vec<String> = match f.properties.get("vars") {
        Some(v) => parts(v),
        None => ["x", "y", "z"][..dims].iter().map(|s| s.to_string()).collect(),
    };
    if !(1..=6).contains(&vars.len()) {
        return Err("vars : de 1 à 6 variables, ex. (x, y)".to_string());
    }
    let ode = parts(f.properties.get("ode").ok_or("trajectory : `ode : (x', y')` manquant, ex. (y, -sin(x))")?);
    let init = parts(f.properties.get("init").ok_or("trajectory : `init : (x0, y0)` manquant")?);
    if ode.len() != vars.len() || init.len() != vars.len() {
        return Err(format!(
            "trajectory : {} variables ({}) : `ode` et `init` doivent en avoir autant",
            vars.len(),
            vars.join(", ")
        ));
    }
    let mut slots = vars.clone();
    slots.push("t".into());
    // Noms reconnus dans les formules : les paramètres, les variables (sauf x, déjà connu), t
    let mut names: Vec<String> = params.keys().cloned().collect();
    names.extend(slots.iter().filter(|s| *s != "x").cloned());
    let deriv = ode
        .iter()
        .map(|s| expr::parse_with(s, &names).and_then(|e| compile(&e, &slots, params)).map_err(|e| format!("ode '{}' : {}", s, e)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut knames: Vec<String> = params.keys().cloned().collect();
    knames.push("k".into());
    let kslots = vec!["k".to_string()];
    let init = init
        .iter()
        .map(|s| expr::parse_with(s, &knames).and_then(|e| compile(&e, &kslots, params)).map_err(|e| format!("init '{}' : {}", s, e)))
        .collect::<Result<Vec<_>, _>>()?;
    let count = number(f, "count", 1.0)?.round();
    if !(1.0..=20_000.0).contains(&count) {
        return Err("count : entre 1 et 20000 trajectoires".to_string());
    }
    let dt = number(f, "dt", 0.01)?;
    if !(dt > 0.0) {
        return Err("dt doit être positif".to_string());
    }
    Ok(System { vars, deriv, init, count: count as usize, dt })
}

/// Intègre chaque trajectoire sur `steps` pas (RK4) ; s'arrête si la solution explose
fn integrate(sys: &System, steps: usize) -> Vec<Vec<V3>> {
    let n = sys.vars.len();
    let dt = sys.dt;
    let f = |s: &[f64], t: f64, out: &mut [f64]| {
        let mut v = [0.0; 7];
        v[..n].copy_from_slice(&s[..n]);
        v[n] = t;
        for (o, d) in out.iter_mut().zip(&sys.deriv) {
            *o = d(&v[..=n]);
        }
    };
    let mut curves = Vec::with_capacity(sys.count);
    for k in 1..=sys.count {
        let mut s = [0.0f64; 6];
        for (i, init) in sys.init.iter().enumerate() {
            s[i] = init(&[k as f64]);
        }
        let mut pts = Vec::with_capacity(steps + 1);
        let mut t = 0.0;
        let point = |s: &[f64]| [s[0], *s.get(1).unwrap_or(&0.0), if n >= 3 { s[2] } else { 0.0 }];
        if s[..n].iter().all(|v| v.is_finite()) {
            pts.push(point(&s));
            let (mut k1, mut k2, mut k3, mut k4, mut tmp) = ([0.0; 6], [0.0; 6], [0.0; 6], [0.0; 6], [0.0; 6]);
            for _ in 0..steps {
                f(&s, t, &mut k1[..n]);
                for i in 0..n {
                    tmp[i] = s[i] + 0.5 * dt * k1[i];
                }
                f(&tmp, t + 0.5 * dt, &mut k2[..n]);
                for i in 0..n {
                    tmp[i] = s[i] + 0.5 * dt * k2[i];
                }
                f(&tmp, t + 0.5 * dt, &mut k3[..n]);
                for i in 0..n {
                    tmp[i] = s[i] + dt * k3[i];
                }
                f(&tmp, t + dt, &mut k4[..n]);
                for i in 0..n {
                    s[i] += dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
                }
                t += dt;
                if !s[..n].iter().all(|v| v.is_finite() && v.abs() < 1e9) {
                    break;
                }
                pts.push(point(&s));
            }
        }
        curves.push(pts);
    }
    curves
}

/// Trajectoires à l'instant de la figure : de max(0, duration - trail) à duration.
/// Renvoie aussi le nombre de variables.
pub fn curves(f: &ResolvedFigure, params: &HashMap<String, f64>, dims: usize) -> Result<(Vec<Vec<V3>>, usize), String> {
    let sys = system(f, params, dims)?;
    let duration = number(f, "duration", 10.0)?.max(0.0);
    let need = (duration / sys.dt).round() as usize;
    if need.saturating_mul(sys.count) > MAX_POINTS {
        return Err(format!(
            "trajectory : {} trajectoires × {} pas, c'est trop (au plus {}) : augmenter dt ou réduire count",
            sys.count, need, MAX_POINTS
        ));
    }
    // Durée calculée : la puissance de 2 (en pas) au-dessus, pour ne pas tout refaire à chaque image
    let horizon = need.max(64).next_power_of_two().min(MAX_POINTS / sys.count.max(1)).max(need);
    let mut deps: HashMap<String, PropValue> = HashMap::new();
    for k in ["vars", "ode", "init", "count", "dt"] {
        if let Some(v) = f.properties.get(k) {
            deps.insert(k.to_string(), v.clone());
        }
    }
    let key = super::memo::key(&deps, params, &format!("{} {}", dims, horizon));
    let all: Arc<Vec<Vec<V3>>> = super::memo::get_or(&format!("ode:{}", f.name), key, || Ok(Arc::new(integrate(&sys, horizon))))?;
    let trail = match f.properties.get("trail") {
        None => None,
        Some(_) => Some(number(f, "trail", 0.0)?.max(0.0)),
    };
    let from = match trail {
        Some(tr) => need.saturating_sub((tr / sys.dt).round() as usize),
        None => 0,
    };
    let out = all
        .iter()
        .map(|c| {
            let end = need.min(c.len().saturating_sub(1));
            if c.is_empty() || from > end { Vec::new() } else { c[from..=end].to_vec() }
        })
        .collect();
    Ok((out, sys.vars.len()))
}

/// Ligne brisée allégée : au plus `max` points (garde le premier et le dernier)
pub fn thin(pts: &[V3], max: usize) -> Vec<V3> {
    if pts.len() <= max || max < 2 {
        return pts.to_vec();
    }
    let step = (pts.len() - 1) as f64 / (max - 1) as f64;
    (0..max).map(|i| pts[((i as f64 * step).round() as usize).min(pts.len() - 1)]).collect()
}

/// Vérification (validateur)
pub fn check(props: &HashMap<String, PropValue>, params: &[String]) -> Vec<(String, String)> {
    let vals: HashMap<String, f64> = params.iter().map(|p| (p.clone(), 1.0)).collect();
    let f = ResolvedFigure {
        name: String::new(),
        index: 0,
        ftype: "trajectory".into(),
        layer: 0.0,
        line: 0,
        properties: props.clone(),
        prop_lines: HashMap::new(),
    };
    // Expressions de paramètres (iterations : 100 + 80*k) calculées comme au rendu
    let f = super::params::bind(&f, &vals).ok().flatten().unwrap_or(f);
    let dims = if props.contains_key("scene") { 3 } else { 2 };
    let mut errs = Vec::new();
    if !props.contains_key("axes") && !props.contains_key("scene") {
        errs.push(("type".to_string(), "trajectory : `axes : repere` (2D) ou `scene : espace` (3D) manquant".to_string()));
    }
    if let Err(e) = system(&f, &vals, dims) {
        let key = ["ode", "init"].into_iter().find(|k| e.starts_with(*k)).unwrap_or("type");
        errs.push((key.to_string(), e));
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fig(props: &[(&str, PropValue)]) -> ResolvedFigure {
        let mut f = ResolvedFigure {
            name: "o".into(),
            index: 0,
            ftype: "trajectory".into(),
            layer: 0.0,
            line: 1,
            properties: HashMap::new(),
            prop_lines: HashMap::new(),
        };
        for (k, v) in props {
            f.properties.insert(k.to_string(), v.clone());
        }
        f
    }

    fn tuple(v: &[&str]) -> PropValue {
        PropValue::Tuple(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn oscillateur_harmonique() {
        // x'' = -x, x(0) = 1 : x(t) = cos t ; après 2π, retour au point de départ (RK4 : très précis)
        let f = fig(&[
            ("ode", tuple(&["y", "-x"])),
            ("init", tuple(&["1", "0"])),
            ("duration", PropValue::Number(std::f64::consts::TAU)),
            ("dt", PropValue::Number(0.01)),
        ]);
        let (c, n) = curves(&f, &HashMap::new(), 2).unwrap();
        assert_eq!(n, 2);
        // 628 pas de 0,01 : t = 6,28
        let last = c[0].last().unwrap();
        assert!((last[0] - 6.28f64.cos()).abs() < 1e-8 && (last[1] + 6.28f64.sin()).abs() < 1e-8, "{:?}", last);
        // Le long du chemin : x = cos t
        let mid = c[0][157];
        assert!((mid[0] - (1.57f64).cos()).abs() < 1e-8);
    }

    #[test]
    fn plusieurs_trajectoires_trainee_et_temps() {
        let f = fig(&[
            ("ode", tuple(&["-x + t*0", "-y"])),
            ("init", tuple(&["k", "1"])),
            ("count", PropValue::Number(3.0)),
            ("duration", PropValue::Number(1.0)),
            ("trail", PropValue::Number(0.5)),
            ("dt", PropValue::Number(0.01)),
        ]);
        let (c, _) = curves(&f, &HashMap::new(), 2).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[2].len(), 51); // de t = 0,5 à t = 1
        // k va de 1 à n : la troisième part de x = 3
        assert!((c[2].last().unwrap()[0] - 3.0 * (-1f64).exp()).abs() < 1e-8);
    }

    #[test]
    fn lorenz_en_3d() {
        let f = fig(&[
            ("ode", tuple(&["10*(y - x)", "x*(28 - z) - y", "x*y - 8/3*z"])),
            ("init", tuple(&["1", "1", "1"])),
            ("duration", PropValue::Number(5.0)),
        ]);
        let (c, n) = curves(&f, &HashMap::new(), 3).unwrap();
        assert_eq!(n, 3);
        assert_eq!(c[0].len(), 501);
        assert!(c[0].iter().all(|p| p[2] >= 0.0 && p[2] < 60.0));
    }

    #[test]
    fn erreurs() {
        let e = |props: &[(&str, PropValue)]| system(&fig(props), &HashMap::new(), 2).err().unwrap();
        assert!(e(&[("init", tuple(&["1", "0"]))]).contains("ode"));
        assert!(e(&[("ode", tuple(&["y"])), ("init", tuple(&["1", "0"]))]).contains("autant"));
        assert!(e(&[("ode", tuple(&["y", "w"])), ("init", tuple(&["1", "0"]))]).contains("nom inconnu"));
    }
}
