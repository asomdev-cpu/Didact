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
| Parser | ⚠️ Fonctionnel avec bug | `src/parser.rs` |
| Validateur | 🔲 Pas commencé | - |
| Résolveur | 🔲 Pas commencé | - |
| Runtime | 🔲 Pas commencé | - |
| Renderer | 🔲 Pas commencé | - |
| Output natif | 🔲 Pas commencé | - |
| Output WASM | 🔲 Pas commencé | - |

### Métriques

- Versions de la spec : v0.1 (initiale), v0.2 (actuelle, plus détaillée et restructurée)
- Licence : MIT
- Plateforme cible : Windows, Mac, Linux (natif) + Navigateur (WASM)
- Langage : Rust

---

## 2. Bugs connus


## 3. Roadmap par ordre de priorité

### Phase 1 — Faire tourner un rendu basique

L'objectif est d'arriver à un `cargo run -- exemple.dct` qui ouvre une fenêtre et affiche **quelque chose** (même si imparfait).

1. **Corriger le bug du parser** (defstyle/deffigure/defgroup)
2. **Validateur minimal** — vérifier les références de figures et de styles
3. **Résolveur de styles** — aplatir l'héritage
4. **Runtime timeline simple** — start/end sans animations
5. **Renderer minimal** — lignes, cercles, rectangles uniquement
6. **Affichage fenêtre** — via `egui` ou `winit + wgpu`

### Phase 2 — Animations et temps réel

7. **Système d'animations** — fadein, move, color
8. **Easing functions** — linear, ease-in-out, etc.
9. **Références dynamiques** — center(), angle(), offset()
10. **Contrôles utilisateur** — pause, play, scrubber temporel

### Phase 3 — Maths et graphes

11. **Intégration LaTeX** — KaTeX en mode WASM, tectonic en natif
12. **Type `axes`** — système de coordonnées mathématique
13. **Type `plot`** — courbes de fonctions
14. **Type `scatter` et `histogram`**

### Phase 4 — Embarquabilité

15. **Compilation WebAssembly**
16. **API JavaScript pour l'embarquement**
17. **Documentation web**

### Phase 5 — Audio et multilingue

18. **Lecture audio**
19. **Système de commentaires multilingues**
20. **Sélection de langue au runtime**

### Phase 6 — Exports

21. **Export MP4** (avec ffmpeg)
22. **Export HTML autonome**
23. **Mode podcast (TTS)**

### Phase 7 — Écosystème

24. **Packages communautaires** — figures réutilisables
25. **Éditeur web style Overleaf**
26. **Site officiel et galerie d'exemples**

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
- Mots-clés `defstyle`, `deffigure`, `defgroup` (renommage)
- Documentation restructurée en trois fichiers (SPEC, ARCHITECTURE, DECISIONS)
- Clarification du modèle compilateur = renderer
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