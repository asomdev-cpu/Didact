//! `--voice` : génère les fichiers de voix avec un programme de synthèse vocale externe,
//! et retrouve ensuite ces fichiers pour les jouer.
//!
//! Le programme est décrit dans `didact-voix.txt` (à côté du .dct), une ligne par langue :
//!
//! ```text
//! FR : piper --model C:\voix\fr_FR-siwis-medium.onnx --output_file {out}
//! EN : piper --model C:\voix\en_US-lessac-medium.onnx --output_file {out}
//! ```
//!
//! Dans la commande : `{out}` = fichier .wav à écrire (obligatoire), `{text}` = la phrase,
//! `{text_file}` = un fichier texte contenant la phrase, `{lang}` = la langue.
//! La phrase est aussi envoyée sur l'entrée standard (c'est ce que lit Piper).

use crate::runtime::timeline::AudioSegment;
use crate::runtime::Timeline;
use crate::voice::{self, Holds};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Nom du fichier de configuration cherché à côté du .dct
pub const CONFIG_NAME: &str = "didact-voix.txt";

/// Commande de synthèse vocale par langue
pub type VoiceConfig = BTreeMap<String, String>;

/// Lit `didact-voix.txt` : `LANGUE : commande` par ligne, `#` = commentaire
pub fn read_config(path: &Path) -> Result<VoiceConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{} : {}", path.display(), e))?;
    let mut cfg = VoiceConfig::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (lang, cmd) = line
            .split_once(':')
            .filter(|(l, _)| !l.trim().is_empty() && !l.trim().contains(' '))
            .ok_or_else(|| format!("{} ligne {} : attendu « LANGUE : commande »", path.display(), i + 1))?;
        if !cmd.contains("{out}") {
            return Err(format!("{} ligne {} : la commande doit contenir {{out}} (le fichier .wav à écrire)", path.display(), i + 1));
        }
        cfg.insert(lang.trim().to_string(), cmd.trim().to_string());
    }
    Ok(cfg)
}

/// Cherche le fichier de configuration : chemin donné, sinon à côté du .dct, sinon dossier courant
pub fn find_config(explicit: Option<&Path>, dct: &Path) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    let beside = dct.parent().unwrap_or(Path::new("")).join(CONFIG_NAME);
    for p in [beside, PathBuf::from(CONFIG_NAME)] {
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(format!(
        "{} introuvable. Créez-le à côté du .dct, une ligne par langue, par exemple :\n\
         FR : piper --model fr_FR-siwis-medium.onnx --output_file {{out}}\n\
         (voir Readme/SPEC.md §11)",
        CONFIG_NAME
    ))
}

/// Dossier des voix d'un fichier .dct : `cours.dct` → `cours_voix/` à côté de lui
pub fn voice_dir(dct: &Path) -> PathBuf {
    let stem = dct.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    dct.parent().unwrap_or(Path::new("")).join(voice::dir_name(&stem))
}

/// Contenu de `index.txt` : fichiers générés et commande de chaque langue
#[derive(Default)]
struct Index {
    commands: BTreeMap<String, String>,
    /// Fichiers et durée lue (secondes)
    files: Vec<(String, Option<f64>)>,
}

fn read_index(dir: &Path) -> Index {
    let mut idx = Index::default();
    let Ok(text) = std::fs::read_to_string(dir.join("index.txt")) else { return idx };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("commande ") {
            if let Some((lang, cmd)) = rest.split_once(" : ") {
                idx.commands.insert(lang.to_string(), cmd.to_string());
            }
        } else if !line.trim().is_empty() && !line.starts_with('#') {
            let mut parts = line.split_whitespace();
            let file = parts.next().unwrap_or("").to_string();
            idx.files.push((file, parts.next().and_then(|d| d.parse().ok())));
        }
    }
    idx
}

fn write_index(dir: &Path, idx: &Index) -> Result<(), String> {
    let mut s = String::from("# Fichiers de voix générés par didact --voice (ne pas modifier)\n");
    for (lang, cmd) in &idx.commands {
        s.push_str(&format!("commande {} : {}\n", lang, cmd));
    }
    for (f, d) in &idx.files {
        match d {
            Some(d) => s.push_str(&format!("{} {:.3}\n", f, d)),
            None => s.push_str(&format!("{}\n", f)),
        }
    }
    std::fs::write(dir.join("index.txt"), s).map_err(|e| format!("index.txt : {}", e))
}

/// Résultat de la génération
pub struct VoiceReport {
    pub dir: PathBuf,
    pub generated: usize,
    pub reused: usize,
    /// Langues sans commande dans la configuration
    pub skipped_langs: Vec<String>,
    /// Phrases plus longues que l'écart avant le commentaire suivant
    pub warnings: Vec<String>,
}

/// Génère les voix manquantes de toutes les langues configurées.
/// `progress(fait, total, phrase)` est appelé avant chaque phrase à générer.
pub fn generate(
    timeline: &Timeline,
    dct: &Path,
    config: &VoiceConfig,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<VoiceReport, String> {
    let dir = voice_dir(dct);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{} : {}", dir.display(), e))?;
    let old = read_index(&dir);
    let mut idx = Index::default();
    let mut skipped_langs = Vec::new();

    // Ce qu'il faut générer
    let mut todo: Vec<(String, voice::Line)> = Vec::new();
    let mut reused = 0;
    for lang in timeline.languages() {
        let Some(cmd) = config.get(lang) else {
            skipped_langs.push(lang.clone());
            continue;
        };
        idx.commands.insert(lang.clone(), cmd.clone());
        // commande changée (autre voix) : tout est refait pour cette langue
        let same_cmd = old.commands.get(lang) == Some(cmd);
        let mut seen = HashSet::new();
        for line in voice::lines(timeline, lang) {
            if !seen.insert(line.file.clone()) {
                continue;
            }
            if same_cmd && dir.join(&line.file).is_file() {
                reused += 1;
                let d = old.files.iter().find(|(f, _)| *f == line.file).and_then(|(_, d)| *d);
                let d = d.or_else(|| super::audio::decode(&dir.join(&line.file)).ok().map(|c| c.duration()));
                idx.files.push((line.file.clone(), d));
            } else {
                todo.push((cmd.clone(), line));
            }
        }
    }

    // Génération
    let total = todo.len();
    for (i, (cmd, line)) in todo.iter().enumerate() {
        progress(i, total, &line.text);
        let lang = line.file.split('/').next().unwrap_or("");
        let out = dir.join(&line.file);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = voice::spoken_text(&line.text, lang);
        synthesize(cmd, &text, lang, &out).map_err(|e| format!("ligne {} ({}) : {}", line.line, lang, e))?;
        let d = super::audio::decode(&out).map_err(|e| format!("ligne {} ({}) : fichier produit illisible : {}", line.line, lang, e))?;
        idx.files.push((line.file.clone(), Some(d.duration())));
        // index à jour après chaque phrase : une interruption ne perd rien
        write_index(&dir, &idx)?;
    }
    write_index(&dir, &idx)?;

    // Phrases trop longues : une ligne par phrase, puis le total par langue
    let mut warnings = Vec::new();
    for lang in timeline.languages() {
        let durations: BTreeMap<&str, f64> =
            idx.files.iter().filter_map(|(f, d)| Some((f.as_str(), (*d)?))).collect();
        let mut count = 0;
        for l in voice::lines(timeline, lang) {
            let Some(&said) = durations.get(l.file.as_str()) else { continue };
            let gap = l.next - l.t;
            if l.t + said + voice::BREATH > l.next {
                count += 1;
                warnings.push(format!(
                    "ligne {} ({}) : la phrase lue dure {:.1} s, mais le commentaire suivant arrive {:.1} s après → l'animation attend {:.1} s",
                    l.line,
                    lang,
                    said,
                    gap,
                    said + voice::BREATH - gap
                ));
            }
        }
        if count > 0 {
            let holds = plan(timeline, dct, lang).holds;
            warnings.push(format!(
                "{} : {} phrase(s) trop longue(s), l'animation est allongée de {:.1} s au total",
                lang,
                count,
                holds.extra()
            ));
        }
    }
    Ok(VoiceReport { dir, generated: total, reused, skipped_langs, warnings })
}

/// Lance la commande pour une phrase
fn synthesize(cmd: &str, text: &str, lang: &str, out: &Path) -> Result<(), String> {
    let text_file = std::env::temp_dir().join(format!("didact_voix_{}.txt", std::process::id()));
    std::fs::write(&text_file, text).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(out);
    let args: Vec<String> = split_command(cmd)
        .into_iter()
        .map(|a| {
            a.replace("{out}", &out.to_string_lossy())
                .replace("{text_file}", &text_file.to_string_lossy())
                .replace("{text}", text)
                .replace("{lang}", lang)
        })
        .collect();
    let (program, rest) = args.split_first().ok_or("commande vide")?;
    let mut child = Command::new(program)
        .args(rest)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("programme '{}' introuvable (vérifiez {} et le PATH)", program, CONFIG_NAME)
            } else {
                format!("impossible de lancer '{}' : {}", program, e)
            }
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes()); // le programme peut ne pas lire l'entrée
        let _ = stdin.write_all(b"\n");
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&text_file);
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").trim().to_string();
        return Err(format!("'{}' a échoué ({}) {}", program, output.status, last));
    }
    if !out.is_file() {
        return Err(format!("'{}' n'a pas écrit {} (la commande utilise-t-elle {{out}} ?)", program, out.display()));
    }
    Ok(())
}

/// Découpe une commande en arguments (les "guillemets" regroupent, comme dans un terminal)
fn split_command(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    for c in cmd.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    args
}

/// Voix d'une langue, prêtes à jouer
#[derive(Debug, Clone, Default)]
pub struct VoicePlan {
    /// Phrases déjà générées, aux instants de la timeline. Chemins relatifs au dossier du .dct.
    pub segments: Vec<AudioSegment>,
    /// Pauses de l'animation pour laisser finir les phrases
    pub holds: Holds,
}

/// Voix d'une langue : fichiers présents dans `cours_voix/` et pauses nécessaires
pub fn plan(timeline: &Timeline, dct: &Path, lang: &str) -> VoicePlan {
    let dir = voice_dir(dct);
    let dir_name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let idx = read_index(&dir);
    let lines: Vec<voice::Line> = voice::lines(timeline, lang).into_iter().filter(|l| dir.join(&l.file).is_file()).collect();
    let duration_of = |file: &str| -> Option<f64> {
        idx.files
            .iter()
            .find(|(f, _)| f == file)
            .and_then(|(_, d)| *d)
            .or_else(|| super::audio::decode(&dir.join(file)).ok().map(|c| c.duration()))
    };
    let holds = Holds::new(&lines, duration_of);
    let segments = lines
        .iter()
        .map(|l| AudioSegment {
            file: format!("{}/{}", dir_name, l.file),
            volume: 1.0,
            start: l.t,
            end: None,
            looping: false,
        })
        .collect();
    VoicePlan { segments, holds }
}

/// Pistes passées en temps réel (pauses comprises)
pub fn real_time(segments: &[AudioSegment], holds: &Holds) -> Vec<AudioSegment> {
    segments
        .iter()
        .map(|s| AudioSegment {
            start: holds.to_real(s.start),
            end: s.end.map(|e| holds.to_real(e)),
            ..s.clone()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoupage_commande() {
        assert_eq!(
            split_command(r#"piper --model "C:\mes voix\fr.onnx" --output_file {out}"#),
            vec!["piper", "--model", r"C:\mes voix\fr.onnx", "--output_file", "{out}"]
        );
    }

    #[test]
    fn configuration() {
        let dir = std::env::temp_dir().join(format!("didact_voixcfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(CONFIG_NAME);
        std::fs::write(&p, "# voix\nFR : piper --model fr.onnx --output_file {out}\n\nEN : say -o {out}\n").unwrap();
        let cfg = read_config(&p).unwrap();
        assert_eq!(cfg.len(), 2);
        assert!(cfg["FR"].starts_with("piper"));
        std::fs::write(&p, "FR : piper sans sortie\n").unwrap();
        assert!(read_config(&p).unwrap_err().contains("{out}"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
