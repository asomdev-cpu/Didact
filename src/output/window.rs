//! Fenêtre desktop : joue l'animation, avec des contrôles en bas.
//!
//! - La scène est dessinée par tiny-skia dans une image, à la taille réelle
//!   de la zone d'affichage (en pixels), puis affichée par egui.
//! - Les contrôles (lecture/pause, barre de lecture, langue) et les sous-titres
//!   sont dessinés par egui par-dessus.
//!
//! Raccourcis : Espace = lecture/pause, ← / → = reculer / avancer d'1 s, Début = revenir à 0.

use crate::ast::{Config, WindowConfig};
use crate::renderer::{self, skia::SkiaBackend, Scene};
use crate::runtime::Timeline;
use eframe::egui;
use std::collections::HashSet;

struct DidactApp {
    timeline: Timeline,
    config: Config,
    /// Temps courant (secondes)
    t: f64,
    playing: bool,
    lang: String,
    backend: Option<SkiaBackend>,
    texture: Option<egui::TextureHandle>,
    /// Problèmes déjà signalés (pour ne pas répéter le même message à chaque frame)
    warned: HashSet<String>,
}

/// Ouvre la fenêtre et bloque jusqu'à sa fermeture.
pub fn run(timeline: Timeline, config: Config, lang: Option<String>, title: &str) -> Result<(), String> {
    let lang = lang.unwrap_or_else(|| config.default_lang.clone());
    let (w, h) = initial_window_size(&config);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Didact — {}", title))
            .with_inner_size([w, h]),
        ..Default::default()
    };

    let app = DidactApp {
        timeline,
        config,
        t: 0.0,
        playing: true,
        lang,
        backend: None,
        texture: None,
        warned: HashSet::new(),
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
        let duration = self.timeline.duration();

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

        // ── Avancement du temps ──
        if self.playing {
            self.t += ctx.input(|i| i.stable_dt) as f64;
            if self.t >= duration {
                self.t = duration;
                self.playing = false;
            }
            ctx.request_repaint();
        }

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
    }
}

impl DidactApp {
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

        // Taille en vrais pixels (écrans haute densité)
        let ppp = ui.ctx().pixels_per_point();
        let pw = (size.x * ppp).round().max(1.0) as u32;
        let ph = (size.y * ppp).round().max(1.0) as u32;

        // (Re)crée l'image si la taille a changé
        if self.backend.as_ref().map(|b| (b.width(), b.height())) != Some((pw, ph)) {
            self.backend = SkiaBackend::new(pw, ph);
        }
        let Some(backend) = self.backend.as_mut() else { return };

        // Dessin
        let frame = self.timeline.frame_at(self.t, Some(&self.lang));
        let scene = Scene::new(&self.config, pw, ph);
        let skipped = renderer::render_frame(&frame, &scene, backend);
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
    }
}
