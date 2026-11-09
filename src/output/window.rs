//! Fenêtre desktop : joue l'animation, avec des contrôles en bas.
//!
//! - La scène est dessinée par tiny-skia dans une image, à la taille réelle
//!   de la zone d'affichage (en pixels), puis affichée par egui.
//! - Les contrôles (lecture/pause, barre de lecture, langue) et les sous-titres
//!   sont dessinés par egui par-dessus.
//!
//! Le son (pistes `audio`) est joué par `audio::AudioPlayer` ; pendant la lecture,
//! son horloge donne le temps, pour que l'image suive le son.
//!
//! Le fichier .dct est surveillé : à chaque sauvegarde, l'animation est rechargée au même
//! instant. S'il contient une erreur, l'ancienne version reste affichée, avec l'erreur par-dessus.
//!
//! Raccourcis : Espace = lecture/pause, ← / → = reculer / avancer d'1 s, Début = revenir à 0.

use super::audio::AudioPlayer;
use super::reload::Watcher;
use crate::ast::{Config, WindowConfig};
use crate::renderer::{self, skia::SkiaBackend, Scene};
use super::speech::{self, VoicePlan};
use crate::runtime::timeline::AudioSegment;
use crate::runtime::Timeline;
use crate::voice::Holds;
use eframe::egui;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct DidactApp {
    timeline: Timeline,
    config: Config,
    /// Temps courant (secondes)
    t: f64,
    playing: bool,
    lang: String,
    /// Langue de la dernière image (pour recaler le temps quand on en change)
    shown_lang: String,
    /// Pauses de l'animation de chaque langue (voix plus longues que l'écart entre commentaires).
    /// `t` est le temps réel ; le temps de la timeline s'en déduit.
    holds: Vec<Holds>,
    backend: Option<SkiaBackend>,
    texture: Option<egui::TextureHandle>,
    /// Problèmes déjà signalés (pour ne pas répéter le même message à chaque frame)
    warned: HashSet<String>,
    /// Son (None s'il n'y a pas de piste audio, ou pas de sortie audio)
    audio: Option<AudioPlayer>,
    /// Pistes du son actuel : le son n'est reconstruit que si elles changent
    tracks: Tracks,
    /// Le fichier .dct joué
    source: PathBuf,
    /// Surveille les sauvegardes du fichier
    watcher: Watcher,
    /// Erreurs de la dernière sauvegarde (l'ancienne version reste affichée)
    reload_errors: Vec<String>,
    /// Caméra 3D tournée à la souris : (élévation, rotation) en degrés, zoom.
    /// Aperçu seulement : les exports gardent la caméra du fichier.
    orbit: (f32, f32, f32),
    /// Part de la résolution utilisée (1 = pleine). Baisse pendant la lecture si les images
    /// tardent, et revient à 1 dès que rien ne bouge.
    quality: f32,
    /// Ce qui a été dessiné la dernière fois : rien à refaire si rien n'a changé
    last_drawn: Option<(f64, (f32, f32, f32), u32, u32, String)>,
}

/// Temps visé pour une image (secondes) : au-delà, la résolution baisse pendant le mouvement
const FRAME_BUDGET: f64 = 1.0 / 30.0;

/// Toutes les pistes de la fenêtre, avec la langue à laquelle chacune appartient
type Tracks = Vec<(AudioSegment, Option<usize>)>;

/// Chaque langue a ses pauses, donc ses propres instants (en temps réel) :
/// les pistes sont préparées pour chaque langue.
fn tracks_of(timeline: &Timeline, voices: &[VoicePlan]) -> Tracks {
    let music = timeline.audio_segments();
    let has_voice = voices.iter().any(|v| !v.segments.is_empty());
    let mut segments = Vec::new();
    for (index, plan) in voices.iter().enumerate() {
        if !music.is_empty() || has_voice {
            let tracks = speech::real_time(&music, &plan.holds)
                .into_iter()
                .chain(speech::real_time(&plan.segments, &plan.holds));
            segments.extend(tracks.map(|s| (s, Some(index))));
        }
    }
    segments
}

/// Décode les pistes et ouvre le son ; sans son, la fenêtre marche quand même
fn open_audio(tracks: &Tracks, base_dir: &Path, lang: usize) -> Option<AudioPlayer> {
    if tracks.is_empty() {
        return None;
    }
    match AudioPlayer::new(tracks, base_dir, lang) {
        Ok((player, warnings)) => {
            for w in warnings {
                eprintln!("Attention : {}", w);
            }
            Some(player)
        }
        Err(e) => {
            eprintln!("Attention : pas de son ({})", e);
            None
        }
    }
}

/// Ouvre la fenêtre et bloque jusqu'à sa fermeture.
/// `source` : le fichier .dct (surveillé ; les chemins audio sont relatifs à son dossier).
/// `voices` : voix de chaque langue, dans l'ordre de `languages`.
pub fn run(
    timeline: Timeline,
    config: Config,
    lang: Option<String>,
    title: &str,
    source: &Path,
    voices: Vec<VoicePlan>,
) -> Result<(), String> {
    let lang = lang.unwrap_or_else(|| config.default_lang.clone());
    let (w, h) = initial_window_size(&config);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Didact — {}", title))
            .with_inner_size([w, h]),
        ..Default::default()
    };

    // Son : décodé avant d'ouvrir la fenêtre
    let tracks = tracks_of(&timeline, &voices);
    let base_dir = source.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let audio = open_audio(&tracks, &base_dir, timeline.lang_index(Some(&lang)));
    let holds: Vec<Holds> = voices.into_iter().map(|v| v.holds).collect();

    let app = DidactApp {
        timeline,
        config,
        t: 0.0,
        playing: true,
        shown_lang: lang.clone(),
        lang,
        holds,
        backend: None,
        texture: None,
        warned: HashSet::new(),
        audio,
        tracks,
        source: source.to_path_buf(),
        watcher: Watcher::new(source),
        reload_errors: Vec::new(),
        orbit: (0.0, 0.0, 1.0),
        quality: 1.0,
        last_drawn: None,
    };

    eframe::run_native("Didact", options, Box::new(|_cc| Ok(Box::new(app)))).map_err(|e| e.to_string())
}

/// Taille de départ de la fenêtre (points écran) : 1280 px de large au ratio de la scène,
/// + la hauteur de la barre de contrôles
fn initial_window_size(config: &Config) -> (f32, f32) {
    let ratio = scene_ratio(config).unwrap_or(16.0 / 9.0);
    let w = 1280.0;
    (w, w / ratio + 48.0)
}

/// Ratio largeur/hauteur imposé par `window`, None si `auto` (la scène prend toute la place)
fn scene_ratio(config: &Config) -> Option<f32> {
    match config.window {
        WindowConfig::Size(w, h) if w > 0 && h > 0 => Some(w as f32 / h as f32),
        WindowConfig::Ratio(a, b) if a > 0 && b > 0 => Some(a as f32 / b as f32),
        _ => None,
    }
}

impl eframe::App for DidactApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Fichier sauvegardé : on recharge, même en pause
        self.check_reload();
        ctx.request_repaint_after(Duration::from_millis(200));

        // Changement de langue : on reste au même endroit de la timeline
        if self.lang != self.shown_lang {
            let old = self.timeline.lang_index(Some(&self.shown_lang));
            let t_timeline = self.holds_of(old).to_timeline(self.t);
            self.t = self.holds_of(self.lang_idx()).to_real(t_timeline);
            self.shown_lang = self.lang.clone();
        }
        let duration = self.timeline.duration() + self.holds_of(self.lang_idx()).extra();

        // ── Avancement du temps (horloge du son s'il y en a un) ──
        if self.playing {
            match &self.audio {
                Some(a) => self.t = a.now(),
                None => self.t += ctx.input(|i| i.stable_dt) as f64,
            }
            if self.t >= duration {
                self.t = duration;
                self.playing = false;
            }
            ctx.request_repaint();
        }

        // ── Clavier ──
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) {
                self.toggle_play(duration);
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                self.t = (self.t + 1.0).min(duration);
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.t = (self.t - 1.0).max(0.0);
            }
            if i.key_pressed(egui::Key::Home) {
                self.t = 0.0;
            }
        });

        // ── Barre de contrôles ──
        egui::Panel::bottom("controles").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("⏮").on_hover_text("Revenir au début (Début)").clicked() {
                    self.t = 0.0;
                }
                let label = if self.playing { "⏸" } else { "▶" };
                if ui.button(label).on_hover_text("Lecture / pause (Espace)").clicked() {
                    self.toggle_play(duration);
                }

                ui.label(format!("{:6.2} / {:.2} s", self.t, duration));

                // Langue (seulement s'il y en a plusieurs)
                if self.config.languages.len() > 1 {
                    egui::ComboBox::from_id_salt("langue")
                        .selected_text(self.lang.clone())
                        .width(50.0)
                        .show_ui(ui, |ui| {
                            for l in &self.config.languages {
                                ui.selectable_value(&mut self.lang, l.clone(), l);
                            }
                        });
                }

                // Barre de lecture : prend toute la place restante
                ui.spacing_mut().slider_width = ui.available_width() - 8.0;
                ui.add(egui::Slider::new(&mut self.t, 0.0..=duration.max(0.001)).show_value(false));
            });
        });

        // ── Scène ──
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_gray(30)))
            .show(ui, |ui| self.draw_scene(ui));

        // ── Son : suit les sauts, la pause et la reprise ──
        if let Some(a) = &self.audio {
            a.set_lang(self.lang_idx());
            a.sync(self.t, self.playing);
        }
    }
}

impl DidactApp {
    fn lang_idx(&self) -> usize {
        self.timeline.lang_index(Some(&self.lang))
    }

    /// Recharge le fichier s'il a été sauvegardé. On reste au même instant de la timeline ;
    /// en cas d'erreur, l'ancienne version continue et l'erreur est affichée.
    fn check_reload(&mut self) {
        let Some(result) = self.watcher.poll() else { return };
        let (doc, timeline) = match result {
            Ok(loaded) => loaded,
            Err(errors) => {
                for e in &errors {
                    eprintln!("✗ {}", e);
                }
                self.reload_errors = errors;
                return;
            }
        };
        self.reload_errors.clear();
        self.warned.clear();
        self.last_drawn = None; // le fichier a changé : à redessiner

        let t_timeline = self.holds_of(self.lang_idx()).to_timeline(self.t);
        self.timeline = timeline;
        self.config = doc.config;
        if !self.config.languages.contains(&self.lang) {
            self.lang = self.config.default_lang.clone();
            self.shown_lang = self.lang.clone();
        }

        let voices: Vec<VoicePlan> =
            self.timeline.languages().iter().map(|l| speech::plan(&self.timeline, &self.source, l)).collect();
        let tracks = tracks_of(&self.timeline, &voices);
        self.holds = voices.into_iter().map(|v| v.holds).collect();
        let duration = self.timeline.duration() + self.holds_of(self.lang_idx()).extra();
        self.t = self.holds_of(self.lang_idx()).to_real(t_timeline).min(duration);

        // Le son n'est redécodé que si ses pistes ont changé
        if tracks != self.tracks {
            let base_dir = self.source.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            self.audio = open_audio(&tracks, &base_dir, self.lang_idx());
            self.tracks = tracks;
        }
        if let Some(a) = &self.audio {
            a.sync(self.t, self.playing);
        }
        println!("✓ Rechargé : {}", self.source.display());
    }

    fn holds_of(&self, lang: usize) -> Holds {
        self.holds.get(lang).cloned().unwrap_or_default()
    }

    fn toggle_play(&mut self, duration: f64) {
        if !self.playing && self.t >= duration {
            self.t = 0.0; // relancer depuis le début une fois la fin atteinte
        }
        self.playing = !self.playing;
    }

    fn draw_scene(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();

        // Zone de la scène : tout l'espace, ou le plus grand rectangle au bon ratio, centré
        let size = match scene_ratio(&self.config) {
            None => avail.size(),
            Some(r) => {
                if avail.width() / avail.height() > r {
                    egui::vec2(avail.height() * r, avail.height())
                } else {
                    egui::vec2(avail.width(), avail.width() / r)
                }
            }
        };
        let rect = egui::Rect::from_center_size(avail.center(), size);

        // Scène 3D : glisser = tourner la caméra, molette = zoom, double-clic = caméra du fichier
        let response = ui.interact(rect, ui.unique_id().with("scene3d"), egui::Sense::click_and_drag());
        let dragging = response.dragged();
        if dragging {
            let d = response.drag_delta();
            self.orbit.1 -= d.x * 0.4;
            self.orbit.0 = (self.orbit.0 + d.y * 0.4).clamp(-180.0, 180.0);
        }
        if response.hovered() {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.orbit.2 = (self.orbit.2 * (1.0 + scroll * 0.002)).clamp(0.1, 10.0);
            }
        }
        if response.double_clicked() {
            self.orbit = (0.0, 0.0, 1.0);
        }

        // Résolution : pleine à l'arrêt ; réduite pendant le mouvement si besoin
        let moving = self.playing || dragging;
        if !moving {
            self.quality = 1.0;
        }

        // Taille en vrais pixels (écrans haute densité), à la qualité du moment
        let ppp = ui.ctx().pixels_per_point();
        let pw = (size.x * ppp * self.quality).round().max(1.0) as u32;
        let ph = (size.y * ppp * self.quality).round().max(1.0) as u32;

        // (Re)crée l'image si la taille a changé
        if self.backend.as_ref().map(|b| (b.width(), b.height())) != Some((pw, ph)) {
            self.backend = SkiaBackend::new(pw, ph);
        }
        let t = self.holds_of(self.lang_idx()).to_timeline(self.t);
        let Some(backend) = self.backend.as_mut() else { return };

        // Dessin (seulement si quelque chose a changé : instant, caméra, taille, langue)
        let frame = self.timeline.frame_at(t, Some(&self.lang));
        let key = (t, self.orbit, pw, ph, self.lang.clone());
        if self.last_drawn.as_ref() != Some(&key) || self.texture.is_none() {
            let started = std::time::Instant::now();
            let mut scene = Scene::new(&self.config, pw, ph);
            scene.orbit = self.orbit;
            let skipped = renderer::render_frame_parallel(&frame, &scene, backend);
            for (name, reason) in skipped {
                if self.warned.insert(format!("{}|{}", name, reason)) {
                    eprintln!("Attention : '{}' non dessinée : {}", name, reason);
                }
            }

            // Envoi de l'image à egui
            let image = egui::ColorImage::from_rgba_premultiplied([pw as usize, ph as usize], backend.pixmap.data());
            match &mut self.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => self.texture = Some(ui.ctx().load_texture("scene", image, egui::TextureOptions::LINEAR)),
            }
            self.last_drawn = Some(key);

            // Ajuste la résolution pour rester fluide pendant le mouvement
            let spent = started.elapsed().as_secs_f64();
            if moving {
                if spent > FRAME_BUDGET * 1.3 {
                    self.quality = (self.quality * (FRAME_BUDGET / spent).sqrt() as f32).max(if dragging { 0.5 } else { 0.7 });
                } else if spent < FRAME_BUDGET * 0.5 {
                    self.quality = (self.quality * 1.1).min(1.0);
                }
            }
        }
        if let Some(tex) = &self.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(tex.id(), rect, uv, egui::Color32::WHITE);
        }

        // Sous-titre
        if let Some(text) = frame.comment {
            let font = egui::FontId::proportional((size.y * 0.04).clamp(12.0, 32.0));
            let galley = ui.painter().layout(
                text.to_string(),
                font,
                egui::Color32::WHITE,
                size.x * 0.9,
            );
            let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.bottom() - galley.size().y - size.y * 0.05);
            let bg = egui::Rect::from_min_size(pos, galley.size()).expand(6.0);
            ui.painter().rect_filled(bg, 4.0, egui::Color32::from_black_alpha(170));
            ui.painter().galley(pos, galley, egui::Color32::WHITE);
        }

        // Erreurs de la dernière sauvegarde, en haut de la scène
        if !self.reload_errors.is_empty() {
            let text = format!("Fichier non rechargé :\n{}", self.reload_errors.join("\n"));
            let font = egui::FontId::monospace((size.y * 0.025).clamp(11.0, 18.0));
            let galley = ui.painter().layout(text, font, egui::Color32::WHITE, size.x * 0.94);
            let pos = rect.left_top() + egui::vec2(size.x * 0.03, size.y * 0.03);
            let bg = egui::Rect::from_min_size(pos, galley.size()).expand(8.0);
            ui.painter().rect_filled(bg, 4.0, egui::Color32::from_rgb(153, 27, 27));
            ui.painter().galley(pos, galley, egui::Color32::WHITE);
        }
    }
}
