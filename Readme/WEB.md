# Didact sur le web

Didact se compile en **WebAssembly** : le même moteur que le programme desktop tourne dans le navigateur. Une page web n'a besoin que du fichier `.dct` (quelques ko de texte) ; le rendu se fait chez le visiteur.

```html
<script type="module" src="didact.js"></script>

<didact-player src="cours.dct" lang="FR"></didact-player>
```

---

## 1. Compiler le moteur web

### Prérequis (une seule fois)

Rust doit être installé avec **rustup** (https://rustup.rs) : c'est lui qui ajoute la cible WebAssembly. Si `rustup` n'est pas reconnu, Rust a été installé autrement (installeur MSI) : le désinstaller, puis installer rustup (`winget install Rustlang.Rustup` sous Windows) et rouvrir le terminal.

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
```

La version de `wasm-bindgen-cli` doit être **exactement** celle de `wasm-bindgen` dans `Cargo.toml` (`=0.2.129`). Si on change l'une, il faut changer l'autre.

### Compilation

Depuis la racine du projet :

```
.\web\build.ps1        # Windows (PowerShell)
./web/build.sh         # Linux / macOS
```

Résultat dans `web/pkg/` :

| Fichier | Rôle |
|---|---|
| `didact_bg.wasm` | le moteur compilé |
| `didact.js` | le code qui charge le moteur et expose `Player` à JavaScript |

`web/pkg/` est un résultat de compilation : il n'est pas versionné dans git.

### Tester en local

Le navigateur refuse de charger du WebAssembly depuis un fichier ouvert directement (`file://`). Il faut un petit serveur web, lancé depuis la racine du projet :

```
python -m http.server 8000
```

puis ouvrir http://localhost:8000/web/ : la page `web/index.html` montre deux lecteurs.

---

## 2. Mettre un lecteur dans une page

Copier à côté de la page :

```
ma-page.html
didact.js          ← web/didact.js
pkg/
  didact.js        ← web/pkg/didact.js
  didact_bg.wasm   ← web/pkg/didact_bg.wasm
cours.dct
```

Dans la page :

```html
<script type="module" src="didact.js"></script>
<didact-player src="cours.dct"></didact-player>
```

### Attributs

| Attribut | Effet | Défaut |
|---|---|---|
| `src` | fichier `.dct` à charger | — |
| `lang` | langue des sous-titres (`FR`, `EN`, ...) | `default_lang` du fichier |
| `autoplay` | `"false"` : ne démarre pas tout seul | démarre |
| `controls` | `"false"` : cache la barre de contrôles | visible |

Changer `src` ou `lang` en JavaScript recharge le lecteur ou change la langue :

```js
document.querySelector("didact-player").setAttribute("lang", "EN");
```

### Écrire le fichier directement dans la page

Sans `src`, le lecteur lit un `<script type="text/didact">` placé à l'intérieur :

```html
<didact-player>
  <script type="text/didact">
[figures]
deffigure titre =
    type    : text
    content : "Bonjour $\pi$"
    pos     : (50%, 50%)

[timeline]
0.000 => { start : [titre fadein(1s)] }
  </script>
</didact-player>
```

### Ce que voit le visiteur

- L'animation, au ratio de `window` (16:9 par défaut), qui s'adapte à la largeur de la page.
- La barre de contrôles : retour au début, lecture / pause, temps, langue (s'il y en a plusieurs), barre de lecture.
- Les sous-titres (`comment`) dans la langue choisie.
- Au clavier, une fois le lecteur sélectionné : Espace (lecture / pause), ← / → (± 1 s).
- Si le fichier contient des erreurs : la liste des erreurs avec leurs numéros de ligne, à la place de l'animation.

Le moteur WebAssembly est chargé **une seule fois** par page, même avec plusieurs lecteurs.

---

## 3. API JavaScript

Pour aller plus loin que la balise (dessiner dans son propre canvas, exporter des images, ...) :

```js
import { initDidact } from "./didact.js";

const { Player } = await initDidact();
const player = new Player(texteDuFichier);   // exception si le fichier est invalide

player.duration();          // durée en secondes
player.languages();         // ["FR", "EN"]
player.defaultLanguage();   // "FR"
player.aspectRatio();       // 1.777… (undefined si window : auto)

const pixels = player.render(t, largeur, hauteur);   // Uint8Array RGBA
context.putImageData(new ImageData(new Uint8ClampedArray(pixels.buffer), largeur, hauteur), 0, 0);

player.comment(t, "EN");    // sous-titre à l'instant t, ou undefined
player.takeWarnings();      // avertissements nouveaux (figure non dessinable, ...)
player.free();              // libère la mémoire quand on n'en a plus besoin
```

---

## 4. Héberger

- Servir `didact_bg.wasm` avec le type `application/wasm` (c'est le cas de la plupart des hébergeurs : GitHub Pages, Netlify, ...).
- Le moteur est lourd (37 Mo, le compositeur LaTeX en représente la plus grande partie) mais il est mis en cache par le navigateur : seule la première visite le télécharge. Les fichiers `.dct` restent de quelques ko. Pistes pour le réduire : voir « Améliorations prévues » dans `DECISIONS.md`.

## 5. Limites actuelles

- L'audio (`start : [audio ...]`) n'est pas encore joué (Phase 5).
- `raw.matplotlib` n'existe pas sur le web (pas de Python dans le navigateur).
