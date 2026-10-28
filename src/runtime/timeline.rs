//! Gestion de la timeline.
//!
//! Principe : au chargement, on rejoue une fois toute la timeline et on garde
//! un **instantané** (snapshot) de l'état de la scène après chaque bloc.
//! Ensuite, connaître l'état à l'instant t revient à trouver le dernier bloc
//! dont le timestamp est ≤ t (recherche dichotomique) et lire son instantané.
//!
//! Les animations sont planifiées au chargement (début, durée) et rangées par figure.
//! Leur effet à l'instant t est calculé par le renderer (qui connaît la géométrie).
//!
//! Avantages :
//! - l'état à t ne dépend que de t, pas des frames précédentes
//!   → avancer, reculer ou sauter n'importe où (scrubber) est trivial ;
//! - le coût par frame est O(log n), quel que soit le nombre de blocs.

use super::animation::{self, Anim, AnimKind};
use crate::ast::{AnimCall, StartItem, TimelineAction};
use crate::resolver::{ResolvedDocument, ResolvedFigure};
use std::collections::HashMap;

/// État d'une figure à un instant donné
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// Jamais démarrée (ou pas encore)
    NotStarted,
    /// À l'écran
    Visible,
    /// Cachée par `end`, mais toujours en mémoire (peut être redémarrée)
    Hidden,
    /// Supprimée par `destroy`
    Destroyed,
}

/// État complet de la scène juste après un bloc de la timeline
#[derive(Debug, Clone)]
struct Snapshot {
    /// Statut de chaque figure ; une figure absente est NotStarted
    status: HashMap<String, Status>,
    /// Figures cachées par `end` avec une animation de sortie :
    /// elles restent affichées jusqu'à la fin de cette animation
    exits: HashMap<String, f64>,
    /// Dernier `start` de chaque figure
    last_start: HashMap<String, f64>,
    /// Dernier commentaire affiché (toutes langues), reste jusqu'au suivant
    comment: Option<Vec<String>>,
    /// Pistes audio en cours : fichier → (volume, instant de démarrage)
    audio: HashMap<String, (f64, f64)>,
}

/// Une piste audio en cours de lecture
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrack {
    pub file: String,
    pub volume: f64,
    /// Position de lecture dans le fichier (secondes depuis son `start`)
    pub position: f64,
}

/// Une figure à l'instant t, avec ses animations déjà commencées
#[derive(Debug)]
pub struct FigureState<'a> {
    pub fig: &'a ResolvedFigure,
    /// À dessiner (visible, ou en train de sortir avec une animation)
    pub visible: bool,
    /// Animations commencées à t, dans l'ordre de début
    pub anims: Vec<&'a Anim>,
    /// Instant du dernier `start` (les fondus d'avant ne comptent plus)
    pub last_start: f64,
}

/// Ce qu'il faut afficher à l'instant t
#[derive(Debug)]
pub struct Frame<'a> {
    pub t: f64,
    /// Figures à dessiner, dans l'ordre de dessin (layer croissant)
    pub visible: Vec<&'a ResolvedFigure>,
    /// Toutes les figures non détruites, dans l'ordre de définition
    /// (les références dynamiques ont besoin des figures cachées aussi)
    pub states: Vec<FigureState<'a>>,
    /// Commentaire dans la langue choisie
    pub comment: Option<&'a str>,
    /// Pistes audio en cours
    pub audio: Vec<AudioTrack>,
}

pub struct Timeline {
    /// Toutes les figures, dans l'ordre de dessin
    figures: Vec<ResolvedFigure>,
    /// Indices dans `figures`, dans l'ordre de définition
    definition_order: Vec<usize>,
    /// Animations de chaque figure, triées par début
    anims: HashMap<String, Vec<Anim>>,
    /// Timestamps des blocs, triés
    times: Vec<f64>,
    /// snapshots[i] = état juste après le bloc times[i]
    snapshots: Vec<Snapshot>,
    /// Langues dans l'ordre de [config]
    languages: Vec<String>,
    default_lang: String,
}

/// Planifie un appel d'animation qui commence à `start`. Renvoie sa durée.
/// Les erreurs ont déjà été signalées par le validateur : un appel invalide est ignoré.
fn schedule(anims: &mut HashMap<String, Vec<Anim>>, figure: &str, call: &AnimCall, start: f64) -> f64 {
    match animation::parse_call(call) {
        Ok((kind, duration, ease)) => {
            anims.entry(figure.to_string()).or_default().push(Anim {
                kind,
                start,
                duration,
                ease,
                line: call.line,
            });
            duration
        }
        Err(_) => 0.0,
    }
}

impl Timeline {
    /// Construit la timeline en rejouant tous les blocs une fois.
    pub fn new(doc: &ResolvedDocument) -> Timeline {
        let mut times = Vec::new();
        let mut snapshots = Vec::new();
        let mut anims: HashMap<String, Vec<Anim>> = HashMap::new();

        let mut current = Snapshot {
            status: HashMap::new(),
            exits: HashMap::new(),
            last_start: HashMap::new(),
            comment: None,
            audio: HashMap::new(),
        };

        // doc.timeline est déjà triée par le résolveur
        for ev in &doc.timeline {
            let t = ev.timestamp;
            // Ordre fixe : destroy → end → start → anim → comment (SPEC §6.3)
            // On parcourt le bloc une fois par commande.
            for located in &ev.actions {
                if let TimelineAction::Destroy(names) = &located.action {
                    for name in names {
                        current.status.insert(name.clone(), Status::Destroyed);
                        current.exits.remove(name);
                    }
                }
            }
            for located in &ev.actions {
                if let TimelineAction::End(items) = &located.action {
                    for item in items {
                        if item.audio {
                            current.audio.remove(&item.name);
                            continue;
                        }
                        current.status.insert(item.name.clone(), Status::Hidden);
                        if let Some(call) = &item.anim {
                            let d = schedule(&mut anims, &item.name, call, t);
                            current.exits.insert(item.name.clone(), t + d);
                        }
                    }
                }
            }
            for located in &ev.actions {
                if let TimelineAction::Start(items) = &located.action {
                    for item in items {
                        match item {
                            StartItem::Figure { name, anim } => {
                                current.status.insert(name.clone(), Status::Visible);
                                current.exits.remove(name);
                                current.last_start.insert(name.clone(), t);
                                if let Some(call) = anim {
                                    schedule(&mut anims, name, call, t);
                                }
                            }
                            StartItem::Audio { file, volume } => {
                                current.audio.insert(file.clone(), (volume.unwrap_or(1.0), t));
                            }
                        }
                    }
                }
            }
            for located in &ev.actions {
                if let TimelineAction::Anim(items) = &located.action {
                    for item in items {
                        // Animations enchaînées : chacune commence à la fin de la précédente
                        let mut start = t;
                        for call in &item.calls {
                            start += schedule(&mut anims, &item.figure, call, start);
                        }
                    }
                }
            }
            for located in &ev.actions {
                if let TimelineAction::Comment(texts) = &located.action {
                    current.comment = Some(texts.clone());
                }
            }

            times.push(t);
            snapshots.push(current.clone());
        }

        for list in anims.values_mut() {
            list.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        }

        let mut definition_order: Vec<usize> = (0..doc.figures.len()).collect();
        definition_order.sort_by_key(|&i| doc.figures[i].index);

        Timeline {
            figures: doc.figures.clone(),
            definition_order,
            anims,
            times,
            snapshots,
            languages: doc.config.languages.clone(),
            default_lang: doc.config.default_lang.clone(),
        }
    }

    /// Fin de la timeline : dernier bloc, ou fin de la dernière animation si elle est plus tardive
    pub fn duration(&self) -> f64 {
        let last_block = self.times.last().copied().unwrap_or(0.0);
        let last_anim = self
            .anims
            .values()
            .flatten()
            .map(|a| a.end())
            .fold(0.0, f64::max);
        last_block.max(last_anim)
    }

    /// Timestamps de tous les blocs, triés
    pub fn times(&self) -> &[f64] {
        &self.times
    }

    /// Animations prévues mais pas encore supportées : (figure, nom), sans doublon
    pub fn unsupported_anims(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .anims
            .iter()
            .flat_map(|(fig, list)| {
                list.iter().filter_map(move |a| match &a.kind {
                    AnimKind::Unsupported(n) => Some((fig.clone(), n.clone())),
                    _ => None,
                })
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Index du dernier bloc dont le timestamp est ≤ t (None si t est avant le premier bloc)
    fn snapshot_index(&self, t: f64) -> Option<usize> {
        // partition_point : nombre de blocs avec timestamp ≤ t (recherche dichotomique)
        let n = self.times.partition_point(|&x| x <= t);
        if n == 0 { None } else { Some(n - 1) }
    }

    /// Statut d'une figure à l'instant t
    pub fn status_at(&self, name: &str, t: f64) -> Status {
        match self.snapshot_index(t) {
            None => Status::NotStarted,
            Some(i) => self.snapshots[i].status.get(name).copied().unwrap_or(Status::NotStarted),
        }
    }

    /// Ce qu'il faut afficher à l'instant t, dans la langue `lang`
    /// (None → default_lang ; langue inconnue → default_lang).
    pub fn frame_at(&self, t: f64, lang: Option<&str>) -> Frame<'_> {
        let snap = match self.snapshot_index(t) {
            None => {
                return Frame { t, visible: Vec::new(), states: Vec::new(), comment: None, audio: Vec::new() };
            }
            Some(i) => &self.snapshots[i],
        };

        let shown = |name: &str| match snap.status.get(name) {
            Some(Status::Visible) => true,
            Some(Status::Hidden) => snap.exits.get(name).is_some_and(|&until| t < until),
            _ => false,
        };

        let visible = self.figures.iter().filter(|f| shown(&f.name)).collect();

        let states = self
            .definition_order
            .iter()
            .map(|&i| &self.figures[i])
            .filter(|f| snap.status.get(&f.name) != Some(&Status::Destroyed))
            .map(|f| FigureState {
                fig: f,
                visible: shown(&f.name),
                anims: self
                    .anims
                    .get(&f.name)
                    .map(|list| list.iter().filter(|a| a.start <= t).collect())
                    .unwrap_or_default(),
                last_start: snap.last_start.get(&f.name).copied().unwrap_or(0.0),
            })
            .collect();

        let lang = lang.unwrap_or(&self.default_lang);
        let lang_index = self
            .languages
            .iter()
            .position(|l| l == lang)
            .or_else(|| self.languages.iter().position(|l| *l == self.default_lang))
            .unwrap_or(0);
        let comment = snap
            .comment
            .as_ref()
            .and_then(|texts| texts.get(lang_index))
            .map(|s| s.as_str());

        let mut audio: Vec<AudioTrack> = snap
            .audio
            .iter()
            .map(|(file, &(volume, start))| AudioTrack { file: file.clone(), volume, position: t - start })
            .collect();
        audio.sort_by(|a, b| a.file.cmp(&b.file));

        Frame { t, visible, states, comment, audio }
    }
}

// ─────────────────────────────────────────────
//  TESTS  (cargo test)
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser, resolver, validator};

    fn timeline(src: &str) -> Timeline {
        let doc = parser::parse(lexer::tokenize(src)).expect("le parsing doit réussir");
        validator::validate(&doc).expect("la validation doit réussir");
        Timeline::new(&resolver::resolve(&doc))
    }

    fn visibles(tl: &Timeline, t: f64) -> Vec<String> {
        tl.frame_at(t, None).visible.iter().map(|f| f.name.clone()).collect()
    }

    const SIMPLE: &str = "[figures]
        deffigure a =
            type : line
        deffigure b =
            type : circle
        [timeline]
        1.000 => { start : [a] }
        2.000 => { start : [b] }
        3.000 => { end : [a] }
        4.000 => { start : [a] }
        5.000 => { destroy : [b] }";

    #[test]
    fn rien_avant_le_premier_bloc() {
        let tl = timeline(SIMPLE);
        assert!(visibles(&tl, 0.5).is_empty());
        assert_eq!(tl.status_at("a", 0.5), Status::NotStarted);
    }

    #[test]
    fn start_end_restart_destroy() {
        let tl = timeline(SIMPLE);
        assert_eq!(visibles(&tl, 1.0), ["a"]); // pile sur le timestamp : déjà visible
        assert_eq!(visibles(&tl, 2.5), ["a", "b"]);
        assert_eq!(visibles(&tl, 3.0), ["b"]);
        assert_eq!(tl.status_at("a", 3.5), Status::Hidden);
        assert_eq!(visibles(&tl, 4.2), ["a", "b"]); // redémarrée après end
        assert_eq!(tl.status_at("b", 5.0), Status::Destroyed);
        assert_eq!(visibles(&tl, 100.0), ["a"]); // après le dernier bloc, l'état reste
    }

    #[test]
    fn on_peut_reculer() {
        let tl = timeline(SIMPLE);
        assert_eq!(visibles(&tl, 4.5), ["a", "b"]);
        assert_eq!(visibles(&tl, 1.5), ["a"]);
        assert_eq!(visibles(&tl, 3.5), ["b"]);
    }

    #[test]
    fn ordre_de_dessin_par_layer() {
        let tl = timeline(
            "[figures]
            deffigure dessus =
                type  : circle
                layer : 1
            deffigure dessous =
                type : rect
            [timeline]
            0.000 => { start : [dessus, dessous] }",
        );
        assert_eq!(visibles(&tl, 0.0), ["dessous", "dessus"]);
    }

    #[test]
    fn commentaires_multilingues() {
        let tl = timeline(
            "[config]
            languages    : [FR, NL, EN]
            default_lang : FR
            [figures]
            [timeline]
            1.000 => { comment : [\"un\", \"een\", \"one\"] }
            2.000 => { comment : [\"deux\", \"twee\", \"two\"] }",
        );
        assert_eq!(tl.frame_at(0.0, None).comment, None);
        assert_eq!(tl.frame_at(1.5, None).comment, Some("un"));
        assert_eq!(tl.frame_at(1.5, Some("EN")).comment, Some("one"));
        assert_eq!(tl.frame_at(2.0, Some("NL")).comment, Some("twee"));
        // langue inconnue → langue par défaut
        assert_eq!(tl.frame_at(2.0, Some("DE")).comment, Some("deux"));
    }

    #[test]
    fn audio_demarre() {
        let tl = timeline(
            "[figures]
            [timeline]
            2.000 => { start : [audio musique.mp3 volume=0.3] }
            8.000 => { end : [audio musique.mp3] }",
        );
        assert!(tl.frame_at(1.0, None).audio.is_empty());
        let audio = tl.frame_at(5.0, None).audio;
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].file, "musique.mp3");
        assert_eq!(audio[0].volume, 0.3);
        assert_eq!(audio[0].position, 3.0);
        assert!(tl.frame_at(8.0, None).audio.is_empty()); // arrêtée par end
    }

    #[test]
    fn animations_planifiees() {
        let tl = timeline(
            "[figures]
            deffigure a =
                type : line
            [timeline]
            1.000 => {
                start : [a fadein(1s)]
                anim  : [a [move(to=(50%, 50%), 2s), color(to=red, 500ms)]]
            }
            5.000 => { end : [a fadeout(2s)] }",
        );
        let f = tl.frame_at(10.0, None);
        let st = f.states.iter().find(|s| s.fig.name == "a").unwrap();
        let starts: Vec<(f64, f64)> = st.anims.iter().map(|a| (a.start, a.duration)).collect();
        // fadein à 1 s ; move à 1 s ; color enchaîné après move → 3 s ; fadeout à 5 s
        assert_eq!(starts, [(1.0, 1.0), (1.0, 2.0), (3.0, 0.5), (5.0, 2.0)]);
        assert_eq!(tl.duration(), 7.0); // le fadeout finit après le dernier bloc
        assert!(tl.frame_at(6.0, None).visible.len() == 1); // sortie en cours
        assert!(tl.frame_at(7.0, None).visible.is_empty());
    }

    #[test]
    fn exemple_officiel() {
        let tl = timeline(include_str!("../../exemple.dct"));
        assert_eq!(tl.duration(), 56.0);
        assert_eq!(visibles(&tl, 0.5), ["axes_xy", "courbe_sin", "etoile"]);
        assert_eq!(
            visibles(&tl, 10.0),
            ["axes_xy", "courbe_sin", "rect_mobile", "etoile", "ligne_direction", "arc_theta", "label_theta"]
        );
        // À 30 s : axes et courbe en train de disparaître (fadeout 0.5 s), encore affichés
        assert_eq!(
            visibles(&tl, 30.2),
            ["axes_xy", "courbe_sin", "rect_mobile", "etoile", "ligne_direction", "arc_theta", "label_theta"]
        );
        assert_eq!(tl.status_at("axes_xy", 30.2), Status::Hidden);
        assert_eq!(
            visibles(&tl, 30.6),
            ["rect_mobile", "etoile", "ligne_direction", "arc_theta", "label_theta"]
        );
        assert_eq!(visibles(&tl, 55.1).len(), 5); // fadeout 0.3 s en cours
        assert!(visibles(&tl, 55.4).is_empty());
        assert_eq!(tl.status_at("rect_mobile", 56.0), Status::Destroyed);
        assert_eq!(
            tl.frame_at(31.0, Some("EN")).comment,
            Some("The curve disappears but the rectangle continues")
        );
    }
}
