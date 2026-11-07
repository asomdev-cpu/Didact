// ─────────────────────────────────────────────
//  Didact — lecteur pour le web
//
//  <script type="module" src="didact.js"></script>
//  <didact-player src="cours.dct" lang="FR"></didact-player>
//
//  Attributs :
//    src       fichier .dct à charger (sinon : <script type="text/didact"> à l'intérieur)
//    lang      langue des sous-titres (sinon default_lang du fichier)
//    autoplay  "false" pour ne pas démarrer tout seul
//    controls  "false" pour cacher la barre de contrôles
//
//  Voix : si `didact cours.dct --voice` a été lancé, le dossier cours_voix/ (à côté du
//  .dct) contient les commentaires lus ; ils sont joués dans la langue choisie.
//
//  Son : les pistes `audio` sont jouées avec <audio>, chemins relatifs au fichier .dct
//  (ou à la page pour un fichier écrit dans la page). Les navigateurs bloquent souvent
//  le son tant que le visiteur n'a pas cliqué : il démarre alors au premier clic sur ▶.
//
//  Le moteur (pkg/didact_bg.wasm) est chargé une seule fois pour toute la page.
//  (Dans une page faite par `didact --export html`, il est inclus dans la page.)
// ─────────────────────────────────────────────

let ready = null;

/**
 * Charge le moteur WebAssembly (une seule fois, partagé par tous les lecteurs).
 * Renvoie le module : `const { Player } = await initDidact();`
 */
export function initDidact() {
  if (!ready && typeof globalThis.__DIDACT_ENGINE__ === "function") {
    // Page autonome (didact --export html) : le moteur est inclus dans la page
    ready = Promise.resolve().then(() => globalThis.__DIDACT_ENGINE__());
  }
  if (!ready) {
    ready = import("./pkg/didact.js")
      .catch(() => {
        throw new Error(
          "Moteur Didact introuvable (pkg/didact.js).\n\n" +
          "Il faut d'abord le compiler, depuis la racine du projet :\n" +
          "    .\\web\\build.ps1\n\n" +
          "Voir Readme/WEB.md."
        );
      })
      .then(async (module) => {
        await module.default();
        return module;
      });
  }
  return ready;
}

const STYLE = `
  :host { display: block; max-width: 100%; font-family: system-ui, sans-serif; }
  .stage { position: relative; width: 100%; aspect-ratio: 16 / 9; background: #1e1e1e; overflow: hidden; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; }
  .subtitle {
    position: absolute; left: 50%; bottom: 5%; transform: translateX(-50%);
    max-width: 90%; padding: 0.2em 0.6em; border-radius: 4px;
    background: rgba(0, 0, 0, 0.7); color: #fff; text-align: center;
    font-size: clamp(12px, 3.2cqh, 30px); line-height: 1.3; pointer-events: none;
  }
  .subtitle:empty { display: none; }
  .stage { container-type: size; }
  .bar { display: flex; align-items: center; gap: 8px; padding: 6px 8px; background: #2b2b2b; color: #ddd; font-size: 13px; }
  .bar[hidden] { display: none; }
  button, select { background: #3a3a3a; color: #eee; border: 0; border-radius: 4px; padding: 4px 8px; font: inherit; cursor: pointer; }
  button:hover, select:hover { background: #4a4a4a; }
  input[type=range] { flex: 1; min-width: 60px; accent-color: #60a5fa; }
  .time { font-variant-numeric: tabular-nums; white-space: nowrap; }
  .error {
    position: absolute; inset: 0; overflow: auto; padding: 16px; background: #fff5f5; color: #9b1c1c;
    font: 13px/1.5 ui-monospace, monospace; white-space: pre-wrap;
  }
  .loading { position: absolute; inset: 0; display: grid; place-items: center; color: #aaa; font-size: 14px; }
`;

class DidactPlayer extends HTMLElement {
  static observedAttributes = ["src", "lang"];

  constructor() {
    super();
    const root = this.attachShadow({ mode: "open" });
    root.innerHTML = `
      <style>${STYLE}</style>
      <div class="stage" part="stage">
        <canvas></canvas>
        <div class="subtitle" part="subtitle"></div>
        <div class="loading">Chargement…</div>
      </div>
      <div class="bar" part="controls">
        <button class="restart" title="Revenir au début">⏮</button>
        <button class="play" title="Lecture / pause (Espace)">▶</button>
        <span class="time">0.00 / 0.00 s</span>
        <select class="lang" title="Langue des sous-titres" hidden></select>
        <input class="seek" type="range" min="0" max="1" step="0.01" value="0" title="Barre de lecture">
      </div>`;
    this.$ = (sel) => root.querySelector(sel);
    this.canvas = this.$("canvas");
    this.ctx = this.canvas.getContext("2d");
    this.player = null;
    this.t = 0;
    this.playing = false;
    this.lastFrame = null;
    this.drawn = null; // (t, largeur, hauteur, langue) de la dernière image

    this.$(".play").addEventListener("click", () => this.toggle());
    this.$(".restart").addEventListener("click", () => this.seek(0));
    this.$(".seek").addEventListener("input", (e) => this.seek(parseFloat(e.target.value)));
    this.$(".lang").addEventListener("change", (e) => this.setLang(e.target.value));
    this.addEventListener("keydown", (e) => {
      if (e.code === "Space") { e.preventDefault(); this.toggle(); }
      if (e.code === "ArrowRight") this.seek(this.t + 1);
      if (e.code === "ArrowLeft") this.seek(this.t - 1);
    });
    new ResizeObserver(() => { this.drawn = null; }).observe(this.$(".stage"));
  }

  connectedCallback() {
    if (!this.hasAttribute("tabindex")) this.tabIndex = 0; // pour le clavier (Espace, flèches)
    this.load();
    this.raf = requestAnimationFrame((now) => this.tick(now));
  }

  disconnectedCallback() {
    cancelAnimationFrame(this.raf);
    this.stopAudio();
  }

  attributeChangedCallback(name, oldValue, value) {
    if (!this.isConnected || oldValue === value) return;
    if (name === "src") this.load();
    if (name === "lang") { this.setLang(value); this.syncLang(); }
  }

  /** Charge (ou recharge) le fichier .dct */
  async load() {
    const token = (this.loadToken = {});
    this.showMessage("loading", "Chargement…");
    let source;
    const src = this.getAttribute("src");
    try {
      this.engine = await initDidact();
      if (src) {
        const response = await fetch(src);
        if (!response.ok) throw new Error(`Impossible de charger ${src} (HTTP ${response.status})`);
        source = await response.text();
      } else {
        const script = this.querySelector('script[type="text/didact"]');
        source = script?.textContent ?? "";
        if (script?.dataset.encoding === "base64") {
          source = new TextDecoder().decode(Uint8Array.from(atob(source.trim()), (c) => c.charCodeAt(0)));
        }
      }
    } catch (e) {
      return this.showMessage("error", String(e.message ?? e));
    }
    if (token !== this.loadToken) return; // un autre chargement a commencé entre-temps

    try {
      this.stopAudio();
      this.player?.free();
      this.player = new this.engine.Player(source);
      this.setupAudio(src);
      this.setupVoice(src);
    } catch (e) {
      this.player = null;
      return this.showMessage("error", "Erreurs dans le fichier Didact :\n\n" + (e.message ?? e));
    }
    this.showMessage(null);

    const ratio = this.player.aspectRatio();
    this.$(".stage").style.aspectRatio = ratio ? String(ratio) : "16 / 9";
    this.currentLang = this.getAttribute("lang") || this.player.defaultLanguage();
    this.duration = this.player.realDuration(this.currentLang);
    this.$(".seek").max = String(this.duration || 0.01);
    this.syncLang();
    this.$(".bar").hidden = this.getAttribute("controls") === "false";
    this.t = 0;
    this.drawn = null;
    this.playing = this.getAttribute("autoplay") !== "false";
    this.updateControls();
  }

  /** Crée un <audio> par piste. `src` : adresse du .dct (null si écrit dans la page) */
  setupAudio(src) {
    const base = new URL(src || ".", document.baseURI);
    const embedded = globalThis.__DIDACT_AUDIO__ || {}; // page autonome : fichiers inclus
    this.tracks = JSON.parse(this.player.audioSegments()).map((seg) => {
      const el = new Audio(embedded[seg.file] || new URL(seg.file, base).href);
      el.preload = "auto";
      el.addEventListener("error", () => console.warn("Didact : audio introuvable :", seg.file));
      return { ...seg, el };
    });
  }

  /** Voix : dossier cours_voix/ à côté du .dct (ou inclus dans une page autonome) */
  async setupVoice(src) {
    this.voiceFiles = null;
    const token = this.loadToken;
    let dir, index;
    if (globalThis.__DIDACT_VOICE_DIR__) {
      dir = globalThis.__DIDACT_VOICE_DIR__;
      index = globalThis.__DIDACT_VOICE_INDEX__ || "";
    } else if (src) {
      const name = new URL(src, document.baseURI).pathname.split("/").pop().replace(/\.dct$/i, "");
      dir = name + "_voix";
      try {
        const res = await fetch(new URL(dir + "/index.txt", new URL(src, document.baseURI)));
        if (!res.ok) return; // pas de voix générées : rien à faire
        index = await res.text();
      } catch { return; }
    } else {
      return;
    }
    if (token !== this.loadToken || !this.player) return;
    // index.txt : « FR/….wav 3.42 » par ligne (fichier, durée lue)
    const files = index.split("\n").map((l) => l.trim())
      .filter((l) => l && !l.startsWith("#") && !l.startsWith("commande "))
      .map((l) => dir + "/" + l.split(/\s+/)[0]);
    this.voiceFiles = new Set(files);
    this.voiceDir = dir;
    this.voiceSrc = src;
    // Pauses de l'animation pour laisser finir les phrases trop longues
    const tl = this.timelineTime();
    this.player.setVoiceIndex(index);
    this.t = this.player.toReal(tl, this.currentLang);
    this.updateDuration();
    this.updateVoiceLang();
  }

  /** Temps de la timeline (celui du .dct) correspondant au temps réel this.t */
  timelineTime() {
    return this.player ? this.player.toTimeline(this.t, this.currentLang) : this.t;
  }

  /** Durée réelle (pauses comprises) dans la langue courante */
  updateDuration() {
    this.duration = this.player.realDuration(this.currentLang);
    this.$(".seek").max = String(this.duration || 0.01);
    this.updateControls();
  }

  /** Change de langue en restant au même endroit de la timeline */
  setLang(lang) {
    if (!this.player || lang === this.currentLang) { this.currentLang = lang; return; }
    const tl = this.timelineTime();
    this.currentLang = lang;
    this.t = this.player.toReal(tl, lang);
    this.updateDuration();
    this.drawn = null;
  }

  /** (Re)crée les pistes de voix de la langue courante */
  updateVoiceLang() {
    for (const tr of this.voiceTracks || []) { tr.el.pause(); tr.el.removeAttribute("src"); }
    this.voiceTracks = [];
    if (!this.player || !this.voiceFiles) return;
    const base = new URL(this.voiceSrc || ".", document.baseURI);
    const embedded = globalThis.__DIDACT_AUDIO__ || {};
    for (const seg of JSON.parse(this.player.voiceSegments(this.currentLang))) {
      const file = this.voiceDir + "/" + seg.file;
      if (!this.voiceFiles.has(file)) continue;
      const el = new Audio(embedded[file] || new URL(file, base).href);
      el.preload = "auto";
      this.voiceTracks.push({ file, volume: 1, start: seg.start, end: null, el });
    }
    this.voiceLang = this.currentLang;
  }

  stopAudio() {
    for (const tr of [...(this.tracks || []), ...(this.voiceTracks || [])]) {
      tr.el.pause();
      tr.el.removeAttribute("src");
    }
    this.tracks = [];
    this.voiceTracks = [];
  }

  /** Joue / arrête / recale chaque piste selon le temps courant */
  syncAudio() {
    if (this.voiceFiles && this.voiceLang !== this.currentLang) this.updateVoiceLang();
    for (const tr of [...(this.tracks || []), ...(this.voiceTracks || [])]) {
      // instants de la timeline → temps réel (pauses laissées aux voix)
      const start = this.player.toReal(tr.start, this.currentLang);
      const end = tr.end === null ? null : this.player.toReal(tr.end, this.currentLang);
      const active = this.playing && this.t >= start && (end === null || this.t < end);
      if (!active) {
        if (!tr.el.paused) tr.el.pause();
        continue;
      }
      const want = this.t - start;
      tr.el.volume = Math.min(1, Math.max(0, tr.volume));
      if (tr.el.paused) {
        if (want < (tr.el.duration || Infinity)) {
          tr.el.currentTime = want;
          tr.el.play().catch(() => {}); // bloqué tant que le visiteur n'a pas cliqué
        }
      } else if (Math.abs(tr.el.currentTime - want) > 0.3) {
        tr.el.currentTime = want; // saut dans la barre de lecture
      }
    }
  }

  /** Liste des langues dans le menu (seulement s'il y en a plusieurs) */
  syncLang() {
    if (!this.player) return;
    const select = this.$(".lang");
    const langs = this.player.languages();
    select.hidden = langs.length < 2;
    select.innerHTML = langs.map((l) => `<option value="${l}">${l}</option>`).join("");
    if (langs.includes(this.currentLang)) select.value = this.currentLang;
  }

  showMessage(kind, text) {
    this.$(".loading")?.remove();
    this.$(".error")?.remove();
    if (!kind) return;
    const div = document.createElement("div");
    div.className = kind;
    div.textContent = text;
    this.$(".stage").appendChild(div);
  }

  toggle() {
    if (!this.player) return;
    if (!this.playing && this.t >= this.duration) this.t = 0; // relancer une fois à la fin
    this.playing = !this.playing;
    this.updateControls();
  }

  seek(t) {
    if (!this.player) return;
    this.t = Math.min(Math.max(t, 0), this.duration);
    this.updateControls();
  }

  updateControls() {
    this.$(".play").textContent = this.playing ? "⏸" : "▶";
    this.$(".seek").value = String(this.t);
    this.$(".time").textContent = `${this.t.toFixed(2)} / ${(this.duration ?? 0).toFixed(2)} s`;
  }

  /** Boucle d'animation : avance le temps et redessine si nécessaire */
  tick(now) {
    this.raf = requestAnimationFrame((n) => this.tick(n));
    const dt = this.lastFrame == null ? 0 : (now - this.lastFrame) / 1000;
    this.lastFrame = now;
    if (!this.player) return;

    if (this.playing) {
      this.t += Math.min(dt, 0.1); // pas de saut si l'onglet était en arrière-plan
      if (this.t >= this.duration) {
        this.t = this.duration;
        this.playing = false;
      }
      this.updateControls();
    }
    this.syncAudio();
    this.draw();
  }

  draw() {
    const stage = this.$(".stage");
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(stage.clientWidth * dpr));
    const h = Math.max(1, Math.round(stage.clientHeight * dpr));
    const key = `${this.t}|${w}|${h}|${this.currentLang}`;
    if (this.drawn === key) return; // rien n'a changé (en pause)
    this.drawn = key;

    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    const tl = this.timelineTime(); // this.t est le temps réel (pauses comprises)
    const pixels = this.player.render(tl, w, h);
    const image = new ImageData(new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.length), w, h);
    this.ctx.putImageData(image, 0, 0);
    this.$(".subtitle").textContent = this.player.comment(tl, this.currentLang) ?? "";
    for (const warning of this.player.takeWarnings()) console.warn("Didact :", warning);
  }
}

customElements.define("didact-player", DidactPlayer);
