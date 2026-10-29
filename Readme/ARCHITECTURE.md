# Didact — Architecture technique

**Version : 0.2**

Document technique pour les contributeurs et développeurs travaillant sur le compilateur Didact lui-même. **Ce n'est pas la documentation du langage** (voir `SPEC.md`).

---

## Table des matières

1. [Vue d'ensemble](#1-vue-densemble)
2. [Choix du langage](#2-choix-du-langage)
3. [Le modèle compilateur = renderer](#3-le-modèle-compilateur--renderer)
4. [Pipeline de compilation](#4-pipeline-de-compilation)
5. [Modules du projet](#5-modules-du-projet)
6. [Les deux cibles : natif et WebAssembly](#6-les-deux-cibles--natif-et-webassembly)
7. [Intégration des moteurs externes](#7-intégration-des-moteurs-externes)
8. [Modèle de stockage web](#8-modèle-de-stockage-web)
9. [Choix de bibliothèques](#9-choix-de-bibliothèques)

---

## 1. Vue d'ensemble

Le compilateur Didact est un **programme unique** qui lit un fichier `.dct` et affiche directement le rendu visuel. Il n'y a **pas** de séparation entre "compilateur" et "renderer" — c'est le même binaire.

```
┌────────────────────────────────────────┐
│         Programme Didact (Rust)        │
│                                        │
│   lecteur ↔ parser ↔ validateur ↔ rendu │
│                                        │
└────────────────────────────────────────┘
            ↑                  ↓
       fichier.dct      affichage visuel
```

Selon le contexte de compilation, le même code produit :
- Un **exécutable natif** qui ouvre une fenêtre (comme matplotlib)
- Un **module WebAssembly** qui s'exécute dans le navigateur

---

## 2. Choix du langage

Le compilateur est écrit en **Rust** pour les raisons suivantes :

| Critère | Pourquoi Rust |
|---|---|
| Performances | Comparable à C, sans coût d'exécution caché |
| Sécurité mémoire | Pas de fuites, pas de segfaults, garanti par le compilateur |
| Compilation vers WebAssembly | Première classe, mature, performante |
| Distribution | Un seul exécutable statique, rien à installer côté utilisateur |
| Typage | Strict, expressif, attrape beaucoup de bugs à la compilation |
| Écosystème | Crates matures pour parsing, rendu graphique, WASM |

**Alternatives rejetées :**
- Python — trop lent pour le rendu temps réel, mauvaise distribution
- JavaScript/Node — pas adapté au rendu natif, écosystème instable
- C++ — performances ok mais sécurité mémoire fragile, distribution complexe
- Go — pas de WASM mature, GC pause pendant les animations

---

## 3. Le modèle compilateur = renderer

### 3.1 Différence avec un compilateur classique

Un compilateur classique (gcc, rustc) :
```
code source → binaire exécutable → exécution
```

Le compilateur Didact :
```
fichier.dct → rendu visuel direct
```

Il n'y a pas d'étape intermédiaire de "binaire compilé". Le rendu se fait à la volée.

### 3.2 Conséquences

**Avantages :**
- Pas besoin de "compiler puis exécuter", une seule commande
- Modifications instantanées (re-run = re-rendu)
- Pas de fichier intermédiaire à gérer
- Plus proche d'un interpréteur que d'un compilateur traditionnel

**Compromis :**
- Le temps de parsing/validation s'ajoute à chaque exécution (négligeable pour des fichiers de quelques ko)
- Pas de "cache" de rendu (à étudier plus tard si nécessaire)

### 3.3 Exports comme cas particuliers

Les exports (MP4, HTML autonome) sont des **fonctionnalités secondaires** :

```
didact cours.dct                  →  ouvre une fenêtre (usage principal)
didact cours.dct --export mp4     →  enregistre une vidéo
didact cours.dct --export html    →  produit un HTML autonome
```

L'export n'est pas le mode normal d'utilisation. C'est une fonctionnalité opt-in.

---

## 4. Pipeline de compilation

```
                fichier.dct
                     │
                     ▼
              ┌─────────────┐
              │   Lexer     │  src/lexer.rs
              │             │  Texte → tokens
              └─────────────┘
                     │ Vec<Token>
                     ▼
              ┌─────────────┐
              │   Parser    │  src/parser.rs + src/ast.rs
              │             │  Tokens → AST
              └─────────────┘
                     │ Document (AST)
                     ▼
              ┌─────────────┐
              │  Validateur │  src/validator.rs
              │             │  Vérifie cohérence sémantique
              └─────────────┘
                     │ Document validé
                     ▼
              ┌─────────────┐
              │  Résolveur  │  src/resolver.rs          
              │             │  Résout styles, refs, hérédité
              └─────────────┘
                     │ Document résolu
                     ▼
              ┌─────────────┐
              │   Moteur    │  src/runtime/            
              │  d'exécution│  Calcule l'état à chaque frame
              └─────────────┘
                     │ Frame courante
                     ▼
              ┌─────────────┐
              │  Renderer   │  src/renderer/           
              │             │  Dessine la frame
              └─────────────┘
                     │
                     ▼
                 affichage
```

### 4.1 Étapes détaillées

**Lexer (`src/lexer.rs`) — ✅ implémenté**
- Lit le texte caractère par caractère
- Produit un `Vec<Token>`
- Gère les identifiants, nombres, strings, symboles, pourcentages, commentaires
- Garde le numéro de ligne de chaque token, propagé dans l'AST pour des messages d'erreur `ligne N : ...`

**Parser (`src/parser.rs` + `src/ast.rs`) — ✅ implémenté**
- Recursive descent parser
- Produit une struct `Document` contenant Config, Styles, Figures, Timeline

**Validateur (`src/validator.rs`) — ✅ implémenté**
- Vérifie l'unicité des noms
- Vérifie que chaque référence pointe vers un élément défini plus haut (pas de référence en avant, donc pas de cycle possible)
- Vérifie l'unicité des timestamps
- Vérifie que les types de figures sont valides
- Vérifie la cohérence des langues (nombre de traductions)
- Vérifie que `destroy` n'est pas appelé sur une figure encore référencée
- Vérifie l'ordre des sections
- Produit des erreurs claires avec localisation

**Résolveur (`src/resolver.rs`) — ✅ implémenté**
- Applique les styles aux figures (par référence)
- Résout l'héritage des styles (chaîne `extends`)
- Calcule les valeurs effectives pour chaque propriété
- Trie les figures par `layer`

**Moteur d'exécution (`src/runtime/`) — ✅ implémenté**
- `animation.rs` : lecture des appels (`move(to=..., 2s, ease=...)`), easing, progression 0 → 1 d'une animation
- `timeline.rs` : planifie les animations par figure au chargement (enchaînements, sorties animées)
- Au chargement, rejoue la timeline une fois et garde un instantané de l'état après chaque bloc
- État à l'instant t = instantané du dernier bloc ≤ t (recherche dichotomique) : on peut avancer, reculer ou sauter librement
- Maintient l'état courant : quelles figures sont visibles, leurs positions, leurs animations en cours
- À chaque frame, calcule le nouvel état en fonction du timestamp
- Exécute les commandes d'un bloc dans l'ordre fixe `destroy` → `end` → `start` → `anim` → `comment`
- Évalue les références dynamiques (`center()`, `angle()`, `offset()`) — fait par le renderer : à chaque frame, toutes les figures sont calculées dans l'ordre de définition (formes → animations → géométrie publiée), ce qui suffit puisqu'il n'y a pas de référence en avant
- Interpole les animations selon les courbes d'easing

**Renderer (`src/renderer/`) — ✅ tous les types sauf `raw.matplotlib`**
- `geometry.rs` : figure → formes en pixels (chemins, remplissage, trait), indépendant de la bibliothèque de dessin
- `skia.rs` : backend tiny-skia, dessine les formes dans une image en mémoire (sur processeur)
- `typeset.rs` : moteur de composition — LaTeX → Typst (`mitex`) → Typst compose → SVG (`typst-svg`) → contours (`usvg`), avec cache
- `text.rs` : point d'entrée simple vers `typeset` pour le type `text`
- `expr.rs` : évaluateur des expressions de `plot` (lu une fois, évalué pour chaque x)
- `mod.rs` : trait `RenderBackend` + `render_frame` (dessine une frame, liste les figures non dessinables)
- Délègue à LaTeX/Matplotlib pour les contenus complexes (à faire)

**Fenêtre (`src/output/window.rs`) — ✅ implémenté**
- eframe/egui : affiche l'image produite par tiny-skia, au ratio de `window`, à la taille réelle en pixels
- Contrôles : lecture/pause, retour au début, barre de lecture, temps, langue ; sous-titres par-dessus
- Clavier : Espace (lecture/pause), ← / → (±1 s), Début

---

## 5. Modules du projet

### 5.1 Structure cible

```
didact/
├── Cargo.toml
├── README.md
├── SPEC.md
├── ARCHITECTURE.md
├── DECISIONS.md
├── exemple.dct
└── src/
    ├── main.rs              # Point d'entrée CLI
    │
    ├── lexer.rs             # Tokenisation
    ├── ast.rs               # Structures de données (AST)
    ├── parser.rs            # Parser récursif descendant
    │
    ├── validator.rs         # Validation sémantique
    ├── resolver.rs          # Résolution des styles et références
    │
    ├── runtime/
    │   ├── mod.rs           # Moteur d'exécution principal
    │   ├── timeline.rs      # Gestion de la timeline
    │   ├── animation.rs     # Système d'animations + easing
    │   └── references.rs    # Évaluation des refs dynamiques
    │
    ├── renderer/
    │   ├── mod.rs           # Orchestration du rendu
    │   ├── geometry.rs      # Dessin des figures géométriques
    │   ├── text.rs          # Rendu texte simple
    │   ├── latex.rs         # Intégration LaTeX
    │   ├── math_plot.rs     # Rendu des graphes
    │   └── audio.rs         # Lecture audio
    │
    └── output/
        ├── window.rs        # Affichage fenêtre desktop
        ├── wasm.rs          # Export WebAssembly
        ├── mp4.rs           # Export MP4 (optionnel)
        └── html.rs          # Export HTML autonome (optionnel)
```

### 5.2 Responsabilités de chaque module

| Module | Responsabilité unique |
|---|---|
| `lexer` | Transformer le texte en tokens |
| `parser` | Transformer les tokens en AST |
| `ast` | Définir les structures de données du langage |
| `validator` | Garantir que l'AST est sémantiquement valide |
| `resolver` | Aplatir l'héritage de styles, résoudre les références par nom |
| `runtime/timeline` | Maintenir l'état courant en fonction du temps |
| `runtime/animation` | Interpoler les valeurs animées |
| `runtime/references` | Calculer les valeurs dynamiques à chaque frame |
| `renderer/*` | Convertir l'état en pixels |
| `output/*` | Afficher ou exporter les pixels |

**Principe :** chaque module a **une** responsabilité claire. Pas de logique métier qui traverse plusieurs modules.

---

## 6. Les deux cibles : natif et WebAssembly

### 6.1 Mode natif (desktop)

```bash
cargo build --release
./target/release/didact cours.dct
```

Produit un exécutable Windows/Mac/Linux. Quand on le lance :
1. Le programme lit `cours.dct`
2. Une fenêtre s'ouvre
3. L'animation se joue dans la fenêtre
4. L'utilisateur peut pause, avancer, reculer
5. Fermer la fenêtre quitte le programme

**Bibliothèques :** `tiny-skia` (dessin vectoriel de la scène dans une image) + `eframe`/`egui` (fenêtre, contrôles). Voir DECISIONS.md.

### 6.2 Mode WebAssembly (web)

```
.\web\build.ps1     # cargo build --lib --target wasm32-unknown-unknown + wasm-bindgen
```

Produit `web/pkg/didact_bg.wasm` (le moteur) et `web/pkg/didact.js` (le lien avec JavaScript). Le lecteur `web/didact.js` définit la balise :

```html
<script type="module" src="didact.js"></script>
<didact-player src="cours.dct" lang="FR"></didact-player>
```

- `src/web.rs` expose `Player` (wasm-bindgen) : charger un source, durée, langues, `render(t, w, h)` → pixels RGBA, sous-titre, avertissements.
- `web/didact.js` : boucle `requestAnimationFrame`, canvas à la taille réelle (densité d'écran ≤ 2), ne redessine pas en pause, contrôles et sous-titres en HTML.
- Guide complet : `Readme/WEB.md`.

### 6.3 Code partagé entre les deux cibles

**Identique dans les deux modes :**
- Lexer
- Parser
- Validator
- Resolver
- Runtime (moteur d'exécution)
- Logique de rendu (calcul des formes à dessiner)

**Spécifique à chaque cible :**
- Mode natif : image tiny-skia affichée dans une fenêtre egui
- Mode WASM : image tiny-skia copiée dans un `<canvas>` par `web/didact.js`

L'abstraction se fait via un trait `RenderBackend` :

```rust
// src/renderer/mod.rs
pub trait RenderBackend {
    fn clear(&mut self, color: Rgba);
    fn draw(&mut self, shape: &Shape); // chemin + remplissage + trait, en pixels
}
```

Les figures sont converties en `Shape` par `renderer/geometry.rs`, indépendamment du backend.

Implémentations :
- `SkiaBackend` (tiny-skia) : natif, export PNG, et WASM (image copiée dans un canvas) ✅
- un backend GPU (wgpu) seulement si les performances l'exigent un jour

---

## 7. Intégration des moteurs externes

### 7.1 LaTeX

**Cas d'usage :** rendu des équations et tableaux complexes.

**Stratégie retenue (identique en natif et en WASM) :**

```
LaTeX
    ↓  mitex::convert_math / convert_text
code Typst
    ↓  typst::compile (polices Latin Modern embarquées,
    ↓  définitions MiTeX dans assets/typst/mitex)
page Typst
    ↓  typst_svg::svg
SVG
    ↓  usvg (lecture, transformations appliquées)
chemins vectoriels → mêmes `Shape` que les autres figures
```

Le résultat est mis en cache (une composition par texte). Le monde Typst est minimal : pas d'accès aux fichiers ni aux packages.

### 7.2 Matplotlib

**Cas d'usage :** rendu des graphes complexes via `raw.matplotlib`.

**Stratégie cible :**
- Mode natif : appeler Python+Matplotlib en sous-processus
- Mode WASM : non supporté pour `raw.matplotlib` (sandbox), utiliser les types natifs

**Sécurité :** `raw.matplotlib` exécute du code Python arbitraire. Désactivé par défaut, activable avec `--allow-raw`.

### 7.3 Types natifs vs escape hatches

```
Types natifs Didact (plot, scatter, histogram)
    →  rendu par le code Rust de Didact
    →  fonctionne dans les deux modes (natif et WASM)
    →  rapide, sûr

Escape hatches (raw.latex, raw.matplotlib)
    →  délégué à des moteurs externes
    →  fonctionne surtout en mode natif
    →  flexibilité maximale mais dépendances externes
```

L'utilisateur final est encouragé à utiliser les types natifs. Les escape hatches sont pour les cas avancés.

---

## 8. Modèle de stockage web

### 8.1 Principe : rendu côté client

Quand un fichier `.dct` est embarqué dans un site web, **le serveur ne fait que servir le texte brut**. Tout le rendu se passe chez le visiteur :

```
Serveur web
    └─ stocke  cours.dct   (5-15 ko de texte)

Navigateur du visiteur
    ├─ télécharge cours.dct
    ├─ charge le runtime Didact (WASM, mis en cache)
    └─ exécute le rendu localement
```

### 8.2 Comparaison avec la vidéo classique

| | Vidéo MP4 | Didact |
|---|---|---|
| Stockage serveur | 50-500 mo | 5-15 ko |
| Bande passante par visite | 50-500 mo | 15 ko + WASM (caché) |
| CPU serveur | Streaming | Aucun |
| Modification | Ré-encodage complet | Édition du texte |
| Versionning | Diff binaire inutile | Diff texte lisible |

---



---

## Annexe : flux de données complet (futur)

Quand l'utilisateur lance `didact cours.dct`, voici ce qui se passe :

```
1. main.rs lit les arguments CLI
   └─ détecte le fichier .dct à ouvrir

2. main.rs lit le fichier complet en mémoire
   └─ String UTF-8

3. lexer.rs tokenise
   └─ Vec<Token>

4. parser.rs construit l'AST
   └─ struct Document

5. validator.rs vérifie la cohérence
   └─ Document validé ou erreurs explicites

6. resolver.rs aplatit les styles et résout les références
   └─ Document avec valeurs effectives

7. main.rs crée une fenêtre via eframe/egui
   └─ event loop démarre

8. À chaque frame (60 fps) :
   a. runtime.rs calcule l'état courant à partir du timestamp
   b. runtime.rs évalue les références dynamiques
   c. renderer.rs convertit l'état en commandes de dessin
   d. backend (tiny-skia) dessine dans une image, egui l'affiche
   e. Le frame est affiché

9. Si l'utilisateur ferme la fenêtre :
   └─ event loop termine
   └─ programme quitte proprement
```