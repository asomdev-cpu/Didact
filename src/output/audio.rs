//! Son dans la fenêtre : lecture des pistes `audio` de la timeline.
//!
//! - Les fichiers sont décodés une fois au démarrage (symphonia : mp3, wav, ogg, flac, m4a…).
//! - La carte son (cpal) demande régulièrement des échantillons ; on les calcule en
//!   mélangeant les pistes actives à l'instant joué. Avancer, reculer ou sauter est donc
//!   immédiat : la position dans chaque fichier se déduit du temps.
//! - L'horloge du son sert de référence à la fenêtre pendant la lecture, pour que l'image
//!   et le son restent synchronisés.

use crate::runtime::timeline::AudioSegment;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Un fichier audio décodé
pub struct Clip {
    /// Échantillons entrelacés (gauche, droite, gauche, …), entre -1 et 1
    samples: Vec<f32>,
    channels: usize,
    rate: u32,
}

impl Clip {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate as f64
    }
}

/// Une piste prête à jouer
struct Track {
    clip: Arc<Clip>,
    volume: f32,
    start: f64,
    end: Option<f64>,
    /// Recommence à la fin du fichier
    looping: bool,
    /// Voix d'une langue (jouée seulement dans cette langue) ; None = toujours
    lang: Option<usize>,
}

/// Temps de lecture partagé entre la fenêtre et la carte son
#[derive(Clone, Copy)]
struct Clock {
    /// Temps de la timeline au moment `at`
    t: f64,
    at: Instant,
    playing: bool,
}

impl Clock {
    fn now(&self) -> f64 {
        if self.playing { self.t + self.at.elapsed().as_secs_f64() } else { self.t }
    }
}

/// Lecteur audio de la fenêtre. Tant qu'il existe, le son est joué.
pub struct AudioPlayer {
    _stream: cpal::Stream,
    clock: Arc<Mutex<Clock>>,
    /// Langue jouée (position dans `languages`), pour les voix
    lang: Arc<AtomicUsize>,
}

impl AudioPlayer {
    /// Décode les pistes et ouvre la sortie audio par défaut.
    /// Chaque piste peut être réservée à une langue (voix) : `Some(position dans languages)`.
    /// Renvoie aussi les avertissements (fichier illisible : piste ignorée).
    pub fn new(
        segments: &[(AudioSegment, Option<usize>)],
        base_dir: &Path,
        lang: usize,
    ) -> Result<(AudioPlayer, Vec<String>), String> {
        let mut warnings = Vec::new();
        let mut clips: Vec<(String, Arc<Clip>)> = Vec::new();
        let mut tracks = Vec::new();
        for (seg, seg_lang) in segments {
            let clip = match clips.iter().find(|(f, _)| *f == seg.file) {
                Some((_, c)) => c.clone(),
                None => match decode(&base_dir.join(&seg.file)) {
                    Ok(c) => {
                        let c = Arc::new(c);
                        clips.push((seg.file.clone(), c.clone()));
                        c
                    }
                    Err(e) => {
                        warnings.push(format!("audio '{}' ignoré : {}", seg.file, e));
                        continue;
                    }
                },
            };
            tracks.push(Track {
                clip,
                volume: seg.volume as f32,
                start: seg.start,
                end: seg.end,
                looping: seg.looping,
                lang: *seg_lang,
            });
        }

        let device = cpal::default_host().default_output_device().ok_or("aucune sortie audio trouvée")?;
        let supported = device.default_output_config().map_err(|e| format!("sortie audio : {}", e))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let out_channels = config.channels as usize;
        let out_rate = config.sample_rate.0 as f64;

        let clock = Arc::new(Mutex::new(Clock { t: 0.0, at: Instant::now(), playing: false }));
        let lang = Arc::new(AtomicUsize::new(lang));
        let mut mixer =
            Mixer { tracks, clock: clock.clone(), lang: lang.clone(), out_channels, out_rate, buffer: Vec::new() };
        let on_error = |e| eprintln!("Attention : erreur audio : {}", e);

        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(&config, move |data: &mut [f32], _| mixer.fill(data), on_error, None),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    let buf = mixer.fill_buffer(data.len());
                    for (d, s) in data.iter_mut().zip(buf) {
                        *d = (s * i16::MAX as f32) as i16;
                    }
                },
                on_error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &config,
                move |data: &mut [u16], _| {
                    let buf = mixer.fill_buffer(data.len());
                    for (d, s) in data.iter_mut().zip(buf) {
                        *d = ((s * 0.5 + 0.5) * u16::MAX as f32) as u16;
                    }
                },
                on_error,
                None,
            ),
            other => return Err(format!("format audio non géré : {:?}", other)),
        }
        .map_err(|e| format!("sortie audio : {}", e))?;
        stream.play().map_err(|e| format!("sortie audio : {}", e))?;

        Ok((AudioPlayer { _stream: stream, clock, lang }, warnings))
    }

    /// Change la langue des voix
    pub fn set_lang(&self, lang: usize) {
        self.lang.store(lang, Ordering::Relaxed);
    }

    /// Temps de lecture selon l'horloge du son
    pub fn now(&self) -> f64 {
        self.clock.lock().map(|c| c.now()).unwrap_or(0.0)
    }

    /// Recale le son sur la fenêtre : à appeler à chaque image.
    /// L'horloge n'est modifiée que si la lecture démarre/s'arrête ou si l'on a sauté
    /// (sinon de petits recalages produiraient des craquements).
    pub fn sync(&self, t: f64, playing: bool) {
        if let Ok(mut c) = self.clock.lock() {
            if c.playing != playing || (c.now() - t).abs() > 0.05 {
                *c = Clock { t, at: Instant::now(), playing };
            }
        }
    }
}

/// Mélangeur appelé par la carte son
struct Mixer {
    tracks: Vec<Track>,
    clock: Arc<Mutex<Clock>>,
    lang: Arc<AtomicUsize>,
    out_channels: usize,
    out_rate: f64,
    buffer: Vec<f32>,
}

impl Mixer {
    fn fill_buffer(&mut self, len: usize) -> Vec<f32> {
        let mut buf = std::mem::take(&mut self.buffer);
        buf.resize(len, 0.0);
        self.fill(&mut buf);
        let out = buf.clone();
        self.buffer = buf;
        out
    }

    fn fill(&mut self, data: &mut [f32]) {
        data.iter_mut().for_each(|s| *s = 0.0);
        let clock = match self.clock.try_lock() {
            Ok(c) => *c,
            Err(_) => return, // très rare : un tampon de silence
        };
        if !clock.playing {
            return;
        }
        let start_time = clock.now();
        let lang = self.lang.load(Ordering::Relaxed);
        let channels = self.out_channels.max(1);
        for (i, frame) in data.chunks_mut(channels).enumerate() {
            let time = start_time + i as f64 / self.out_rate;
            for tr in &self.tracks {
                if time < tr.start || tr.end.is_some_and(|e| time >= e) || tr.lang.is_some_and(|l| l != lang) {
                    continue;
                }
                let clip = &tr.clip;
                let mut pos = (time - tr.start) * clip.rate as f64;
                if tr.looping && clip.frames() > 1 {
                    pos %= (clip.frames() - 1) as f64;
                }
                let i0 = pos.floor() as usize;
                if i0 + 1 >= clip.frames() {
                    continue;
                }
                let frac = (pos - i0 as f64) as f32;
                for (c, out) in frame.iter_mut().enumerate() {
                    let sc = c.min(clip.channels - 1);
                    let a = clip.samples[i0 * clip.channels + sc];
                    let b = clip.samples[(i0 + 1) * clip.channels + sc];
                    *out += (a + (b - a) * frac) * tr.volume;
                }
            }
            for out in frame.iter_mut() {
                *out = out.clamp(-1.0, 1.0);
            }
        }
    }
}

/// Décode un fichier audio entier en mémoire
pub fn decode(path: &Path) -> Result<Clip, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).map_err(|e| format!("{} : {}", path.display(), e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("format non reconnu ({})", e))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("aucune piste audio dans le fichier")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("codec non géré ({})", e))?;

    let mut samples = Vec::new();
    let (mut channels, mut rate) = (
        track.codec_params.channels.map(|c| c.count()).unwrap_or(0),
        track.codec_params.sample_rate.unwrap_or(0),
    );
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(format!("lecture : {}", e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                let spec = *audio.spec();
                channels = spec.channels.count();
                rate = spec.rate;
                let mut buf = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
                buf.copy_interleaved_ref(audio);
                samples.extend_from_slice(buf.samples());
            }
            Err(Error::DecodeError(_)) => continue, // paquet abîmé : on saute
            Err(e) => return Err(format!("décodage : {}", e)),
        }
    }
    if channels == 0 || rate == 0 || samples.is_empty() {
        return Err("fichier audio vide".to_string());
    }
    Ok(Clip { samples, channels, rate })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Écrit un petit WAV (mono, 16 bits) : un la à 440 Hz pendant `secs` secondes
    fn wav(path: &Path, secs: f32, rate: u32) {
        let n = (secs * rate as f32) as u32;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n * 2).to_le_bytes());
        for i in 0..n {
            let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5;
            b.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn decodage_et_melange() {
        let dir = std::env::temp_dir().join(format!("didact_audio_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("la.wav");
        wav(&path, 1.0, 8000);
        let clip = decode(&path).unwrap();
        assert_eq!((clip.channels, clip.rate), (1, 8000));
        assert!((clip.duration() - 1.0).abs() < 0.01);
        assert!(decode(&dir.join("absent.wav")).is_err());

        // Mélange : piste de 2 s à 3 s (volume 0.5), sortie stéréo
        let clip = Arc::new(clip);
        let clock = Arc::new(Mutex::new(Clock { t: 0.0, at: Instant::now(), playing: true }));
        let mut m = Mixer {
            tracks: vec![Track { clip, volume: 0.5, start: 2.0, end: Some(3.0), looping: false, lang: None }],
            clock: clock.clone(),
            lang: Arc::new(AtomicUsize::new(0)),
            out_channels: 2,
            out_rate: 8000.0,
            buffer: Vec::new(),
        };
        let energy = |m: &mut Mixer, t: f64| {
            *clock.lock().unwrap() = Clock { t, at: Instant::now(), playing: true };
            let mut d = vec![0.0f32; 800];
            m.fill(&mut d);
            d.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
        };
        assert_eq!(energy(&mut m, 1.0), 0.0); // avant la piste
        let e = energy(&mut m, 2.2);
        assert!(e > 0.2 && e <= 0.26, "{}", e); // 0.5 × 0.5
        assert_eq!(energy(&mut m, 3.5), 0.0); // après `end`

        // En boucle : un fichier d'1 s joue encore 5 s après son début
        m.tracks[0].end = None;
        assert_eq!(energy(&mut m, 7.2), 0.0, "sans loop : fini");
        m.tracks[0].looping = true;
        let e = energy(&mut m, 7.2);
        assert!(e > 0.2 && e <= 0.26, "{}", e);
        std::fs::remove_dir_all(&dir).ok();
    }
}
