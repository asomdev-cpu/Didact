# Didact — Journal de design et avancement

**Version : 0.2**

Ce document garde la trace des décisions importantes prises pendant le développement, avec leur raisonnement. Permet d'éviter de remettre en question des choix déjà tranchés, et de comprendre le **pourquoi** derrière le **quoi**.

---

## Table des matières

1. [État actuel du projet](#1-état-actuel-du-projet)
2. [Bugs connus](#2-bugs-connus)
3. [Roadmap par ordre de priorité](#3-roadmap-par-ordre-de-priorité)
4. [Décisions de design](#4-décisions-de-design)
5. [Idées reportées](#5-idées-reportées)
6. [Idées rejetées (et pourquoi)](#6-idées-rejetées-et-pourquoi)

---

## 1. État actuel du projet

### Composants implémentés

| Composant | Statut | Fichier |
|---|---|---|
| Lexer | ✅ Fonctionnel | `src/lexer.rs` |
| AST (structures) | ✅ Fonctionnel | `src/ast.rs` |
| Parser | ✅ Fonctionnel | `src/parser.rs` |
| Validateur | ✅ Fonctionnel (15 tests) | `src/validator.rs` |
| Résolveur | ✅ Fonctionnel (6 tests) | `src/resolver.rs` |
| Runtime | ✅ Visibilité, commentaires, audio, animations, easing | `src/runtime/` |
| Renderer | ✅ tous les types de la spec sauf `raw.matplotlib` | `src/renderer/` |
| Output natif | ✅ Fenêtre + export PNG d'une frame | `src/output/window.rs` |
| Output WASM | ✅ Lecteur `<didact-player>` fonctionnel dans le navigateur | `src/web.rs`, `web/` |

### Métriques

- Versions de la spec : v0.1 (initiale), v0.2 (actuelle, plus détaillée et restructurée)
- Licence : MIT
- Plateforme cible : Windows, Mac, Linux (natif) + Navigateur (WASM)
- Langage : Rust

---

## 2. Bugs connus

Aucun bug connu.

- ~~`end : [audio fichier.mp3]` rejeté par le validateur~~ — corrigé.

- ~~Parser : `defstyle`/`deffigure`/`defgroup` non reconnus~~ — corrigé, `exemple.dct` se parse entièrement.


## 3. Roadmap par ordre de priorité

### Phase 1 — Faire tourner un rendu basique

L'objectif est d'arriver à un `cargo run -- exemple.dct` qui ouvre une fenêtre et affiche **quelque chose** (même si imparfait).

1. ~~**Corriger le bug du parser** (defstyle/deffigure/defgroup)~~ ✅
2. ~~**Validateur minimal** — vérifier les références de figures et de styles~~ ✅
3. ~~**Résolveur de styles** — aplatir l'héritage~~ ✅
4. ~~**Runtime timeline simple** — start/end sans animations~~ ✅
5. ~~**Renderer minimal** — lignes, cercles, rectangles uniquement~~ ✅ (+ polygon, arc, star)
6. ~~**Affichage fenêtre** — via `egui` ou `winit + wgpu`~~ ✅ (egui + tiny-skia)

### Phase 2 — Animations et temps réel

7. ~~**Système d'animations** — fadein, move, color~~ ✅ (+ fadeout, grow, shrink, scale, rotate)
8. ~~**Easing functions** — linear, ease-in-out, etc.~~ ✅ (smooth, linear, ease_in, ease_out)
9. ~~**Références dynamiques** — center(), angle(), offset()~~ ✅
10. ~~**Contrôles utilisateur** — pause, play, scrubber temporel~~ ✅ (fait avec la fenêtre)

### Phase 3 — Maths et graphes

11. ~~**Intégration LaTeX**~~ ✅ (LaTeX → Typst via MiTeX, composé par Typst ; text, equation, latex, table, raw.latex)
12. ~~**Type `axes`** — système de coordonnées mathématique~~ ✅
13. ~~**Type `plot`** — courbes de fonctions~~ ✅
14. ~~**Type `scatter` et `histogram`**~~ ✅ (+ texte, follow, auto_tangent : exemple.dct est jouable)

### Phase 4 — Embarquabilité

15. ~~**Compilation WebAssembly**~~ ✅ (`web/build.ps1`, nécessite rustup)
16. ~~**API JavaScript pour l'embarquement**~~ ✅ (`Player` + balise `<didact-player>`)
17. ~~**Documentation web**~~ ✅ (`Readme/WEB.md`, `web/index.html`)

### Phase 5 — Audio et multilingue

18. ~~**Lecture audio**~~ ✅ (fenêtre : symphonia + cpal, l'horloge du son guide l'image ; web : `<audio>` recalé ; inclus dans la page autonome)
19. ~~**Système de commentaires multilingues**~~ ✅ (fait en Phase 2 avec la timeline)
20. ~~**Sélection de langue au runtime**~~ ✅ (`--lang`, menu de la fenêtre et du lecteur web)

### Phase 6 — Exports

21. ~~**Export MP4** (avec ffmpeg)~~ ✅ (`--export mp4` : sous-titres incrustés, pistes audio mixées)
22. ~~**Export HTML autonome**~~ ✅ (`--export html` : un seul fichier, s'ouvre sans serveur)
23. ~~**Mode podcast (TTS)**~~ ✅ (`--voice` + `didact-voix.txt`, `--export audio`) — Décidé : si une phrase lue dure plus longtemps que l'écart avant le commentaire suivant, **l'animation attend** que la phrase soit finie (comme manim-voiceover). D'abord on avait choisi « avertir sans couper », mais les voix se superposaient et devenaient inaudibles. Décidé : Didact ne dépend d'aucun modèle de voix ; il appelle un programme de synthèse vocale externe choisi par l'utilisateur (comme ffmpeg), qui renvoie un fichier audio par commentaire. Pour commencer : un modèle open source léger qui tourne sur processeur (Piper ou Kokoro). Sortie : la voix est jouée partout (fenêtre, web, MP4, HTML) ; `--export audio` donne le son seul.

### Phase 7 — Écosystème

24. **Packages communautaires** — figures réutilisables
25. **Éditeur web style Overleaf**
26. **Site officiel et galerie d'exemples**

---

### Améliorations prévues

Améliorations techniques, sans impact sur le langage, à faire quand le besoin se présente.

#### Réduire la taille du moteur web

**Constat :** `web/pkg/didact_bg.wasm` pèse 37 Mo (essentiellement le compositeur LaTeX Typst). Sans importance en local, lourd pour la première visite d'un site.

**Pistes :**
1. **Compiler pour la taille** : régler le profil `release` de Cargo (`opt-level = "z"`, `lto = true`, `codegen-units = 1`). Estimation non mesurée : environ 15–20 Mo.
2. **Passer `wasm-opt`** (outil d'optimisation WebAssembly de Binaryen) en dernière étape de `web/build.ps1` (`wasm-opt -Oz`).
3. **Compression par le serveur** (gzip ou brotli) : le fichier tombe généralement au tiers ou moins pendant le transfert. Automatique chez la plupart des hébergeurs.

**Quand :** avant de mettre Didact en ligne.

**Mesures** (moteur du 29 octobre, 37,2 Mo : 19,1 Mo de code, 10,7 Mo de données dont 0,85 Mo de polices, 7,3 Mo de noms de fonctions pour le débogage) :

| Variante | Taille | Compressé (gzip) | Vitesse de rendu |
|---|---|---|---|
| actuel | 37,2 Mo | 11,2 Mo | référence |
| sans les noms de fonctions | 29,9 Mo | 10,4 Mo | identique |
| `wasm-opt -Oz` | 27,1 Mo | 10,4 Mo | environ 3 fois plus lent |

Les trois variantes donnent exactement la même image.

**Fait :** profil Cargo `web` (LTO, une seule unité de compilation, `panic = "abort"` : effet sur la taille non mesurable ici, la cible WebAssembly n'est pas installable dans cet environnement), noms de fonctions retirés par `wasm-bindgen`, `wasm-opt -O2` facultatif (`-Opt`). Écarté : `-Oz` et `opt-level = "z"` (trop lents pour un moteur de rendu).

**Ce qui reste :** environ 10 Mo transférés, à cause de Typst (bibliographies, coloration de code, dictionnaires de césure, impossibles à désactiver). Aller plus bas demande de changer de compositeur pour le web (voir « À décider »).

#### Vérifier le cadre et les chevauchements avant le rendu ✅ (`--check`)

**Constat :** le validateur ne regarde que la structure (noms, références, timestamps, `destroy`…). Une figure qui sort du cadre ou en recouvre une autre n'est visible qu'en regardant l'animation. Pour qu'une IA (ou un humain) puisse se corriger sans rendre d'image, il faut le détecter avant.

**Pistes :**
1. **Boîtes englobantes** : à partir de la géométrie déjà calculée par le renderer (positions, références dynamiques comme `center()`), calculer la boîte de chaque figure visible, sans dessiner de pixels.
2. **Échantillonner le temps** : au minimum à chaque bloc de la timeline et en fin de chaque animation ; des pas plus fins pendant les `move`/`follow`.
3. **Avertissement, pas erreur** : sortir du cadre ou se chevaucher peut être voulu (une figure qui entre par le bord, une étiquette sur une courbe). Comment l'auteur signale qu'un chevauchement est voulu reste à décider.

**Quand :** avant de faire générer des fichiers par une IA.

**Fait (`didact fichier.dct --check`, `src/check.rs`) :**
- Boîtes calculées par le renderer lui-même (une variante qui ne dessine rien et note la boîte de chaque figure), à chaque bloc, à chaque fin d'animation et toutes les 0,25 s. Les figures `pixels` ne calculent pas leurs couleurs pendant la vérification.
- **Hors du cadre :** toute figure, au-delà de 2 px (sur une image de 1080 px de haut). Signalé une fois par figure : premier instant, côtés, plus grand débordement.
- **Chevauchements :** seulement entre textes (`text`, `equation`, `latex`, `raw.latex`, `table`), quand plus de 10 % de la plus petite boîte est recouverte. Les autres chevauchements (étiquette sur une courbe, texte sur une figure) sont trop souvent voulus. Un texte qui apparaît ou disparaît (opacité < 95 %) est ignoré : un fondu enchaîné au même endroit est voulu.
- **Le signaler comme voulu (provisoire, à valider) :** propriété `allow : outside`, `allow : overlap` ou `allow : [outside, overlap]` sur la figure.
- Une figure qui ne peut pas être dessinée (propriété manquante découverte au rendu) est aussi signalée, comme erreur.

#### Sortie des erreurs exploitable par machine ✅ (`--check --format json`)

**Constat :** les erreurs du validateur sont du texte (`ligne N (t=…) : message`). Lisible pour un humain, mais un outil ou une IA doit les reparser.

**Pistes :**
1. **Option de sortie structurée** (par exemple `--check --format json`) : valider sans ouvrir de fenêtre, et écrire une liste d'objets avec au moins la gravité (erreur / avertissement), la ligne, le timestamp s'il y en a un, la figure concernée et le message.
2. **Code de retour** distinct selon qu'il y a des erreurs, seulement des avertissements, ou rien.

**Quand :** en même temps que la vérification du cadre, puisque les deux servent la boucle de correction par IA.

**Fait :** `--check --format json` écrit une liste d'objets `{severity, code, line, time, figure, other, message}` ; `severity` vaut `error` ou `warning`, `code` vaut `parse`, `validation`, `render`, `outside` ou `overlap` ; `other` est l'autre figure d'un chevauchement. Code de retour : 0 rien, 2 seulement des avertissements, 1 au moins une erreur. Sans `--format json`, la même liste en texte.

#### Ne calculer que les figures utiles ✅

**Constat :** toutes les figures étaient calculées à chaque image, même invisibles (pas encore apparues ou déjà parties), au cas où une autre y ferait référence.

**Fait :** à chaque image, on part des figures affichées et on ajoute de proche en proche celles dont elles dépendent : tout nom de figure qui apparaît comme mot dans leurs propriétés ou leurs animations en cours (repère, scène, `center(...)`, cible d'un `transform` ou d'un `follow`, surface d'un `iterate`), les membres d'un groupe, et tout groupe dont une figure est utile (il la déplace). La règle est prudente : elle peut garder une figure inutile, jamais en oublier une. Vérifié : toutes les démos, toutes les 0,5 s, images identiques au pixel près avec et sans ce tri (`DIDACT_ALL_FIGURES=1` le désactive). Export de `demo_flots_hasard_graphes` : 64 s → 39 s.

---

## 4. Décisions de design

Cette section liste les choix importants qui ont été tranchés. Chaque décision a un **raisonnement** documenté. Si l'on veut revenir dessus, il faut d'abord comprendre pourquoi elle a été prise.

### Sortie par défaut : fenêtre desktop, pas MP4

**Décision :** Le compilateur ouvre une fenêtre (comme matplotlib), il ne produit pas de MP4 par défaut.

**Raisonnement :**
- MP4 oblige à recompiler à chaque modification
- MP4 perd la légèreté du format Didact (5-500 mo vs 15 ko)
- Une fenêtre + WASM couvre 99% des usages
- L'export MP4 reste possible en option pour les cas spéciaux (YouTube, etc.)

### Deux niveaux de syntaxe, pas trois

**Décision :** Un seul niveau de syntaxe structurée + escape hatches. Pas de niveau "ultra-simplifié pour débutants".

**Raisonnement :**
- L'IA joue le rôle de couche d'abstraction pour les non-techniques
- Multiplier les niveaux complique le langage sans bénéfice clair
- Cohérence : un seul vrai langage à apprendre

### Format texte uniquement, pas de binaire

**Décision :** Tous les fichiers Didact sont du texte brut. Pas de format binaire compact.

**Raisonnement :**
- Le gain de taille est négligeable 
- Le binaire casse la lisibilité IA et humaine, qui est la valeur centrale
- Git diff utilisable uniquement en texte
- À reconsidérer seulement si une vraie contrainte apparaît

### Didact doit rester git-friendly

**Décision :** un fichier `.dct` doit se versionner et se fusionner facilement avec git. C'est un critère à vérifier pour toute nouvelle syntaxe ou commande qui réécrit un fichier.

**Raisonnement :**
- Le texte brut ne suffit pas : il faut aussi qu'une modification locale produise un diff local (quelques lignes), lisible dans une revue
- Deux personnes (ou une personne et une IA) qui modifient des parties différentes du fichier ne doivent pas entrer en conflit
- Favorise la correction ciblée plutôt que la régénération complète d'un fichier

### Compilateur = renderer (un seul programme)

**Décision :** Pas de séparation entre "compiler" et "afficher". Une seule commande `didact fichier.dct` fait tout.

**Raisonnement :**
- Analogue à matplotlib (pas à gcc)
- Plus simple à comprendre et utiliser
- Pas de fichier intermédiaire à gérer
- Performances suffisantes pour le re-rendu à chaque exécution

### Langage du compilateur : Rust

**Décision :** Le compilateur est écrit en Rust.

**Raisonnement :**
- Performances natives + WebAssembly mature
- Sécurité mémoire garantie
- Un seul exécutable distribuable
- Voir `ARCHITECTURE.md` §2 pour les alternatives rejetées

### Origine des coordonnées : bas gauche

**Décision :** L'origine (0, 0) est en bas à gauche, l'axe Y va vers le haut.

**Raisonnement :**
- Convention mathématique standard
- Cohérent avec matplotlib
- Public cible inclut scientifiques et profs de maths
- Plus intuitif que la convention écran (origine en haut à gauche)

### Coordonnées en pourcentages

**Décision :** Les positions sont en pourcentages de la scène par défaut, pas en pixels.

**Raisonnement :**
- Le contenu doit être responsive (différentes tailles d'écran, d'embedding)
- Les pixels absolus restent disponibles via la syntaxe `px`
- Pour les graphes mathématiques, on utilise `axes_xy(x, y)` qui est encore plus pertinent

### Convention des angles : positif = trigo

**Décision :** Angle positif = sens trigonométrique (anti-horaire). Angle négatif = horaire.

**Raisonnement :**
- Convention mathématique universelle
- Pas besoin de paramètre `direction` explicite
- Le signe encode l'information

### Unité de temps : secondes.millisecondes

**Décision :** Tous les timestamps sont en `secondes.millisecondes`, format `XX.XXX`.

**Raisonnement :**
- Pas d'ambiguïté entre minutes:secondes et fraction décimale
- Précision suffisante pour les animations (millisecondes)
- Facile à parser, facile à lire

### Animation par défaut : instantanée (opt-in)

**Décision :** Une figure apparaît/disparaît instantanément si aucune animation n'est spécifiée.

**Raisonnement :**
- Comportement prévisible
- Pas de magie cachée
- L'utilisateur ajoute explicitement les animations quand il les veut

### `end` vs `destroy` : deux commandes distinctes

**Décision :** `end` cache visuellement, `destroy` supprime de la mémoire. Deux commandes séparées.

**Raisonnement :**
- Cas réel : une figure peut être référencée par une autre même après avoir disparu visuellement
- Exemple : `rect_mobile` qui continue à suivre `courbe_sin` après que celle-ci ait disparu
- `destroy` automatique serait dangereux et peu prévisible
- Le compilateur vérifie qu'on ne `destroy` pas une figure encore référencée

### Mots-clés avec préfixe `def`

**Décision :** `defstyle`, `deffigure`, `defgroup` au lieu de `style`, `figure`, `group`.

**Raisonnement :**
- Sans préfixe, ambiguïté avec les noms de propriétés (ex: `style : red` vs `style mon_style =`)
- Évite le lookahead dans le parser → parser plus simple et plus robuste
- Lisibilité : on voit immédiatement qu'on définit quelque chose
- Cohérent avec d'autres langages (Lisp : `defun`, Python : `def`)

### Timeline unique pour visuel + audio + commentaires

**Décision :** Pas de fichier séparé pour l'audio et les commentaires. Tout dans `[timeline]`.

**Raisonnement :**
- Vision globale en un coup d'œil
- Synchronisation audio/visuel directement visible
- Pas de risque de désynchronisation entre deux fichiers
- Simplicité d'implémentation

### Moteurs externes : LaTeX et Matplotlib en arrière-plan

**Décision :** Pour les équations on délègue à LaTeX/KaTeX. Pour les graphes complexes, on délègue à Matplotlib (en mode escape hatch).

**Raisonnement :**
- Ne pas réinventer un moteur de rendu mathématique complet
- Profiter des décennies de R&D sur LaTeX
- Garder la syntaxe Didact propre pour 95% des cas, escape hatches pour le reste

### Pas de références en avant

**Décision :** Un élément ne peut référencer qu'un élément défini plus haut dans le fichier.

**Raisonnement :**
- Les références circulaires deviennent impossibles par construction : pas de détection de cycles à écrire
- Le validateur peut tout vérifier en une seule passe, de haut en bas
- Lecture naturelle : on comprend chaque définition avec ce qui précède

### Timestamps uniques

**Décision :** Deux blocs timeline ne peuvent pas avoir le même timestamp (erreur de compilation).

**Raisonnement :**
- Un instant = un bloc : tout ce qui se passe à ce moment est visible au même endroit
- Pas de question d'ordre entre deux blocs simultanés
- Un doublon écrit par erreur est détecté au lieu de passer inaperçu
- Plusieurs figures démarrant ensemble se mettent simplement dans la même liste

### Ordre fixe des commandes dans un bloc

**Décision :** Dans un bloc timeline, les commandes s'exécutent toujours dans l'ordre `destroy` → `end` → `start` → `anim` → `comment`, indépendamment de l'ordre d'écriture.

**Raisonnement :**
- Un bloc décrit un instant, pas une suite d'étapes : l'ordre des lignes ne doit pas changer le résultat
- Cohérent avec les propriétés d'une figure, dont l'ordre n'a pas d'importance non plus
- Supprime une source d'erreurs (écrire `anim` avant `start` sur la même figure)
- `start` avant `anim` : on peut faire apparaître et animer une figure dans le même bloc

**Alternative rejetée :** exécution dans l'ordre d'écriture — oblige à faire attention à l'ordre des lignes dans chaque bloc.

### `stroke` pour l'épaisseur du trait

**Décision :** L'épaisseur du trait s'appelle `stroke`. `width` et `height` sont réservés aux dimensions d'un `rect`.

**Raisonnement :**
- Avec `width` pour les deux, un style d'épaisseur (`width : 2px`) appliqué à un `rect` changeait sa largeur
- Un nom = un sens, quel que soit le type de figure
- Le validateur refuse `width` sur une figure qui n'est pas un `rect` et suggère `stroke`

### Noms sensibles à la casse

**Décision :** `Ma_Ligne` et `ma_ligne` sont deux noms distincts. Pas d'avertissement si deux noms ne diffèrent que par la casse.

**Raisonnement :**
- Convention de la plupart des langages (Rust, Python, C)
- Comparaison de noms triviale à implémenter

### Rendu : tiny-skia pour la scène, egui pour la fenêtre

**Décision :** La scène est dessinée par `tiny-skia` (dessin vectoriel sur processeur, dans une image). La fenêtre et les contrôles sont faits avec `eframe`/`egui`, qui affiche cette image.

**Raisonnement :**
- Qualité de dessin vectoriel (anticrénelage, courbes, transparence) adaptée à des animations de maths
- Export PNG/MP4 trivial : on écrit les images, sans fenêtre ni capture d'écran
- LaTeX naturel plus tard : LaTeX → SVG → `resvg`, qui dessine lui-même avec tiny-skia
- Rendu identique sur toutes les plateformes, testable sans carte graphique
- egui donne les contrôles (pause, barre de lecture) et la compilation WASM

**Alternatives rejetées :**
- egui seul : dessin pensé pour des interfaces, export vidéo maladroit
- winit + wgpu : tout à coder (formes, texte, contrôles), beaucoup plus long à mettre au point ; à reconsidérer seulement si les performances du processeur deviennent un problème (le trait `RenderBackend` permet de changer de backend)

### Positions en %, tailles en % du plus petit côté, `[16:9]` par défaut

**Décision :**
- Les positions restent en % (x de la largeur, y de la hauteur) : elles servent à **placer**.
- Toutes les tailles (`radius`, `size`, `width`/`height` d'un rect) sont en % du **plus petit côté** de la scène.
- La géométrie exacte se fait dans un repère `axes`, à échelle égale en x et en y par défaut.
- `window` vaut `[16:9]` par défaut au lieu de `auto`.

**Raisonnement :**
- Avec des tailles en % de la largeur/hauteur, un « carré » devenait un rectangle en 16:9 et se déformait au redimensionnement : inacceptable pour un outil de maths
- Les positions en % restent intuitives (« au centre », « aux deux tiers »)
- Avec `auto`, chaque spectateur voit une composition différente et l'auteur ne voit pas ce que voient les autres ; un ratio fixe se comporte comme une vidéo
- Les angles et les intersections ne doivent pas dépendre de coordonnées calculées à la main en %

### Repère `axes` défini par une boîte `from` / `to`

**Décision :** Un repère est placé par sa boîte à l'écran (`from` = coin bas-gauche, `to` = coin haut-droit) au lieu d'un `origin`. Avec l'échelle égale (défaut), il est centré dans la boîte.

**Raisonnement :**
- On contrôle directement l'espace occupé à l'écran, comme pour les autres éléments de mise en page
- Fonctionne même quand 0 n'est pas dans les intervalles
- L'échelle égale reste garantie ; `equal : false` remplit la boîte

### `follow` : une durée, pas de vitesse

**Décision :** `follow(path=fig, 10s)` parcourt tout le contour en 10 s, à vitesse régulière. Plus de `speed=1cm/s`.

**Raisonnement :**
- Le plus simple à écrire et à calculer
- Les « cm » n'ont pas de sens sur un écran qui change de taille
- Cohérent avec toutes les autres animations, qui prennent une durée

### Texte et LaTeX : MiTeX + Typst, police Latin Modern, lettres en vecteurs

**Décision :** Tout le texte et le LaTeX passent par la même chaîne : LaTeX → Typst (crate `mitex`) → composé par Typst (crate `typst`, polices Latin Modern embarquées) → SVG → contours vectoriels dessinés comme les autres formes. Résultats mis en cache.

**Raisonnement :**
- Qualité LaTeX réelle : fractions empilées, racines, bornes d'intégrales, matrices, `aligned`, tableaux, crénage du texte
- Pur Rust, rien à installer (pas de distribution LaTeX), fonctionne en WebAssembly (Typst tourne dans le navigateur)
- Le texte s'anime comme une forme (move, rotate, scale) et reste net à toute taille
- Licences compatibles : Typst et MiTeX Apache-2.0, polices GUST

**Coût accepté :** binaire plus lourd (≈ +40 Mo) et première compilation plus longue (quelques minutes). Composition : 4 à 20 ms par texte, une seule fois grâce au cache.

**Alternatives rejetées :**
- Maths en ligne codées à la main : impossible d'atteindre la qualité LaTeX (fractions, bornes, matrices)
- LaTeX installé (pdflatex, tectonic) : dépendance externe lourde, pas de WebAssembly
- KaTeX : JavaScript, ne marche pas en natif

### `expr` lue jusqu'à la fin de la ligne

**Décision :** La valeur de `expr` est le texte brut après `:` jusqu'à la fin de la ligne (sans guillemets).

**Raisonnement :**
- Les formules contiennent des espaces, `^`, `\`, `{}` que le lexer découpait (`x^2 + 1` perdait `+ 1`)
- Garde la syntaxe de la spec (`expr : sin(x)`, `expr : \int_0^\infty ...`)

### Web : balise `<didact-player>`, rendu en pixels dans un canvas

**Décision :**
- Le projet est une bibliothèque (`src/lib.rs`) utilisée par le programme natif et compilée en WebAssembly ; la fenêtre (eframe) n'existe qu'en natif.
- Côté web, le même backend tiny-skia dessine l'image ; JavaScript la copie dans un `<canvas>`. Pas de second backend « Canvas HTML5 ».
- Intégration par une balise `<didact-player src="cours.dct">` (le nom doit contenir un tiret : règle des éléments HTML personnalisés, `<didact>` n'est pas possible).

**Raisonnement :**
- Rendu identique au pixel près entre desktop, export PNG et web
- Un seul code de dessin à maintenir
- Une balise suffit à intégrer une animation, sans écrire de JavaScript

### Groupes : `pos` est un décalage

**Décision :** `pos : (10%, 0%)` sur un groupe décale toutes ses figures de 10 % vers la droite. Sans `pos`, elles restent où elles sont définies.

**Raisonnement :** chaque figure garde ses propres coordonnées, et le groupe permet de déplacer un ensemble déjà construit sans rien recalculer.

### `transform(to=b)` : `b` remplace la figure

**Décision :** pendant le transform, la figure se déforme vers `b` ; à la fin, elle est cachée et `b` est affichée à sa place (comme `ReplacementTransform` de manim). On anime ensuite `b`.

**Raisonnement :** après le transform, le nom `b` désigne ce qu'on voit ; `center(b)` est juste. L'autre option (la figure garde son nom avec l'aspect de `b`) rend les références confuses.

### `slidein` / `slideout` : glissé court + fondu

**Décision :** la figure arrive d'un peu plus loin (12 % de la hauteur), en fondu, style 3Blue1Brown. La direction de `slidein` est le côté d'où elle **vient**, celle de `slideout` le côté où elle **part**.

### `move(direction, distance, durée)` au lieu d'une vitesse

**Décision :** `move(down, 20%, 2s)` remplace `move(down, 3cm/s)`.

**Raisonnement :** les `cm` n'existent pas dans Didact (tout est en %), et une durée obligatoire est cohérente avec toutes les autres animations.

### Paramètres animés : `defparam` et `to(...)`

**Décision :** `defparam k = 1` définit un nombre ; `anim : [k to(3, 2s)]` le fait évoluer. Il est utilisable dans `expr` et dans les coordonnées d'un repère.

**Raisonnement :**
- Une courbe qui change sous les yeux montre la cause et l'effet : c'est le cas où l'animation aide le plus à comprendre
- La valeur ne dépend que de t, comme le reste de la timeline : reculer et sauter marchent sans rien stocker
- Même mécanisme que les autres animations (`anim`, durée, easing, enchaînement)
- Pas de référence en avant : même règle que les figures

**Limite assumée :** pas encore utilisable dans les autres propriétés (`radius`, angles, texte).

### `transform` lettre par lettre entre textes et équations

**Décision :** entre deux figures de texte, `transform` reconnaît les lettres identiques (même contour, à la taille près) : elles glissent à leur place, le reste passe en fondu. Automatique, sans nouvelle syntaxe. Les formes géométriques gardent la déformation point par point.

**Raisonnement :**
- La déformation dans l'ordre des formes mélangeait des lettres sans rapport
- Le spectateur voit ce qui a changé dans la formule, et un terme qui change de côté se déplace vraiment
- On compare les dessins, pas le LaTeX : rien à annoter, et ça marche pour tout ce que Typst compose

**Comment :** plus longue sous-suite commune (ce qui garde son ordre), puis association des lettres identiques restantes.

### `color(..., part="...")` plutôt qu'une nouvelle animation

**Décision :** surligner un terme, c'est `color` avec un paramètre `part`. Le morceau est composé à part puis retrouvé dans la figure, lettre par lettre.

**Raisonnement :** un nom = un sens : c'est bien un changement de couleur. Un morceau introuvable est signalé à la compilation, avec sa ligne.

### Modèles : `defmodel`, `defrepeat`, `@nom`

**Décision :** un modèle est du texte avec des trous : `@nom` et `@(calcul)`. `deffigure a = modele(...)` remplit les trous et relit le texte comme une figure ordinaire. `defrepeat nom = modele(@i) for i in 0..7` crée `nom_0` … `nom_7` et le groupe `nom`.

**Raisonnement :**
- Remplacer du texte fonctionne pour toutes les propriétés, y compris le LaTeX et `expr`, sans cas particulier
- `@` n'entre en conflit ni avec le LaTeX (`{}`) ni avec les maths (`$`), ni avec les formules
- Le validateur et le renderer ne changent pas : ils voient des figures normales
- Le groupe créé par `defrepeat` donne directement les animations d'ensemble

**Limite assumée :** le nombre de figures est fixé à la compilation, il ne peut pas dépendre d'un paramètre animé.

### Rechargement à chaud dans la fenêtre

**Décision :** la fenêtre surveille le fichier `.dct` (date et taille, 10 fois par seconde) et recharge à chaque sauvegarde, au même instant de la timeline. En cas d'erreur, l'ancienne version reste affichée, avec l'erreur par-dessus.

**Raisonnement :**
- Tester un réglage ne doit pas obliger à relancer et à revenir au bon instant
- Une faute de frappe pendant l'écriture ne doit pas fermer la fenêtre
- Aucune dépendance en plus : un regard sur la date du fichier suffit. On attend que le fichier soit stable, car les éditeurs écrivent parfois en deux temps
- Le son n'est redécodé que si ses pistes ont changé

### Temps absolus gardés, décalage par `didact shift`

**Décision :** les timestamps restent absolus. Pour insérer ou retirer du temps au milieu, `didact shift cours.dct 16 +2` réécrit tous les blocs à partir de 16 s.

**Raisonnement :**
- Le problème réel était d'insérer quelque chose au milieu : il fallait décaler tous les blocs suivants à la main
- Les temps absolus restent lisibles : on voit dans le fichier à quel instant tombe chaque bloc
- Des temps relatifs (`+2` partout) ou une suite d'étapes rendaient le chevauchement d'animations moins direct et l'instant d'un bloc moins visible
- Un geste explicite : Didact ne devine pas si une modification d'un timestamp doit entraîner la suite

**Écartés pour l'instant :** blocs relatifs à convertir ensuite (`+2`, `didact format`), extension d'éditeur.

### Moteur 3D : projection triée plutôt que tampon de profondeur

**Décision :** les objets 3D sont découpés en petits morceaux (faces, segments, points) projetés par la caméra et triés du plus loin au plus proche, puis dessinés par le moteur 2D existant.

**Raisonnement :**
- Le style visé (quadrillages translucides, trajectoires lumineuses) repose sur la transparence et l'anti-crénelage : c'est le point fort du tri, le point faible d'un tampon de profondeur
- Le rendu, la fenêtre, le web, le MP4 et la page HTML ne changent pas : la 3D produit des formes 2D
- Toutes les animations, les paramètres et les références marchent aussi en 3D, sans moteur parallèle
- Même méthode que le rendu par défaut de Manim

**Limite assumée :** deux grandes faces qui se coupent peuvent être mal ordonnées par endroits ; on découpe finement pour l'éviter.

### Une scène 3D est une figure, les objets s'y rattachent

**Décision :** `type : scene3d` porte la caméra ; les objets ont `scene : nom`. Les points s'écrivent `espace(x, y, z)` (comme `repere(x, y)` en 2D) ou `surface(a, b)` (posé sur une surface).

**Raisonnement :** même logique que les repères 2D ; plusieurs scènes possibles côte à côte ; la caméra se règle et s'anime avec des paramètres, sans syntaxe nouvelle. Les figures 2D acceptent les points 3D : une équation suit un point de la scène.

### `iterate` : une récurrence générale, les optimiseurs en raccourcis

**Décision :** `vars`, `init`, `update` décrivent n'importe quelle suite ; `method : gd | momentum | nesterov | rmsprop | adam | newton` en sont des raccourcis. Avec `on : surface`, les dérivées (`dx`, `dy`, `hxx`, ...) sont calculées par Didact.

**Raisonnement :** le but est de tout pouvoir animer : une méthode qui n'existe pas encore s'écrit en une ligne. Les méthodes courantes restent simples à utiliser.

### Halo (`glow`) calculé une fois par image

**Décision :** les formes sont dessinées nettes dans l'ordre ; les halos de toute l'image sont floutés en une seule fois puis ajoutés (comme le « bloom » d'un moteur 3D).

**Raisonnement :** une trajectoire en tube compte des milliers de morceaux ; un flou par morceau coûtait 1,1 s par image, une seule passe 0,2 s.

### Expressions : une seule grammaire, enrichie

**Décision :** une seule grammaire de formules sert partout (`expr`, coordonnées, propriétés, `#{...}`, `map`). Elle gagne `sum`/`prod` à variable liée, les fonctions à plusieurs arguments, les comparaisons et `if(c, a, b)`, et un mode complexe (`z`, `i`) pour `cmap`.

**Raisonnement :** apprendre une syntaxe une fois ; tout ce qui s'écrit dans un `plot` s'écrit ailleurs. `if(...)` plutôt qu'une notation par cas : lisible, imbricable, sans nouveau symbole. Une borne de somme non entière ajoute le terme suivant en fraction : animer `n` fait apparaître les termes en douceur au lieu de sauts.

### Paramètres dans toutes les propriétés, nombres dynamiques `#{...}`

**Décision :** un nombre peut être remplacé par un calcul de paramètres suivi de son unité (`(k + 1)px`, `(2k)%`). Dans le texte, `#{expr}` / `#{expr:d}` insère la valeur.

**Raisonnement :** pas de liste de propriétés « animables » à retenir : tout nombre l'est. Le calcul est reconnu seulement s'il utilise un paramètre déjà défini (pas de confusion avec `ease-in-out` ou une couleur). `#{` ne se confond ni avec un commentaire (`# `) ni avec une couleur (`#FF0000`). Les décimales sont fixées par l'auteur (`:0`) pour éviter un texte qui tremble.

### Transformations : propriétés de la figure, avancement par `amount`

**Décision :** `matrix`, `map`, `cmap` et `amount` sont des propriétés de la figure, appliquées dans les coordonnées de son repère ; nouveau type `grid` pour le quadrillage à déformer.

**Raisonnement :** une seule idée (déformer le plan) couvre algèbre linéaire, changements de variables et applications conformes, en 2D comme en 3D. L'animation passe par un paramètre (`amount : t`) : pas de nouvelle animation, on combine ce qui existe. Interpolation linéaire entre le point et son image (comme 3Blue1Brown) : simple et prévisible.

Une figure placée dans un repère est coupée à son cadre (voir « Caméra 2D »).

### Caméra 2D : le repère est la caméra

**Décision :** pas d'objet caméra en 2D. Le repère reçoit `center : (x, y)` et `zoom`, en plus de xrange/yrange, et tout cela accepte des paramètres. Les figures qui ont `axes : repere` sont coupées à son cadre.

**Raisonnement :** en 2D, « où regarde-t-on » est exactement la fenêtre du repère ; plusieurs repères côte à côte peuvent zoomer indépendamment. Un zoom s'anime avec un paramètre (`zoom : 10^k`, `k` linéaire → zoom régulier), sans nouvelle animation. Le reste suit seul : graduations, `step : auto`, courbes affinées là où il faut.

### Régions et courbes de niveau : une grille fine, pas de calcul symbolique

**Décision :** `contour` (f(x, y) = c, ou une équation `g = h`) et `region` (`upper`/`lower`, ou `where : inégalités`) sont calculés sur une grille (marching squares). `where` combine les contraintes par `and`/`or`/`not`. Les niveaux automatiques suivent la répartition des valeurs.

**Raisonnement :** marche pour n'importe quelle formule, sans résoudre d'équation ; une région est remplie d'un seul tenant (pas de joints visibles). Des niveaux également espacés en valeur tombent presque tous sur les pentes raides d'une fonction de coût : par quantiles, on voit le creux.

### Cache entre images pour les calculs lourds

**Décision :** les courbes de niveau, régions et maillages de surface sont gardés d'une image à l'autre ; la clé contient tout ce dont ils dépendent.

**Raisonnement :** toutes les figures sont calculées à chaque image (les références en ont besoin), même immobiles ou pas encore apparues. Le résultat est identique avec ou sans cache.

### Équations différentielles : un seul type, `trajectory`

**Décision :** une trajectoire (RK4) couvre la courbe solution, le portrait de phase (`count` + `init` en k), le nuage de particules (`trail`, `head`) et l'attracteur 3D. Pas de type séparé pour les lignes de courant ni pour les particules.

**Raisonnement :** ce sont la même chose vue autrement : des solutions d'une même équation à partir de points de départ différents, montrées en entier ou seulement leur fin. Une seule syntaxe à apprendre, et `duration` animé suffit pour tout faire avancer.

### Hasard reproductible : `rand(k)`, pas de générateur caché

**Décision :** `rand(k)` et `randn(k)` sont des fonctions déterministes de k (et d'une graine facultative). Un tirage est désigné par son numéro.

**Raisonnement :** une animation doit être identique à chaque rendu, en avant comme en arrière dans le temps. Désigner les tirages par un numéro permet de les réutiliser exactement (le même point dans le nuage et dans l'estimation de π) et de les sommer avec `sum` (théorème central limite).

### Graphes et tableaux : des morceaux nommés, surlignés par `color(part=...)`

**Décision :** pas de figures séparées pour chaque nœud ou case : un `graph` ou un `array` nomme ses morceaux (`B`, `"A-C"`, `"(2,3)"`, `"row 2"`), et l'animation existante `color(part=...)` les colore.

**Raisonnement :** un algorithme se déroule en surlignant pas à pas : c'est exactement ce que `part` faisait déjà pour les équations. Rien de nouveau à apprendre dans la timeline.

### `pixels` : une image calculée, posée sur le cadre du repère

**Décision :** un type `pixels` avec trois modes (`expr`, `domain`, `escape`), calculé à la résolution de l'écran, réparti sur les cœurs, gardé en cache. Une forme peut désormais porter une image.

**Raisonnement :** coloration de domaine, fractales et cartes de chaleur ne se dessinent pas avec des traits. Calculer à la taille de l'écran (et non sur une grille fixe) garde des détails nets à tout zoom, avec la caméra 2D.

### `pos` = centre, 9 ancres

**Décision :** `pos` désigne le centre de la figure, y compris pour `rect` (avant : coin bas-gauche). `anchor` accepte 9 valeurs (`topleft`, `top`, `topright`, `left`, `center`, `right`, `bottomleft`, `bottom`, `bottomright`), pour `rect` et les figures de texte ou de contenu (boîte englobante).

**Raisonnement :** cercles, étoiles, textes et équations étaient déjà placés par leur centre ; le rect était la seule exception. Les 9 ancres permettent d'aligner un texte sur un bord sans calculer sa taille.

### Couleurs : `hsl()` en plus des noms et de l'hexadécimal

**Décision :** `hsl(teinte, s%, l%)` et `hsl(t, s%, l%, opacité)`, avec paramètres dans les composantes. Pas de `rgb()` pour l'instant (l'hexadécimal le couvre).

**Raisonnement :** la teinte se règle et s'anime avec un seul nombre (`hsl(360*t, 80%, 50%)`), ce qui est pratique pour colorier une suite d'objets ou un balayage.

### Numérotation : k va de 1 à n

**Décision :** dans `trajectory` (`init` avec `count`), k va de 1 à n, comme dans `scatter`/`histogram` et `sum`.

**Raisonnement :** une seule convention dans tout le langage, celle des maths (u₁ … uₙ).

### Easing : `smooth` pour les figures, `linear` pour les paramètres

**Décision :** une animation de figure démarre et finit lentement ; `to` sur un paramètre va à vitesse constante. `ease=...` change l'un ou l'autre.

**Raisonnement :** un mouvement doux est le style 3Blue1Brown. Mais un paramètre est souvent un temps ou un zoom : avec `smooth`, le temps ralentissait au début et à la fin.

### Une figure dans un seul groupe

**Décision :** une figure ne peut être que dans un groupe (erreur claire sinon) ; un groupe peut contenir un autre groupe.

**Raisonnement :** avec deux groupes, les effets s'additionnaient et les cas étaient ambigus (un groupe disparaît en fondu, l'autre reste : la figure doit-elle partir ?). Les groupes imbriqués couvrent le besoin sans ambiguïté.

### `comment : none` efface le sous-titre

**Décision :** `comment : none`, une seule valeur pour toutes les langues.

**Raisonnement :** plus lisible qu'une liste de chaînes vides, une par langue.

### Unités : `%`, `px`, `deg`, `rad`, `s`, `ms` ; pas de position en pixels absolus

**Décision :** ces six unités seulement ; `cm`, `mm`, `pt`, `em` sont refusés avec un message qui dit quoi écrire. Les positions sont en % (ou dans un repère) : `pos : (200px, 300px)` est refusé.

**Raisonnement :** une longueur physique n'a pas de sens sur un écran de taille inconnue. Une position en pixels ne garde pas la même image quand la taille ou les proportions de l'écran changent. `rad` est naturel en maths.

### Timestamps au millième

**Décision :** la précision des timestamps reste la milliseconde.

**Raisonnement :** suffisant (une image dure 16,7 ms à 60 images/s) et précis.

### `allow` gardé dans `--check`

**Décision :** `allow : outside | overlap` reste, et seuls les chevauchements texte contre texte sont signalés.

**Raisonnement :** une personne peut ignorer un avertissement, mais une IA qui relance `--check` le verrait à chaque fois, « corrigerait » un effet voulu, et le code de retour ne serait jamais 0. Un texte sur une forme (étiquette sur une courbe) est normal, donc pas signalé.

### Remarques d'hygiène dans `--check`

**Décision :** figure jamais affichée ni utilisée, style ou paramètre jamais utilisé : signalés comme remarques (`info`), sans changer le code de retour.

**Raisonnement :** utile pour nettoyer un fichier, mais ce n'est jamais une faute : un fichier de styles partagés en définit forcément qui ne servent pas partout.

### Web : Typst gardé dans le moteur

**Décision :** le moteur web garde Typst (environ 10 Mo compressés, en cache après la première visite).

**Raisonnement :** rendu identique au bureau et au MP4, texte modifiable dans le navigateur (nécessaire pour un éditeur en ligne), aucun travail en plus. Un lecteur léger (textes précomposés) reste possible plus tard ; KaTeX est écarté (rendu différent, pas de morphing lettre par lettre).

### Petites propriétés : le minimum utile

**Décision :** `corner_radius` (rect), `radius : (rx, ry)` (ellipse, sans nouveau type), `inner` (étoile), `marker` (scatter), `step` (axes, graduations en π quand le pas en est un multiple), `closed : false` (polygon), `loop` (audio). Pas de style de bordure pour `table`, pas de choix de police.

**Raisonnement :** chacune répond à un besoin courant d'un cours de maths (trigonométrie, méthode d'Euler, séries de points distinguées sans la couleur). L'ellipse reste un `circle` : un type de moins à apprendre.

### Gras et italique : par LaTeX, pas par des propriétés

**Décision :** `\textbf{...}`, `\textit{...}`, `\emph{...}` dans `text`, `latex` et `table` ; pas de `bold : true`. Les polices grasse et italique de Latin Modern sont embarquées.

**Raisonnement :** le gras ne porte souvent que sur un mot (« **Théorème.** ... ») : une propriété sur toute la figure ne suffirait pas, et la syntaxe LaTeX est déjà connue. Avant, `\textbf` était ignoré faute de police grasse.

### Type `image` : PNG et SVG, posés comme un rect

**Décision :** `file`, `pos`, `size` (plus grand côté), `anchor` ; SVG dessiné par resvg à la taille de l'écran ; animations de fondu, déplacement, échelle, rotation ; `draw` devient un fondu ; pas de `transform`.

**Raisonnement :** pouvoir réutiliser un schéma fait ailleurs sans le redessiner. Une image n'a pas de contour : ce qui doit se transformer se dessine en Didact.

### Rien n'est ignoré en silence

**Décision :** une propriété inconnue pour le type de la figure, un paramètre d'animation inconnu, un type inconnu sont des erreurs, avec une suggestion (« vouliez-vous dire `color` ? »). Un calcul dans une animation (`to(2*pi, 3s)`) est calculé ou refusé, jamais tronqué. Dans un style, seuls les noms inconnus de tous les types sont des erreurs (un style sert à plusieurs types).

**Raisonnement :** des essais en écrivant comme une IA (9 vidéos) ont montré que les erreurs silencieuses sont le pire défaut pour elle : `colour : red`, `xrange` sur un `plot` ou `to(2*pi)` (lu 2) passaient `--check` sans un mot et donnaient une vidéo fausse sans moyen de le savoir.

### LaTeX : compléter MiTeX plutôt que le remplacer

**Décision :** les anciens noms de symboles produits par MiTeX (`diff`, `sect`, `angle.l`, ...) sont redéfinis pour Typst 0.15 ; `|x|` et `\|x\|` sont appariés en délimiteurs ; `\textcolor` / `\color` gardent leur couleur ; les listes d'un bloc `latex` sont alignées à gauche.

**Raisonnement :** `\partial`, `\langle`, `\cap` et les valeurs absolues sont partout en maths ; une table de correspondance et quelques retouches du LaTeX suffisent, sans toucher à MiTeX.

### Licence MIT

**Décision :** Le projet est sous licence MIT.

**Raisonnement :**
- Maximum d'adoption (entreprises, profs, etc. peuvent l'utiliser sans contrainte)
- Permet de construire un produit commercial sans limitation

### Renaming `[figures]` envisagé puis rejeté

**Décision considérée :** Renommer `[figures]` en `[objects]` ou `[definitions]`.

**Décision finale :** Garder `[figures]`.

**Raisonnement :**
- `figures` est le terme utilisé dans Matplotlib, LaTeX, et la communication mathématique
- Plus court et plus parlant pour le public cible
- Inclut techniquement aussi les styles et groupes, mais la section reste cohérente conceptuellement

---

## 5. Idées reportées

Idées intéressantes mais non prioritaires. À reconsidérer plus tard.

### Format binaire compact

**Idée :** Permettre de stocker les fichiers `.dct` sous forme binaire pour économiser de la place.

**Quand reconsidérer :** Si une vraie contrainte de stockage apparaît (par exemple, des dizaines de milliers de fichiers à indexer).

### Mode syllabus / texte lisible

**Idée :** Compiler un `.dct` en document texte lisible (style polycopié de cours), où la timeline devient une succession de slides.

**Pourquoi reporté :** Idée intéressante mais pas convaincue de la valeur réelle. À tester en pratique avant de spécifier.

**Quand reconsidérer :** Quand le projet a des utilisateurs réels et qu'on peut savoir s'il y a une demande.

### Mode podcast (TTS automatique)

**Idée :** Générer automatiquement un podcast audio à partir des `comment` du fichier (TTS).

**Pourquoi reporté :** Pas une priorité court terme. Le mode visuel est plus important d'abord.

**Quand reconsidérer :** Après que le rendu visuel soit solide.

### Traduction automatique par IA

**Idée :** Au lieu de demander à l'utilisateur de traduire chaque `comment`, une IA traduit automatiquement à partir d'une langue source.

**Pourquoi reporté :** Pas une fonctionnalité du compilateur, mais d'un outil tiers. Sera ajouté en option plus tard.

### Packages communautaires

**Idée :** Permettre l'import de bibliothèques de figures réutilisables (ex: `physique.pendule`, `maths.normale`).

**Pourquoi reporté :** Nécessite que le langage de base soit stable et qu'il y ait une communauté. Trop tôt.

### Éditeur web style Overleaf

**Idée :** Une interface web pour éditer des fichiers `.dct` avec prévisualisation en temps réel.

**Pourquoi reporté :** C'est un produit séparé, pas le compilateur.

### Format binaire pour le stockage interne

**Idée :** Permettre d'optimiser le stockage côté serveur en convertissant en binaire pour le transit.

**Pourquoi reporté :** Même raisonnement que pour le format binaire général.

---

## 6. Idées rejetées (et pourquoi)

Idées qui ont été discutées et rejetées explicitement.

### Compiler vers MP4 comme cible principale

**Pourquoi rejeté :** Perd tous les avantages du format texte (légèreté, versionnable, embarquable, modifiable). Le MP4 est une option d'export, pas le mode principal.

### Trois niveaux de syntaxe (débutant / intermédiaire / expert)

**Pourquoi rejeté :** Multiplie la complexité du langage sans bénéfice clair. L'IA remplace efficacement le niveau "débutant".

### Mots-clés sans préfixe (`style`, `figure`)

**Pourquoi rejeté :** Crée des ambiguïtés avec les noms de propriétés. Le préfixe `def` est court et clair.

### Indentation significative (style Python)

**Pourquoi rejeté :** Trop fragile pour un langage destiné à être édité par des IAs et copié-collé fréquemment. Les blocs `{}` sont plus robustes.

### Origine des coordonnées en haut à gauche

**Pourquoi rejeté :** Public cible mathématique. La convention écran (origine en haut) est moins intuitive pour le public visé.


### Animation par défaut = fadein automatique

**Pourquoi rejeté :** Magie implicite. Le comportement par défaut doit être prévisible : pas d'animation à moins qu'on en demande une.

### `destroy` automatique quand `end`

**Pourquoi rejeté :** Casserait les cas légitimes où une figure cachée est encore référencée. La séparation `end` / `destroy` est intentionnelle.


### Stocker le rendu côté serveur

**Pourquoi rejeté :** Le modèle "texte stocké, rendu chez le visiteur" est plus efficace, plus scalable, et préserve l'idée de format vivant et modifiable.


---

## Changelog des versions du langage

### v0.2 (en cours)
- Graphes : boucles `A->A`, arêtes réciproques courbes, `initial` et `accepting` (automates), étiquettes entre guillemets ; `part=` vise aussi un nœud, une arête ou une case dans les positions (`above(g, 1%, part="B")`) et `frame`
- Cohérence : `offset` (et non `pos`) pour le décalage d'un groupe ; `font` retiré des textes (`size` seul ; `font` reste sur `graph` et `array`) ; `frame` (et non `axes`) pour l'affichage du repère d'une `scene3d` ; blocs de la timeline dans l'ordre du temps (erreur sinon) ; `comment : "Texte"` avec une seule langue ; action de timeline inconnue : erreur ; instants relatifs `+d =>` retirés (`didact shift` sert à insérer) ; `move(to=)` pose le point de `pos` (ancre) ; deux animations contradictoires en même temps : erreur
- Audit et tests à l'aveugle (des IA n'ayant que la doc reproduisent des vidéos) : calculs dans les listes et couples (`[0, 2*pi]`) ; `line extend`, type `brace`, polygone régulier (`sides`), positions `start` / `end` / `vertex` / `side`, `offset` au bout d'une flèche ; `rotate(..., around=)`, `swap` sur un `array` ; `decimal : comma` ; `\"` dans une chaîne ; sous-titres dans `--png` ; `riemann` en fondu quand `n` est animé ; rayon sans unité refusé hors repère (cercle, arc) ; noms de fonctions de position réservés ; erreurs claires pour guillemet ou crochet non fermé, `nan`, durées absurdes ; une figure qui ferait planter le calcul devient une erreur ; section Recettes de la SPEC
- Propriété inconnue dans `[config]` : erreur avec suggestion (avant : avertissement) ; `bands` accepté sur `pixels` ; test `src/doc_check.rs` : SPEC et REFERENCE doivent nommer chaque type, propriété, animation, fonction, position et palette du code
- SPEC réécrite en version compacte (tout ce qui est décrit est implémenté, vérifié sur le code) et fiche d'une page `Readme/REFERENCE.md` ; `size` marche sur `text` (synonyme de `font`), `align` sur `latex` et `table` ; `didact --help`
- Bloc B des essais « comme une IA » : `deffunction f(x) = ...` (fonctions nommées dans toutes les formules) ; `plot` partiel (`xrange : [0, T]`) ; type `curve` (paramétrée `(x(t), y(t))` ou `polar`) ; rayon de `circle` en unités du repère ; `transform(..., rigid=true)` (déplacement rigide, symétrie au besoin) ; morceaux de formule (`center(eq, part="...")`, `top`/`bottom`/`left`/`right`/coins) et type `frame` ; placement relatif `below` / `above` / `left_of` / `right_of` avec ancre implicite ; repères alignés (`align_x`, `align_y`, `same_scale`) ; types `tangent` et `riemann` ; texte des cases d'un `array` colorable (`part="text (i,j)"`)
- Bloc A des essais « comme une IA » : propriétés, paramètres d'animation et types inconnus refusés avec suggestion ; calculs de constantes dans les animations (`to(2*pi, 3s)`, auparavant tronqué à 2) ; `expr : |x| ...` n'est plus pris pour un bloc ; LaTeX `\partial`, `\langle`, `\cap`, `\hbar`, `\Re`, `\Im`, `\mathscr`, `\color`, `\textcolor`, `|x|`, `cases`, `\not\in` ; paramètres utilisables dans un `defmodel` (`opacity : @i < n`) ; pointes de flèche rigides sous `matrix` / `map` / `cmap` ; listes `itemize` alignées à gauche ; `-0.00` affiché `0.00`
- Type `image` (PNG, SVG via resvg ; `src/renderer/image.rs`) ; `corner_radius`, ellipses (`radius : (rx, ry)`), `inner` (étoile), `marker` (scatter), `step` des axes et graduations en π, `closed : false`, `audio ... loop` ; gras et italique (`\textbf`, `\textit`, `\emph`) avec les polices Latin Modern grasse et italique ; `demo_details.dct`
- `to` sur un paramètre à vitesse constante par défaut ; une figure dans un seul groupe ; `comment : none` ; unités `%`, `px`, `deg`, `rad`, `s`, `ms` (`rotate : 30deg` / `0.5rad` marche enfin), `cm`/`mm`/`pt`/`em` et positions en px refusés ; remarques `unused_figure` / `unused_style` / `unused_param` dans `--check`
- `pos` d'un `rect` = son centre (avant : coin bas-gauche) ; `anchor` à 9 valeurs pour `rect` et les figures de texte (`text`, `equation`, `latex`, `raw.latex`, `table`, `array`, `graph`) ; couleurs `hsl(t, s%, l%[, a])` (paramètres acceptés) ; k de 1 à n dans `trajectory`
- `--check` (figures hors du cadre, textes qui se chevauchent, figures non dessinables ; `allow : outside | overlap`) et `--format json`, codes de retour 0 / 2 / 1 (`src/check.rs`) ; seules les figures affichées et leurs dépendances sont calculées à chaque image
- `trajectory` : équations différentielles (RK4), portraits de phase, particules avec traînée, attracteurs 3D (`src/renderer/ode.rs`) ; `rand`, `randn`, `fact`, `binom`, `gamma`, `erf` ; `scatter` et `histogram` tirés par formule (`count`, `range`, `density`, `where`) ; formules compilées, sommes comprises
- `graph` (dispositions cercle, grille, arbre, forces) et `array` (matrices), morceaux surlignés par `color(part=...)` (`src/renderer/discrete.rs`)
- `pixels` : cartes de chaleur, coloration de domaine, fractales par temps de fuite ; une forme peut porter une image (`src/renderer/pixels.rs`) ; `demo_flots_hasard_graphes.dct`
- Caméra 2D : `center` et `zoom` sur un repère (paramètres acceptés), découpe des figures au cadre du repère (`clip : false`), graduations précises à tout zoom, `step : auto` pour `grid`, `plot` affiné là où la courbe varie vite (`src/renderer/clip.rs`)
- Types `contour` (courbes de niveau, courbes implicites) et `region` (entre deux courbes, ou par inégalités) ; lignes de niveau des surfaces 3D (`levels`, `levels_on`) ; couleurs de surface limitées à zrange ; cache des calculs lourds entre images (`src/renderer/contour.rs`, `src/renderer/memo.rs`) ; `demo_niveaux.dct`
- Expressions : `sum`/`prod`, `min max atan2 mod hypot clamp round sign`, comparaisons, `if(c, a, b)`, `and or not`, nombres complexes (`z`, `i`, `re im arg conj`)
- Paramètres dans toutes les propriétés numériques (`radius : (2k)%`, `stroke : (k+1)px`, tuples, listes de points) ; nombres dynamiques `#{expr}` / `#{expr:d}` dans le texte (`src/renderer/params.rs`)
- Transformations `matrix` / `map` / `cmap` + `amount` sur les figures 2D (dans un repère) et 3D ; type `grid` (`src/renderer/warp.rs`) ; `demo_transformations.dct`
- 3D : `scene3d` (caméra animable, perspective/ortho, lumière, repère), `surface` (explicite ou paramétrée, palettes, grille, découpe), `curve3d`, solides (`cube`, `box`, `sphere`, `cylinder`, `cone`, `plane`, `polyhedron`), `arrow` et `field` en 2D et 3D, `iterate` (récurrence générale + 6 optimiseurs), animations `rotate3d` et `fill`, `transform` entre surfaces, halo `glow`, caméra à la souris dans la fenêtre ; démos `demo_3d_*.dct` et `demo_champs_2d.dct`
- `didact shift` : décaler les blocs à partir d'un instant (`src/shift.rs`)
- `defmodel` / `defrepeat` (`@nom`, `@(calcul)`) ; points de `polygon` dans un repère ; `demo_modeles.dct`
- Rechargement à chaud de la fenêtre à chaque sauvegarde (`src/output/reload.rs`)
- `transform` lettre par lettre entre textes et équations ; `color(..., part="...")` ; `align` sur `equation` ; `demo_equations.dct`
- Paramètres animés : `defparam`, animation `to(valeur, durée)`, utilisables dans `expr` et dans les coordonnées d'un repère (`repere(a - 1, sin(a))`) ; `demo_parametres.dct`
- Mots-clés `defstyle`, `deffigure`, `defgroup` (renommage)
- Documentation restructurée en trois fichiers (SPEC, ARCHITECTURE, DECISIONS)
- Clarification du modèle compilateur = renderer
- Parser : correction de la reconnaissance de `defstyle`/`deffigure`/`defgroup`
- `width` → `stroke` pour l'épaisseur du trait (`width` réservé aux `rect`)
- Runtime timeline : instantanés après chaque bloc, état à t par recherche dichotomique (avancer/reculer gratuit), commentaires dans la langue choisie, audio
- CLI : `--lang XX`, `--debug` (tokens et AST seulement sur demande), `--summary`, `--png <t> <fichier>`
- Phase 6 (23) : `--voice` (synthèse vocale par un programme externe configuré dans `didact-voix.txt`, un fichier par phrase dans `cours_voix/`, régénération seulement des phrases modifiées, maths prononcées, avertissements de durée) ; voix jouées dans la fenêtre (selon la langue), sur le web, dans le MP4 et la page autonome ; `--export audio` (MP3)
- Phase 5 : lecture audio dans la fenêtre (décodage symphonia, sortie cpal, mélange des pistes, sauts/pause), sur le web (`audioSegments()` + `<audio>`), fichiers audio inclus dans `--export html`
- Langage complété : groupes (`defgroup`, `pos` = décalage, `start`/`end`/`destroy` en cascade, animations autour du centre de l'ensemble) ; `draw`, `write` ; `slidein`/`slideout` (glissé court + fondu) ; `move(direction, distance, durée)` ; `transform(to=b)` (morphing, `b` remplace la figure à la fin) ; pourcentages négatifs (`-10%`) ; `demo_langage.dct` ; `demo_complete.dct` refaite (7 chapitres, un groupe par chapitre)
- Phase 6 : `--export mp4` (images rendues dans le programme et envoyées à ffmpeg, H.264 ; sous-titres incrustés coupés en lignes, maths jamais coupées ; pistes `audio` placées et mixées ; options `-o`, `--fps`, `--height`, `--no-subtitles`, `--ffmpeg`) ; `--export html` (page autonome : source, lecteur et moteur WASM compressé en gzip/base64 dans un seul fichier, ouvrable en `file://` ; option `--engine`)
- Phase 4 : bibliothèque + programme ; module `web` (wasm-bindgen : `Player`) ; lecteur `web/didact.js` (`<didact-player>`, contrôles, sous-titres, langues, erreurs) ; `web/build.ps1` ; guide `Readme/WEB.md`
- LaTeX complet via MiTeX + Typst : `text` avec `$...$`, `equation`, `latex`, `raw.latex`, `table` ; erreurs LaTeX signalées avec le numéro de ligne
- Blocs `|` multilignes lus depuis le source (retours à la ligne et `|` conservés)
- `axes` (boîte from/to, échelle égale, graduations, grille), `plot` (évaluateur d'expressions), `scatter`, `histogram`, `repère(x, y)`
- `follow(path=fig, durée, rotate=tangent)`, propriété `rotate` (angle ou `auto_tangent(courbe)`)
- `exemple.dct` adapté (from/to, follow avec durée) et jouable en entier
- Animations : entrée (`start`), sortie (`end`, la figure reste affichée pendant le fondu), `anim`, enchaînements ; fadein, fadeout, grow, shrink, move, color, scale, rotate ; easing
- Références dynamiques : `center()`, `angle()`, `offset()` recalculées à chaque frame, dans l'ordre de définition
- Renderer tiny-skia + fenêtre egui (lecture/pause, barre de lecture, langue, sous-titres)
- Lexer : couleurs hexadécimales `#FF0000` (avant : lues comme un commentaire)
- Parser : listes de points `[(20%, 80%), ...]` (avant : les points étaient perdus)
- Résolveur : styles et chaîne `extends` appliqués, surcharges, tri par `layer`, timeline triée
- Numéros de ligne dans toutes les erreurs (lexer → AST → parser/validateur)
- Validateur : noms uniques, pas de référence en avant, types et animations connus, timestamps uniques, nombre de traductions, `destroy` vérifié
- Détails étendus sur tous les types de figures

### v0.1 (initiale)
- Premier draft de la spécification
- Structure de fichiers (modes unique et séparé)
- Trois symboles fondamentaux (`=`, `:`, `=>`)
- Sections `[config]`, `[figures]`, `[timeline]`
- Types de figures de base
- Animations principales
- Audio et commentaires multilingues
- Escape hatches `raw.latex` et `raw.matplotlib`
- Implémentation lexer, AST, parser en Rust