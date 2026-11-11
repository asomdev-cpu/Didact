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
use crate::ast::PropValue;
use crate::resolver::{ResolvedDocument, ResolvedFigure};
use std::collections::HashMap;

/// Figures d'un groupe (`items`), groupes imbriqués compris, sans le groupe lui-même
pub fn group_members(group: &str, figures: &[ResolvedFigure]) -> Vec<String> {
    fn walk(name: &str, figures: &[ResolvedFigure], out: &mut Vec<String>, depth: usize) {
        let Some(f) = figures.iter().find(|f| f.name == name) else { return };
        if f.ftype != "group" || depth > 32 {
            return;
        }
        if let Some(PropValue::List(items)) = f.properties.get("items") {
            for item in items {
                if !out.contains(item) {
                    out.push(item.clone());
                    walk(item, figures, out, depth + 1);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(group, figures, &mut out, 0);
    out.retain(|m| m != group);
    out
}

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

/// Une piste audio sur toute sa durée de lecture
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSegment {
    pub file: String,
    pub volume: f64,
    /// Instant de démarrage dans la vidéo (secondes)
    pub start: f64,
    /// Instant d'arrêt (None = jusqu'à la fin de la vidéo)
    pub end: Option<f64>,
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
    /// Pour un groupe : ses figures (groupes imbriqués compris), sinon vide
    pub members: &'a [String],
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
    /// Valeur de chaque paramètre (`defparam`) à l'instant t
    pub params: HashMap<String, f64>,
}

pub struct Timeline {
    /// Toutes les figures, dans l'ordre de dessin
    figures: Vec<ResolvedFigure>,
    /// Indices dans `figures`, dans l'ordre de définition
    definition_order: Vec<usize>,
    /// Animations de chaque figure, triées par début
    anims: HashMap<String, Vec<Anim>>,
    /// Membres de chaque groupe (groupes imbriqués compris)
    groups: HashMap<String, Vec<String>>,
    /// Timestamps des blocs, triés
    times: Vec<f64>,
    /// snapshots[i] = état juste après le bloc times[i]
    snapshots: Vec<Snapshot>,
    /// Langues dans l'ordre de [config]
    languages: Vec<String>,
    default_lang: String,
    /// Chaque `comment` de la timeline, dans l'ordre chronologique
    comments: Vec<CommentAt>,
    /// Paramètres et valeurs de départ
    params: Vec<(String, f64)>,
}

/// Un `comment` de la timeline : instant, ligne, traductions (dans l'ordre de `languages`)
#[derive(Debug, Clone, PartialEq)]
pub struct CommentAt {
    pub t: f64,
    pub line: usize,
    pub texts: Vec<String>,
}

/// `a` remplacée par `b` à la fin d'un transform(to=b)
#[derive(Debug, Clone)]
struct Swap {
    time: f64,
    from: String,
    to: String,
}

/// Planifie un appel d'animation qui commence à `start`. Renvoie sa durée.
/// Les erreurs ont déjà été signalées par le validateur : un appel invalide est ignoré.
fn schedule(
    anims: &mut HashMap<String, Vec<Anim>>,
    swaps: &mut Vec<Swap>,
    figure: &str,
    call: &AnimCall,
    start: f64,
) -> f64 {
    match animation::parse_call(call) {
        Ok((kind, duration, ease)) => {
            if let AnimKind::Transform { to } = &kind {
                swaps.push(Swap { time: start + duration, from: figure.to_string(), to: to.clone() });
            }
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
        let mut swaps: Vec<Swap> = Vec::new();
        let mut comments: Vec<CommentAt> = Vec::new();
        let groups: HashMap<String, Vec<String>> = doc
            .figures
            .iter()
            .filter(|f| f.ftype == "group")
            .map(|f| (f.name.clone(), group_members(&f.name, &doc.figures)))
            .collect();
        // Une figure et, si c'est un groupe, tous ses membres
        let with_members = |name: &str| -> Vec<String> {
            let mut v = vec![name.to_string()];
            if let Some(m) = groups.get(name) {
                v.extend(m.iter().cloned());
            }
            v
        };

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
            // Fins de transform d'avant ce bloc : instantanés intermédiaires
            flush_swaps(&mut swaps, &mut current, Some(t), &mut times, &mut snapshots);
            // Ordre fixe : destroy → end → start → anim → comment (SPEC §6.3)
            // On parcourt le bloc une fois par commande.
            for located in &ev.actions {
                if let TimelineAction::Destroy(names) = &located.action {
                    for name in names.iter().flat_map(|n| with_members(n)) {
                        current.status.insert(name.clone(), Status::Destroyed);
                        current.exits.remove(&name);
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
                        let d = match &item.anim {
                            Some(call) => schedule(&mut anims, &mut swaps, &item.name, call, t),
                            None => 0.0,
                        };
                        for name in with_members(&item.name) {
                            // une figure du groupe déjà cachée le reste (pas de nouvelle sortie)
                            let was_visible = current.status.get(&name) == Some(&Status::Visible);
                            if name != item.name && !was_visible {
                                continue;
                            }
                            current.status.insert(name.clone(), Status::Hidden);
                            current.exits.remove(&name);
                            if item.anim.is_some() {
                                current.exits.insert(name, t + d);
                            }
                        }
                    }
                }
            }
            for located in &ev.actions {
                if let TimelineAction::Start(items) = &located.action {
                    for item in items {
                        match item {
                            StartItem::Figure { name, anim } => {
                                for n in with_members(name) {
                                    current.status.insert(n.clone(), Status::Visible);
                                    current.exits.remove(&n);
                                    current.last_start.insert(n, t);
                                }
                                if let Some(call) = anim {
                                    schedule(&mut anims, &mut swaps, name, call, t);
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
                            start += schedule(&mut anims, &mut swaps, &item.figure, call, start);
                        }
                    }
                }
            }
            for located in &ev.actions {
                if let TimelineAction::Comment(texts) = &located.action {
                    current.comment = Some(texts.clone());
                    comments.push(CommentAt { t, line: located.line, texts: texts.clone() });
                }
            }

            times.push(t);
            snapshots.push(current.clone());
        }
        flush_swaps(&mut swaps, &mut current, None, &mut times, &mut snapshots);

        for list in anims.values_mut() {
            list.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        }

        let mut definition_order: Vec<usize> = (0..doc.figures.len()).collect();
        definition_order.sort_by_key(|&i| doc.figures[i].index);

        Timeline {
            figures: doc.figures.clone(),
            definition_order,
            anims,
            groups,
            times,
            snapshots,
            languages: doc.config.languages.clone(),
            default_lang: doc.config.default_lang.clone(),
            comments,
            params: doc.params.clone(),
        }
    }

    /// Valeur des paramètres à l'instant t : chaque `to` commencé rapproche la valeur de sa cible.
    /// Comme le reste, elle ne dépend que de t (on peut reculer).
    fn params_at(&self, t: f64) -> HashMap<String, f64> {
        self.params
            .iter()
            .map(|(name, start)| {
                let mut v = *start;
                for a in self.anims.get(name).map(|l| l.as_slice()).unwrap_or(&[]) {
                    if let (AnimKind::To(target), true) = (&a.kind, a.start <= t) {
                        v += (target - v) * a.progress(t);
                    }
                }
                (name.clone(), v)
            })
            .collect()
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

    /// Fins de toutes les animations, triées, sans doublons
    pub fn anim_ends(&self) -> Vec<f64> {
        let mut v: Vec<f64> = self.anims.values().flatten().map(|a| a.end()).collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    }

    /// Timestamps de tous les blocs, triés
    pub fn times(&self) -> &[f64] {
        &self.times
    }

    /// Pistes audio sur toute la durée : quand chacune démarre et s'arrête (fenêtre, web, MP4)
    pub fn audio_segments(&self) -> Vec<AudioSegment> {
        let mut done: Vec<AudioSegment> = Vec::new();
        let mut open: Vec<AudioSegment> = Vec::new();
        for &t in self.times() {
            let frame = self.frame_at(t, None);
            let now: Vec<(String, f64, f64)> = frame
                .audio
                .iter()
                .map(|a| (a.file.clone(), a.volume, round_ms(t - a.position)))
                .collect();
            // Pistes arrêtées (ou relancées) à t
            let mut still = Vec::new();
            for mut seg in open.drain(..) {
                if now.iter().any(|(f, v, s)| *f == seg.file && *v == seg.volume && *s == seg.start) {
                    still.push(seg);
                } else {
                    seg.end = Some(t);
                    done.push(seg);
                }
            }
            open = still;
            // Nouvelles pistes
            for (file, volume, start) in now {
                if !open.iter().any(|s| s.file == file && s.start == start) {
                    open.push(AudioSegment { file, volume, start, end: None });
                }
            }
        }
        done.extend(open);
        done.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
        done
    }

    /// Tous les `comment`, dans l'ordre chronologique
    pub fn comments(&self) -> &[CommentAt] {
        &self.comments
    }

    /// Langues déclarées dans [config], dans l'ordre
    pub fn languages(&self) -> &[String] {
        &self.languages
    }

    /// Position d'une langue dans `languages` (langue inconnue ou None → default_lang)
    pub fn lang_index(&self, lang: Option<&str>) -> usize {
        let lang = lang.unwrap_or(&self.default_lang);
        self.languages
            .iter()
            .position(|l| l == lang)
            .or_else(|| self.languages.iter().position(|l| *l == self.default_lang))
            .unwrap_or(0)
    }

    /// Membres d'un groupe (groupes imbriqués compris) ; vide si ce n'est pas un groupe
    pub fn members(&self, group: &str) -> &[String] {
        self.groups.get(group).map(|v| v.as_slice()).unwrap_or(&[])
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
                return Frame {
                    t,
                    visible: Vec::new(),
                    states: Vec::new(),
                    comment: None,
                    audio: Vec::new(),
                    params: self.params_at(t),
                };
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
                members: self.members(&f.name),
            })
            .collect();

        let lang_index = self.lang_index(lang);
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

        Frame { t, visible, states, comment, audio, params: self.params_at(t) }
    }
}

fn round_ms(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Applique les fins de transform (a cachée, b affichée) d'avant `until`
/// (toutes si None). Une fin qui tombe pile sur un bloc est appliquée juste avant lui ;
/// les autres créent un instantané à leur instant.
fn flush_swaps(
    swaps: &mut Vec<Swap>,
    current: &mut Snapshot,
    until: Option<f64>,
    times: &mut Vec<f64>,
    snapshots: &mut Vec<Snapshot>,
) {
    swaps.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    let last = times.last().copied().unwrap_or(f64::NEG_INFINITY);
    while let Some(sw) = swaps.first().cloned() {
        if until.is_some_and(|u| sw.time > u) {
            break;
        }
        swaps.remove(0);
        current.status.insert(sw.from.clone(), Status::Hidden);
        current.exits.remove(&sw.from);
        current.status.insert(sw.to.clone(), Status::Visible);
        current.exits.remove(&sw.to);
        current.last_start.insert(sw.to.clone(), sw.time);
        let is_block_time = until.is_some_and(|u| (sw.time - u).abs() < 1e-9);
        let next_same = swaps.first().is_some_and(|n| (n.time - sw.time).abs() < 1e-9);
        if !is_block_time && !next_same {
            if sw.time <= last {
                // fin d'un transform démarré dans le dernier bloc, au même instant (durée 0) :
                // on corrige l'instantané de ce bloc
                *snapshots.last_mut().unwrap() = current.clone();
            } else {
                times.push(sw.time);
                snapshots.push(current.clone());
            }
        }
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

    #[test]
    fn groupe_et_transform() {
        let tl = timeline(
            "[figures]
            deffigure a =
                type : circle
            deffigure b =
                type : rect
            deffigure c =
                type : star
            defgroup g =
                items : [a, b]
                pos : (10%, -5%)
            [timeline]
            0.000 => { start : [g fadein(1s)] }
            2.000 => { anim : [a transform(to=c, 1.5s)] }
            5.000 => { end : [g fadeout(1s)] }
            6.000 => { destroy : [g] }",
        );
        assert_eq!(visibles(&tl, 0.5), ["a", "b", "g"]);
        // pendant le transform : a encore affichée, c pas encore
        assert_eq!(visibles(&tl, 3.0), ["a", "b", "g"]);
        // à la fin (3.5 s) : a remplacée par c (instantané intermédiaire)
        assert_eq!(visibles(&tl, 3.5), ["b", "c", "g"]);
        assert!(tl.times().contains(&3.5));
        assert_eq!(visibles(&tl, 5.5), ["b", "c", "g"]);
        assert_eq!(visibles(&tl, 6.5), ["c"]);
        // a, cachée par le transform, ne réapparaît pas avec le end du groupe
        assert_eq!(tl.status_at("a", 5.0), Status::Hidden);
        assert_eq!(tl.status_at("a", 6.0), Status::Destroyed);
        assert_eq!(tl.status_at("b", 6.0), Status::Destroyed);
        assert_eq!(tl.members("g"), ["a", "b"]);
    }

    #[test]
    fn parametres_dans_le_temps() {
        let tl = timeline(
            "[figures]
            defparam k = 1
            [timeline]
            1.000 => { anim : [k to(3, 2s, ease=linear)] }
            5.000 => { anim : [k [to(0, 1s, ease=linear), to(-2, 1s, ease=linear)]] }",
        );
        let k = |t: f64| tl.frame_at(t, None).params["k"];
        assert_eq!(k(0.0), 1.0); // avant le premier bloc : valeur de départ
        assert_eq!(k(1.0), 1.0);
        assert_eq!(k(2.0), 2.0); // à mi-chemin
        assert_eq!(k(3.0), 3.0);
        assert_eq!(k(4.0), 3.0); // garde sa valeur
        assert_eq!(k(5.5), 1.5);
        assert_eq!(k(6.5), -1.0); // enchaîné
        assert_eq!(k(9.0), -2.0);
        assert_eq!(k(2.0), 2.0); // on peut reculer
        assert_eq!(tl.duration(), 7.0); // la fin d'un `to` compte dans la durée
    }

    #[test]
    fn segments_audio() {
        let tl = timeline(
            "[config]\nwindow : [16:9]\n[figures]\n[timeline]
            2.000 => { start : [audio musique.mp3 volume=0.3] }
            8.000 => { end : [audio musique.mp3] }
            9.000 => { start : [audio bip.wav] }",
        );
        let segs = tl.audio_segments();
        assert_eq!(
            segs,
            vec![
                AudioSegment { file: "musique.mp3".into(), volume: 0.3, start: 2.0, end: Some(8.0) },
                AudioSegment { file: "bip.wav".into(), volume: 1.0, start: 9.0, end: None },
            ]
        );
    }
}
