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

La compilation utilise le profil `web` de `Cargo.toml` (optimisation sur tout le programme : plus long à compiler, un peu plus petit) et enlève les noms de fonctions, qui ne servent qu'à déboguer (7 Mo). `.\web\build.ps1 -Opt` (ou `OPT=1 ./web/build.sh`) passe en plus `wasm-opt -O2` s'il est installé.

**Taille** (mesurée sur le moteur du 29 octobre) : 37,2 Mo → 29,9 Mo sans les noms. Ce qui compte pour un visiteur, c'est la taille transférée : les hébergeurs compressent le fichier (gzip), il fait alors environ 10,4 Mo, et `wasm-opt` n'y change presque rien. Le reste vient surtout du compositeur LaTeX (Typst), qui embarque des données qu'on ne peut pas désactiver. Le navigateur garde le moteur en cache : seule la première visite paie ce téléchargement.

**À éviter : `wasm-opt -Oz`** (le plus petit) : 27,1 Mo, mais le rendu est environ trois fois plus lent.

**Le moteur web ne se met pas à jour tout seul** : après chaque modification du code Rust, relancer `.\web\build.ps1`, sinon la page web et l'export `--export html` utilisent l'ancien moteur.

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

## 4. Page autonome (un seul fichier)

```powershell
cargo run --release -- cours.dct --export html     # → cours.html
```

Produit un fichier `.html` qui contient tout (le cours, le lecteur et le moteur). Il s'ouvre d'un double-clic, sans serveur ni connexion, et peut s'envoyer tel quel. Il pèse environ un tiers du moteur (compressé). Le moteur `web/pkg` doit avoir été compilé avant (section 1) ; sinon : `--engine <dossier>`.

## 5. Héberger

- Servir `didact_bg.wasm` avec le type `application/wasm` (c'est le cas de la plupart des hébergeurs : GitHub Pages, Netlify, ...).
- Le moteur est lourd (37 Mo, le compositeur LaTeX en représente la plus grande partie) mais il est mis en cache par le navigateur : seule la première visite le télécharge. Les fichiers `.dct` restent de quelques ko. Pistes pour le réduire : voir « Améliorations prévues » dans `DECISIONS.md`.

## 6. Limites actuelles

- Voix : si `didact cours.dct --voice` a été lancé, mettre le dossier `cours_voix/` à côté du `.dct` sur le site ; la voix suit la langue choisie.
- Son : les navigateurs bloquent souvent le son tant que le visiteur n'a pas cliqué ; il démarre alors au premier clic sur ▶. Les fichiers audio sont cherchés à côté du `.dct` (ou de la page pour un fichier écrit dans la page) ; la page autonome (`--export html`) les inclut.
- Images (`type : image`) : le lecteur télécharge les fichiers PNG et SVG à côté du `.dct` avant la première image ; la page autonome les inclut aussi. Les polices grasse et italique et le dessin des SVG (resvg) ajoutent environ 1 Mo au moteur (moins compressé).
- `raw.matplotlib` n'existe pas sur le web (pas de Python dans le navigateur).
