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
//  Le moteur (pkg/didact_bg.wasm) est chargé une seule fois pour toute la page.
// ─────────────────────────────────────────────

let ready = null;

/**
 * Charge le moteur WebAssembly (une seule fois, partagé par tous les lecteurs).
 * Renvoie le module : `const { Player } = await initDidact();`
 */
export function initDidact() {
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
    this.$(".lang").addEventListener("change", (e) => { this.currentLang = e.target.value; this.drawn = null; });
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
  }

  attributeChangedCallback(name, oldValue, value) {
    if (!this.isConnected || oldValue === value) return;
    if (name === "src") this.load();
    if (name === "lang") { this.currentLang = value; this.syncLang(); this.drawn = null; }
  }

  /** Charge (ou recharge) le fichier .dct */
  async load() {
    const token = (this.loadToken = {});
    this.showMessage("loading", "Chargement…");
    let source;
    try {
      this.engine = await initDidact();
      const src = this.getAttribute("src");
      if (src) {
        const response = await fetch(src);
        if (!response.ok) throw new Error(`Impossible de charger ${src} (HTTP ${response.status})`);
        source = await response.text();
      } else {
        source = this.querySelector('script[type="text/didact"]')?.textContent ?? "";
      }
    } catch (e) {
      return this.showMessage("error", String(e.message ?? e));
    }
    if (token !== this.loadToken) return; // un autre chargement a commencé entre-temps

    try {
      this.player?.free();
      this.player = new this.engine.Player(source);
    } catch (e) {
      this.player = null;
      return this.showMessage("error", "Erreurs dans le fichier Didact :\n\n" + (e.message ?? e));
    }
    this.showMessage(null);

    const ratio = this.player.aspectRatio();
    this.$(".stage").style.aspectRatio = ratio ? String(ratio) : "16 / 9";
    this.duration = this.player.duration();
    this.$(".seek").max = String(this.duration || 0.01);
    this.currentLang = this.getAttribute("lang") || this.player.defaultLanguage();
    this.syncLang();
    this.$(".bar").hidden = this.getAttribute("controls") === "false";
    this.t = 0;
    this.drawn = null;
    this.playing = this.getAttribute("autoplay") !== "false";
    this.updateControls();
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
    const pixels = this.player.render(this.t, w, h);
    const image = new ImageData(new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.length), w, h);
    this.ctx.putImageData(image, 0, 0);
    this.$(".subtitle").textContent = this.player.comment(this.t, this.currentLang) ?? "";
    for (const warning of this.player.takeWarnings()) console.warn("Didact :", warning);
  }
}

customElements.define("didact-player", DidactPlayer);
