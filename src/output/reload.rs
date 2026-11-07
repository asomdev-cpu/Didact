//! Rechargement à chaud : surveille un fichier .dct et le relit quand il est sauvegardé.
//!
//! On regarde simplement la date et la taille du fichier, quelques fois par seconde.
//! Un éditeur peut écrire un fichier en plusieurs fois : on attend qu'il soit stable
//! pendant deux regards de suite avant de le relire.

use crate::resolver::ResolvedDocument;
use crate::runtime::Timeline;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Intervalle entre deux regards
const POLL_EVERY: Duration = Duration::from_millis(100);

type Stamp = Option<(SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub struct Watcher {
    path: PathBuf,
    /// État du fichier à la dernière lecture
    loaded: Stamp,
    /// État vu au dernier regard, en attente de stabilité
    pending: Stamp,
    last_poll: Option<Instant>,
    /// Intervalle entre deux regards (0 dans les tests)
    every: Duration,
}

/// Résultat d'un rechargement : le nouveau document, ou les erreurs à afficher
pub type Reloaded = Result<(ResolvedDocument, Timeline), Vec<String>>;

impl Watcher {
    pub fn new(path: &Path) -> Watcher {
        let now = stamp(path);
        Watcher { path: path.to_path_buf(), loaded: now, pending: now, last_poll: None, every: POLL_EVERY }
    }

    /// À appeler souvent. Renvoie Some quand le fichier a changé et a été relu.
    pub fn poll(&mut self) -> Option<Reloaded> {
        if self.last_poll.is_some_and(|l| l.elapsed() < self.every) {
            return None;
        }
        self.last_poll = Some(Instant::now());

        let now = stamp(&self.path);
        if now.is_none() {
            return None; // fichier remplacé par l'éditeur : il va revenir
        }
        if now == self.loaded {
            self.pending = now;
            return None;
        }
        if now != self.pending {
            self.pending = now; // ça vient de changer : on attend le prochain regard
            return None;
        }
        self.loaded = now;
        let source = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) => return Some(Err(vec![format!("lecture de {} impossible : {}", self.path.display(), e)])),
        };
        if source.trim().is_empty() {
            return None; // fichier vidé pendant l'écriture
        }
        Some(crate::load(&source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "[figures]\ndeffigure a =\n    type : line\n[timeline]\n1.000 => { start : [a] }\n";
    const OK2: &str = "[figures]\ndeffigure a =\n    type : line\n[timeline]\n9.000 => { start : [a] }\n";
    const BAD: &str = "[figures]\n[timeline]\n1.000 => { start : [zz] }\n";

    /// Change le fichier avec une date différente (les dates de fichiers sont arrondies)
    fn write(path: &Path, text: &str, age: u64) {
        std::fs::write(path, text).unwrap();
        let time = SystemTime::now() + Duration::from_secs(age);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(time).unwrap();
    }

    fn poll_until_stable(w: &mut Watcher) -> Option<Reloaded> {
        w.poll().or_else(|| w.poll())
    }

    #[test]
    fn suit_les_sauvegardes() {
        let dir = std::env::temp_dir().join(format!("didact-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cours.dct");
        write(&path, OK, 1);
        let mut w = Watcher::new(&path);
        w.every = Duration::ZERO;

        // Rien n'a changé
        assert!(w.poll().is_none());

        // Sauvegarde : relue une seule fois, au regard suivant (fichier stable)
        write(&path, OK2, 2);
        assert!(w.poll().is_none());
        let (_, tl) = w.poll().expect("relu").ok().expect("valide");
        assert_eq!(tl.duration(), 9.0);
        assert!(w.poll().is_none());

        // Erreur : renvoyée avec sa ligne
        write(&path, BAD, 3);
        let errors = poll_until_stable(&mut w).expect("relu").err().expect("erreur attendue");
        assert!(errors[0].contains("ligne 3"), "{:?}", errors);

        // Fichier vidé par l'éditeur : ignoré, puis la version corrigée passe
        write(&path, "", 4);
        assert!(poll_until_stable(&mut w).is_none());
        write(&path, OK, 5);
        assert!(poll_until_stable(&mut w).expect("relu").is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
