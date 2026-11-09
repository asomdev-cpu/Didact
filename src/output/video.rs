//! Export MP4 : chaque image est dessinée ici (tiny-skia) puis envoyée brute à ffmpeg,
//! qui l'encode en H.264. Les sous-titres (commentaires) sont incrustés dans l'image,
//! et les pistes audio de la timeline sont mixées dans le fichier.
//!
//! ffmpeg doit être installé (Windows : `winget install Gyan.FFmpeg`).

use crate::ast::Config;
use crate::renderer::geometry::{PathCmd, Rgba, Shape};
use crate::renderer::skia::SkiaBackend;
use crate::renderer::text::{self, Align};
use crate::renderer::{self, RenderBackend, Scene};
use crate::runtime::timeline::AudioSegment;
use crate::runtime::Timeline;

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Réglages de l'export
pub struct VideoOptions {
    pub output: PathBuf,
    pub fps: u32,
    /// Hauteur de la vidéo en pixels (la largeur suit le format de `window`) ; None = taille par défaut
    pub height: Option<u32>,
    pub lang: Option<String>,
    pub subtitles: bool,
    /// Commande ffmpeg (par défaut "ffmpeg", cherché dans le PATH)
    pub ffmpeg: String,
    /// Dossier du fichier .dct : les chemins audio sont relatifs à lui
    pub base_dir: PathBuf,
    /// Voix de la langue choisie (fichiers générés par --voice) et pauses de l'animation
    pub voice: super::speech::VoicePlan,
}

/// Résultat de l'export
pub struct VideoReport {
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub audio_tracks: usize,
    /// Avertissements de rendu (figure non dessinée), sans doublons
    pub warnings: Vec<String>,
}

/// Taille de la vidéo : celle du rendu PNG, ou mise à l'échelle sur `height`.
/// H.264 (yuv420p) exige des dimensions paires.
pub fn video_size(config: &Config, height: Option<u32>) -> (u32, u32) {
    let (w, h) = Scene::default_size(config);
    let (w, h) = match height {
        Some(target) if target > 0 => {
            let w2 = (w as f64 * target as f64 / h as f64).round() as u32;
            (w2, target)
        }
        _ => (w, h),
    };
    ((w / 2 * 2).max(2), (h / 2 * 2).max(2))
}

/// Exporte la timeline en MP4. `progress(image, total)` est appelé après chaque image.
pub fn export(
    timeline: &Timeline,
    config: &Config,
    opts: &VideoOptions,
    mut progress: impl FnMut(u32, u32),
) -> Result<VideoReport, String> {
    let (w, h) = video_size(config, opts.height);
    // Temps réel de la vidéo = timeline + pauses laissées aux phrases trop longues
    let holds = &opts.voice.holds;
    let duration = (timeline.duration() + holds.extra()).max(1.0 / opts.fps as f64);
    let total = (duration * opts.fps as f64).ceil() as u32;

    // ── Pistes audio (en temps réel) ──
    let mut segments = super::speech::real_time(&timeline.audio_segments(), holds);
    segments.extend(super::speech::real_time(&opts.voice.segments, holds));
    let mut audio_inputs = Vec::new();
    for seg in &segments {
        let path = opts.base_dir.join(&seg.file);
        if !path.is_file() {
            return Err(format!("fichier audio introuvable : {}", path.display()));
        }
        audio_inputs.push(path);
    }

    // ── Lancement de ffmpeg ──
    let mut cmd = Command::new(&opts.ffmpeg);
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
    cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba"]);
    cmd.args(["-s", &format!("{}x{}", w, h), "-r", &opts.fps.to_string(), "-i", "-"]);
    for path in &audio_inputs {
        cmd.arg("-i").arg(path);
    }
    if !segments.is_empty() {
        cmd.args(["-filter_complex", &audio_filter(&segments, duration, 1)]);
        cmd.args(["-map", "0:v", "-map", "[aout]", "-c:a", "aac", "-b:a", "192k"]);
    }
    cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p"]);
    cmd.args(["-movflags", "+faststart", "-t", &format!("{:.3}", duration)]);
    cmd.arg(&opts.output);
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "ffmpeg introuvable ('{}'). Installez-le (Windows : winget install Gyan.FFmpeg, \
                 puis rouvrez le terminal) ou indiquez son chemin avec --ffmpeg <chemin>",
                opts.ffmpeg
            )
        } else {
            format!("impossible de lancer ffmpeg : {}", e)
        }
    })?;

    // ffmpeg écrit ses erreurs pendant l'encodage : on les lit à côté pour ne pas le bloquer
    let mut stderr = child.stderr.take().expect("stderr");
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    // ── Rendu des images ──
    let mut backend = SkiaBackend::new(w, h).ok_or("taille de vidéo invalide")?;
    let scene = Scene::new(config, w, h);
    let mut warnings = BTreeSet::new();
    let mut write_error = None;
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for i in 0..total {
            let t = holds.to_timeline(i as f64 / opts.fps as f64);
            let frame = timeline.frame_at(t, opts.lang.as_deref());
            for (name, reason) in renderer::render_frame_parallel(&frame, &scene, &mut backend) {
                warnings.insert(format!("'{}' non dessinée : {}", name, reason));
            }
            if opts.subtitles {
                if let Some(c) = frame.comment {
                    if let Err(e) = draw_subtitle(&mut backend, c) {
                        warnings.insert(format!("sous-titre non dessiné : {}", e));
                    }
                }
            }
            if let Err(e) = stdin.write_all(backend.pixmap.data()) {
                write_error = Some(e);
                break;
            }
            progress(i + 1, total);
        }
        // stdin est fermé ici : ffmpeg termine le fichier
    }

    let status = child.wait().map_err(|e| format!("ffmpeg : {}", e))?;
    let log = errors.join().unwrap_or_default();
    if !status.success() || write_error.is_some() {
        let detail = if log.trim().is_empty() {
            write_error.map(|e| e.to_string()).unwrap_or_else(|| status.to_string())
        } else {
            log.trim().to_string()
        };
        return Err(format!("ffmpeg a échoué : {}", detail));
    }

    Ok(VideoReport {
        frames: total,
        width: w,
        height: h,
        duration,
        audio_tracks: segments.len(),
        warnings: warnings.into_iter().collect(),
    })
}

/// Filtre ffmpeg qui place chaque piste à son instant, avec son volume, puis les mélange.
/// La piste i est l'entrée `first + i` (pour la vidéo, l'entrée 0 est l'image).
fn audio_filter(segments: &[AudioSegment], duration: f64, first: usize) -> String {
    let mut parts = Vec::new();
    let mut labels = String::new();
    for (i, seg) in segments.iter().enumerate() {
        let length = seg.end.unwrap_or(duration) - seg.start;
        let delay = (seg.start * 1000.0).round() as i64;
        parts.push(format!(
            "[{}:a]atrim=0:{:.3},asetpts=PTS-STARTPTS,volume={},adelay={}:all=1[a{}]",
            i + first,
            length.max(0.0),
            seg.volume,
            delay,
            i
        ));
        labels.push_str(&format!("[a{}]", i));
    }
    if segments.len() == 1 {
        parts.push(format!("{}apad[aout]", labels));
    } else {
        parts.push(format!(
            "{}amix=inputs={}:duration=longest:normalize=0,apad[aout]",
            labels,
            segments.len()
        ));
    }
    parts.join(";")
}

/// Export du son seul (le « podcast ») : pistes audio et voix mélangées, en MP3
pub fn export_audio(
    segments: &[AudioSegment],
    duration: f64,
    base_dir: &Path,
    output: &Path,
    ffmpeg: &str,
) -> Result<(), String> {
    if segments.is_empty() {
        return Err("aucun son à exporter (ni piste audio, ni voix : lancez d'abord --voice)".to_string());
    }
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
    for seg in segments {
        let path = base_dir.join(&seg.file);
        if !path.is_file() {
            return Err(format!("fichier audio introuvable : {}", path.display()));
        }
        cmd.arg("-i").arg(path);
    }
    cmd.args(["-filter_complex", &audio_filter(segments, duration, 0)]);
    cmd.args(["-map", "[aout]", "-t", &format!("{:.3}", duration), "-c:a", "libmp3lame", "-q:a", "2"]);
    cmd.arg(output);
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("ffmpeg introuvable ('{}') : voir --ffmpeg", ffmpeg)
        } else {
            e.to_string()
        }
    })?;
    if !out.status.success() {
        return Err(format!("ffmpeg a échoué : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

// ─────────────────────────────────────────────
//  SOUS-TITRES
// ─────────────────────────────────────────────

/// Dessine le commentaire en bas de l'image, sur un fond sombre (même aspect que la fenêtre)
fn draw_subtitle(backend: &mut SkiaBackend, comment: &str) -> Result<(), String> {
    let (w, h) = (backend.width() as f32, backend.height() as f32);
    let size = (h * 0.04).clamp(12.0, 64.0);
    let text = wrap(comment, size, w * 0.9)?;
    let (tw, th) = text::text_size(&text, size)?;

    let pad = size * 0.4;
    let cx = w / 2.0;
    let cy = h - h * 0.05 - th / 2.0;
    let bg = Shape { glow: 0.0,
        path: rounded_rect(cx - tw / 2.0 - pad, cy - th / 2.0 - pad, tw + 2.0 * pad, th + 2.0 * pad, size * 0.25),
        fill: Some(Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.67 }),
        stroke: None,
    };
    backend.draw(&bg);
    for shape in text::text_shapes(&text, cx, cy, size, Align::Center, Rgba::WHITE)? {
        backend.draw(&shape);
    }
    Ok(())
}

/// Coupe le texte en lignes de largeur max `max_w` (les maths `$...$` ne sont jamais coupées)
fn wrap(comment: &str, size: f32, max_w: f32) -> Result<String, String> {
    let mut out = Vec::new();
    for paragraph in comment.split('\n') {
        let mut line = String::new();
        for word in words(paragraph) {
            let candidate = if line.is_empty() { word.clone() } else { format!("{} {}", line, word) };
            if !line.is_empty() && text::text_size(&candidate, size)?.0 > max_w {
                out.push(std::mem::replace(&mut line, word));
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    Ok(out.join("\n"))
}

/// Mots séparés par des espaces, en gardant chaque `$...$` d'un seul tenant
fn words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_math = false;
    for c in s.chars() {
        if c == '$' {
            in_math = !in_math;
        }
        if c == ' ' && !in_math {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<PathCmd> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = r * 0.5523; // approximation d'un quart de cercle par une courbe de Bézier
    vec![
        PathCmd::MoveTo(x + r, y),
        PathCmd::LineTo(x + w - r, y),
        PathCmd::CubicTo(x + w - r + k, y, x + w, y + r - k, x + w, y + r),
        PathCmd::LineTo(x + w, y + h - r),
        PathCmd::CubicTo(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h),
        PathCmd::LineTo(x + r, y + h),
        PathCmd::CubicTo(x + r - k, y + h, x, y + h - r + k, x, y + h - r),
        PathCmd::LineTo(x, y + r),
        PathCmd::CubicTo(x, y + r - k, x + r - k, y, x + r, y),
        PathCmd::Close,
    ]
}

/// Chemin de sortie par défaut : cours.dct → cours.mp4 (ou cours_EN.mp4 avec une langue)
pub fn default_output(source: &Path, lang: Option<&str>, ext: &str) -> PathBuf {
    let stem = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("video".into());
    let name = match lang {
        Some(l) => format!("{}_{}.{}", stem, l, ext),
        None => format!("{}.{}", stem, ext),
    };
    source.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mots_et_maths() {
        assert_eq!(words("la dérivée $f'(x) = 2 x$ vaut"), vec!["la", "dérivée", "$f'(x) = 2 x$", "vaut"]);
    }

    #[test]
    fn taille_paire() {
        let config = Config::default();
        let (w, h) = video_size(&config, Some(721));
        assert_eq!((w % 2, h % 2), (0, 0));
    }
}
