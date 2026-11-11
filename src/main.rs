use didact::{check, lexer, output, parser, renderer, resolver, runtime, validator};

use std::env;
use std::fs;

const USAGE: &str = "Usage: didact <fichier.dct> [options]
       didact shift <fichier.dct> <à partir de> <décalage>

Sans option : ouvre une fenêtre et joue l'animation.

shift : décale tous les blocs qui commencent à <à partir de> ou après, et réécrit le fichier.
  didact shift cours.dct 16 +2      ajoute 2 s à partir de 16 s
  didact shift cours.dct 16 -1.5    retire 1,5 s à partir de 16 s

Options :
  --png <t> <image.png>   enregistre l'image à l'instant t (en secondes), sans fenêtre
  --summary               affiche les figures et la timeline dans la console, sans fenêtre
  --check                 vérifie tout sans fenêtre : erreurs, figures hors du cadre, textes qui se
                          chevauchent (à chaque bloc, fin d'animation et toutes les 0,25 s)
      --format json         sortie lisible par un programme : une liste d'objets
                            {severity, code, line, time, figure, other, message}
                          code de retour : 0 rien, 2 seulement des avertissements, 1 des erreurs
  --export mp4            exporte la vidéo (nécessite ffmpeg)
      -o <fichier>          fichier de sortie (défaut : <nom>.mp4, ou <nom>_<LANG>.mp4 avec --lang)
      --fps <n>             images par seconde (défaut : 30)
      --height <px>         hauteur de la vidéo, ex : 720 (défaut : 1080)
      --no-subtitles        sans les commentaires incrustés
      --ffmpeg <chemin>     chemin de ffmpeg s'il n'est pas dans le PATH
  --export html           page web autonome (un seul fichier, s'ouvre sans serveur)
      -o <fichier>          fichier de sortie (défaut : <nom>.html)
      --engine <dossier>    dossier du moteur compilé (défaut : web/pkg)
  --export audio          le son seul (pistes audio et voix), en MP3 : le « podcast » (nécessite ffmpeg)
      -o <fichier>          fichier de sortie (défaut : <nom>.mp3, ou <nom>_<LANG>.mp3 avec --lang)
  --voice                 fait lire les commentaires par une voix de synthèse (voir didact-voix.txt) ;
                          les phrases sont ensuite jouées partout (fenêtre, web, MP4, HTML)
      --voice-config <f>    fichier de configuration (défaut : didact-voix.txt à côté du .dct)
  --lang <XX>             langue des commentaires (ex: --lang EN), sinon default_lang
  --debug                 affiche aussi les tokens et l'AST complets";

fn main() {
    // ── Arguments ──
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(|a| a.as_str()) == Some("shift") {
        return shift_command(&args[1..]);
    }
    let mut filename = None;
    let mut lang: Option<String> = None;
    let mut debug = false;
    let mut summary = false;
    let mut png: Option<(f64, String)> = None;
    let mut export: Option<String> = None;
    let mut out: Option<String> = None;
    let mut fps: u32 = 30;
    let mut height: Option<u32> = None;
    let mut subtitles = true;
    let mut ffmpeg = String::from("ffmpeg");
    let mut engine: Option<String> = None;
    let mut voice = false;
    let mut voice_config: Option<String> = None;
    let mut check_mode = false;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--debug" => debug = true,
            "--summary" => summary = true,
            "--check" => check_mode = true,
            "--format" => {
                i += 1;
                json = match args.get(i).map(|s| s.as_str()) {
                    Some("json") => true,
                    Some("text") => false,
                    _ => fail("--format attend json ou text"),
                };
            }
            "--lang" => {
                i += 1;
                lang = Some(args.get(i).cloned().unwrap_or_else(|| fail("--lang attend une langue")));
            }
            "--png" => {
                let t = args
                    .get(i + 1)
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or_else(|| fail("--png attend un temps en secondes, ex: --png 2.5 image.png"));
                let path = args
                    .get(i + 2)
                    .cloned()
                    .unwrap_or_else(|| fail("--png attend un nom de fichier, ex: --png 2.5 image.png"));
                png = Some((t, path));
                i += 2;
            }
            "--export" => {
                i += 1;
                export = Some(args.get(i).cloned().unwrap_or_else(|| fail("--export attend un format : mp4")));
            }
            "-o" | "--output" => {
                i += 1;
                out = Some(args.get(i).cloned().unwrap_or_else(|| fail("-o attend un nom de fichier")));
            }
            "--fps" => {
                i += 1;
                fps = args
                    .get(i)
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|&f| (1..=120).contains(&f))
                    .unwrap_or_else(|| fail("--fps attend un nombre entre 1 et 120"));
            }
            "--height" => {
                i += 1;
                height = Some(
                    args.get(i)
                        .and_then(|s| s.parse::<u32>().ok())
                        .filter(|&h| (16..=4320).contains(&h))
                        .unwrap_or_else(|| fail("--height attend une hauteur en pixels, ex : 720")),
                );
            }
            "--no-subtitles" => subtitles = false,
            "--ffmpeg" => {
                i += 1;
                ffmpeg = args.get(i).cloned().unwrap_or_else(|| fail("--ffmpeg attend un chemin"));
            }
            "--voice" => voice = true,
            "--voice-config" => {
                i += 1;
                voice_config = Some(args.get(i).cloned().unwrap_or_else(|| fail("--voice-config attend un fichier")));
            }
            "--engine" => {
                i += 1;
                engine = Some(args.get(i).cloned().unwrap_or_else(|| fail("--engine attend un dossier")));
            }
            other if other.starts_with("--") => fail(&format!("Option inconnue '{}'\n\n{}", other, USAGE)),
            other => filename = Some(other.to_string()),
        }
        i += 1;
    }

    let filename = filename.unwrap_or_else(|| fail(USAGE));

    let content = fs::read_to_string(&filename).unwrap_or_else(|e| {
        fail(&format!("Erreur lors de la lecture du fichier '{}': {}", filename, e))
    });

    // ── Vérification seule (--check) ──
    if check_mode {
        let diags = check::check(&content, lang.as_deref());
        if json {
            println!("{}", check::to_json(&diags));
        } else if diags.is_empty() {
            println!("✓ {} : aucun problème", filename);
        } else {
            let errors = diags.iter().filter(|d| d.severity == check::Severity::Error).count();
            println!("{} : {} erreur(s), {} avertissement(s)", filename, errors, diags.len() - errors);
            for d in &diags {
                let mark = if d.severity == check::Severity::Error { "✗" } else { "⚠" };
                println!("  {} {}", mark, d.to_text());
            }
        }
        std::process::exit(check::exit_code(&diags));
    }

    // ── Étape 1 : Lexer ──
    let tokens = lexer::tokenize(&content);
    if debug {
        println!("--- Tokens ({} trouvés) ---", tokens.len());
        for tok in &tokens {
            println!("  ligne {:>3} : {:?}", tok.line, tok.token);
        }
        println!();
    }

    // ── Étape 2 : Parser ──
    let doc = parser::parse_with_source(tokens, &content).unwrap_or_else(|e| fail(&format!("✗ Erreur de parsing : {}", e)));
    if debug {
        println!("--- Document parsé ---\n{:#?}\n", doc);
    }

    // ── Étape 3 : Validateur ──
    if let Err(errors) = validator::validate(&doc) {
        eprintln!("✗ {} erreur(s) de validation :", errors.len());
        for e in &errors {
            eprintln!("  - {}", e);
        }
        std::process::exit(1);
    }

    // ── Étape 4 : Résolveur ──
    let resolved = resolver::resolve(&doc);

    // ── Étape 5 : Runtime ──
    let timeline = runtime::Timeline::new(&resolved);

    // ── Sortie ──
    let source = std::path::Path::new(&filename);
    let base_dir = source.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    if voice {
        generate_voices(&timeline, source, voice_config.as_deref());
    } else if summary {
        print_summary(&resolved, &timeline, lang.as_deref());
    } else if let Some(format) = export {
        // Voix de la langue choisie (si elles ont été générées)
        let lang_name = timeline.languages().get(timeline.lang_index(lang.as_deref())).cloned().unwrap_or_default();
        let voice = output::speech::plan(&timeline, source, &lang_name);
        match format.as_str() {
            "mp4" => {
                let output = out
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| output::video::default_output(source, lang.as_deref(), "mp4"));
                let opts = output::video::VideoOptions { output, fps, height, lang, subtitles, ffmpeg, base_dir, voice };
                export_mp4(&resolved, &timeline, &opts);
            }
            "html" => {
                let output = out
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| output::video::default_output(source, None, "html"));
                let (w, h) = renderer::Scene::default_size(&resolved.config);
                let opts = output::html::HtmlOptions {
                    output,
                    title: source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    lang,
                    engine: engine.map(std::path::PathBuf::from),
                    ratio: w as f64 / h as f64,
                    audio: audio_files(&timeline, source),
                    voice_dir: Some(output::speech::voice_dir(source)).filter(|d| d.is_dir()).and_then(|d| {
                        let index = std::fs::read_to_string(d.join("index.txt")).ok()?;
                        Some((d.file_name()?.to_string_lossy().to_string(), index))
                    }),
                };
                match output::html::export(&content, &opts) {
                    Ok(r) => println!(
                        "✓ Page autonome enregistrée dans {} ({:.1} Mo, moteur : {})",
                        opts.output.display(),
                        r.size as f64 / 1_000_000.0,
                        r.engine_dir.display()
                    ),
                    Err(e) => fail(&format!("✗ Export HTML : {}", e)),
                }
            }
            "audio" => {
                let output = out
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| output::video::default_output(source, lang.as_deref(), "mp3"));
                let mut segments = output::speech::real_time(&timeline.audio_segments(), &voice.holds);
                segments.extend(output::speech::real_time(&voice.segments, &voice.holds));
                let duration = timeline.duration() + voice.holds.extra();
                match output::video::export_audio(&segments, duration, &base_dir, &output, &ffmpeg) {
                    Ok(()) => println!(
                        "✓ Son enregistré dans {} ({:.1}s, {} piste(s))",
                        output.display(),
                        duration,
                        segments.len()
                    ),
                    Err(e) => fail(&format!("✗ Export audio : {}", e)),
                }
            }
            other => fail(&format!("format d'export inconnu '{}' (mp4, html, audio)", other)),
        }
    } else if let Some((t, path)) = png {
        export_png(&resolved, &timeline, t, &path, lang.as_deref());
    } else {
        let title = std::path::Path::new(&filename)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(filename.clone());
        // Voix de toutes les langues (on peut changer de langue pendant la lecture)
        let voices: Vec<output::speech::VoicePlan> =
            timeline.languages().iter().map(|l| output::speech::plan(&timeline, source, l)).collect();
        if let Err(e) = output::window::run(timeline, resolved.config.clone(), lang, &title, source, voices) {
            fail(&format!("✗ Impossible d'ouvrir la fenêtre : {}", e));
        }
    }
}

/// Enregistre l'image de l'instant t en PNG
fn export_png(resolved: &resolver::ResolvedDocument, timeline: &runtime::Timeline, t: f64, path: &str, lang: Option<&str>) {
    let (w, h) = renderer::Scene::default_size(&resolved.config);
    let mut backend = renderer::skia::SkiaBackend::new(w, h).unwrap_or_else(|| fail("taille d'image invalide"));
    let scene = renderer::Scene::new(&resolved.config, w, h);
    let frame = timeline.frame_at(t, lang);
    for (name, reason) in renderer::render_frame_parallel(&frame, &scene, &mut backend) {
        eprintln!("Attention : '{}' non dessinée : {}", name, reason);
    }
    backend.save_png(path).unwrap_or_else(|e| fail(&format!("✗ Écriture de '{}' impossible : {}", path, e)));
    println!("✓ Image t={:.3}s enregistrée dans {} ({}×{})", t, path, w, h);
}

/// Génère les voix (--voice), avec la progression dans la console
fn generate_voices(timeline: &runtime::Timeline, source: &std::path::Path, config: Option<&str>) {
    use std::io::Write;
    let cfg_path = output::speech::find_config(config.map(std::path::Path::new), source).unwrap_or_else(|e| fail(&format!("✗ Voix : {}", e)));
    let cfg = output::speech::read_config(&cfg_path).unwrap_or_else(|e| fail(&format!("✗ Voix : {}", e)));
    println!("Voix : configuration {}", cfg_path.display());
    let result = output::speech::generate(timeline, source, &cfg, |done, total, text| {
        let short: String = text.chars().take(60).collect();
        print!("\r\x1b[K  phrase {}/{} : {}", done + 1, total, short);
        let _ = std::io::stdout().flush();
    });
    println!("\r\x1b[K");
    match result {
        Ok(r) => {
            for l in &r.skipped_langs {
                eprintln!("Attention : pas de commande pour la langue {} dans {}", l, cfg_path.display());
            }
            for w in &r.warnings {
                eprintln!("Attention : {}", w);
            }
            println!(
                "✓ Voix dans {} : {} phrase(s) générée(s), {} déjà prête(s)",
                r.dir.display(),
                r.generated,
                r.reused
            );
        }
        Err(e) => fail(&format!("✗ Voix : {}", e)),
    }
}

/// Fichiers audio utilisés par la timeline, voix comprises :
/// (nom tel qu'il est cherché, chemin à côté du .dct)
fn audio_files(timeline: &runtime::Timeline, source: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let base = source.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    let voices = timeline.languages().iter().flat_map(|l| output::speech::plan(timeline, source, l).segments);
    for seg in timeline.audio_segments().into_iter().chain(voices) {
        if !files.iter().any(|(f, _)| *f == seg.file) {
            files.push((seg.file.clone(), base.join(&seg.file)));
        }
    }
    files
}

/// Exporte la vidéo MP4, avec une barre de progression dans la console
fn export_mp4(resolved: &resolver::ResolvedDocument, timeline: &runtime::Timeline, opts: &output::video::VideoOptions) {
    use std::io::Write;
    let start = std::time::Instant::now();
    let mut last_percent = u32::MAX;
    let result = output::video::export(timeline, &resolved.config, opts, |done, total| {
        let percent = done * 100 / total.max(1);
        if percent != last_percent {
            last_percent = percent;
            let bar = "█".repeat((percent / 5) as usize) + &"░".repeat(20 - (percent / 5) as usize);
            print!("\r  {} {:>3}%  image {}/{}", bar, percent, done, total);
            let _ = std::io::stdout().flush();
        }
    });
    println!();
    match result {
        Ok(r) => {
            for w in &r.warnings {
                eprintln!("Attention : {}", w);
            }
            let audio = match r.audio_tracks {
                0 => String::new(),
                1 => ", 1 piste audio".to_string(),
                n => format!(", {} pistes audio", n),
            };
            println!(
                "✓ Vidéo enregistrée dans {} ({}×{}, {:.1}s, {} images à {} fps{}) en {:.1}s",
                opts.output.display(),
                r.width,
                r.height,
                r.duration,
                r.frames,
                opts.fps,
                audio,
                start.elapsed().as_secs_f64()
            );
        }
        Err(e) => fail(&format!("✗ Export MP4 : {}", e)),
    }
}

/// Résumé texte : figures résolues et état après chaque bloc de la timeline
fn print_summary(resolved: &resolver::ResolvedDocument, timeline: &runtime::Timeline, lang: Option<&str>) {
    println!("✓ Parsing, validation, résolution\n");
    println!("--- Figures (ordre de dessin) ---");
    for f in &resolved.figures {
        let mut keys: Vec<&String> = f.properties.keys().collect();
        keys.sort();
        let props: Vec<String> = keys
            .iter()
            .filter(|k| k.as_str() != "type")
            .map(|k| format!("{}={}", k, resolver::fmt_value(&f.properties[*k])))
            .collect();
        println!("  {} [{}, layer {}] : {}", f.name, f.ftype, f.layer, props.join(", "));
    }

    println!("\n--- Timeline (durée {:.3}s) ---", timeline.duration());
    for &t in timeline.times() {
        let frame = timeline.frame_at(t, lang);
        let names: Vec<&str> = frame.visible.iter().map(|f| f.name.as_str()).collect();
        println!("  t={:>8.3}  visibles : [{}]", t, names.join(", "));
        if let Some(c) = frame.comment {
            println!("              comment  : \"{}\"", c);
        }
        for a in &frame.audio {
            println!("              audio    : {} (volume {}, à {:.3}s)", a.file, a.volume, a.position);
        }
    }
}

/// Affiche un message d'erreur et quitte
fn fail(msg: &str) -> ! {
    eprintln!("{}", msg);
    std::process::exit(1);
}

/// didact shift <fichier.dct> <à partir de> <décalage>
fn shift_command(args: &[String]) {
    const FORM: &str = "Usage : didact shift <fichier.dct> <à partir de> <décalage>\n  ex : didact shift cours.dct 16 +2";
    let [file, start, delta] = args else { fail(FORM) };
    let number = |s: &str, what: &str| {
        s.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or_else(|| fail(&format!("{} : '{}' n'est pas un nombre\n\n{}", what, s, FORM)))
    };
    let (start, delta) = (number(start, "à partir de"), number(delta, "décalage"));
    let source = fs::read_to_string(file).unwrap_or_else(|e| fail(&format!("Lecture de '{}' impossible : {}", file, e)));
    let (text, moved) = didact::shift::shift(&source, start, delta).unwrap_or_else(|e| fail(&format!("✗ Rien n'a été modifié : {}", e)));
    fs::write(file, text).unwrap_or_else(|e| fail(&format!("Écriture de '{}' impossible : {}", file, e)));
    let n = moved.len();
    println!("✓ {} bloc{} décalé{} de {:+.3} s :", n, if n > 1 { "s" } else { "" }, if n > 1 { "s" } else { "" }, delta);
    for m in &moved {
        println!("  ligne {:>4} : {:>9.3} → {:.3}", m.line, m.from, m.to);
    }
}
