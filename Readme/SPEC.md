# Didact — Spécification du langage

**Version 0.2.** Référence complète : tout ce qui est écrit ici est implémenté. Fiche d'une page : [REFERENCE.md](REFERENCE.md). Choix de conception : [DECISIONS.md](DECISIONS.md).

1. [Fichier](#1-fichier) · 2. [`[config]`](#2-config) · 3. [`[figures]`](#3-figures) · 4. [Valeurs](#4-valeurs) · 5. [Propriétés communes](#5-propriétés-communes) · 6. [Figures 2D](#6-figures-2d) · 7. [Positions dynamiques](#7-positions-dynamiques) · 8. [`[timeline]`](#8-timeline) · 9. [Animations](#9-animations) · 10. [3D](#10-3d) · 11. [Outils](#11-outils) · 12. [Recettes](#12-recettes) · 13. [Limites](#13-limites)

---

## 1. Fichier

Un fichier `.dct` contient trois sections, dans cet ordre :

```
[config]
window : [16:9]

[figures]
deffigure titre =
    type    : text
    content : "Bonjour"
    pos     : (50%, 80%)

[timeline]
0.000 => { start : [titre fadein(1s)] }
3.000 => { end : [titre fadeout(0.5s)] }
```

- `=` définit, `:` donne une propriété, `=>` marque un instant. `#` commente jusqu'à la fin de la ligne (sauf dans une couleur `#FF0000`).
- Une propriété par ligne, indentée sous sa définition. Noms sensibles à la casse : lettres, chiffres, `_` ; les noms des fonctions de position (`center`, `top`, `left`, `below`, ... §7) sont réservés.
- **Ordre :** on ne référence que ce qui est défini **plus haut** (style, figure, paramètre, fonction, modèle). Pas de cycle possible.
- **Erreurs :** tout nom inconnu (type, propriété, animation, paramètre d'animation) est une erreur avec suggestion (`colour` → « vouliez-vous dire `color` ? »). Les erreurs donnent la ligne.

## 2. `[config]`

| Propriété | Valeurs | Défaut |
|---|---|---|
| `window` | `[16:9]` (ratio, taille adaptée, bandes si besoin), `[1920, 1080]` (taille fixe), `auto` (remplit le conteneur) | `[16:9]` |
| `background` | couleur | `white` |
| `languages` | `[FR, EN, ...]` : ordre des traductions dans `comment` | `[FR]` |
| `default_lang` | langue jouée sans `--lang` | `FR` |
| `decimal` | `comma` : les nombres affichés (`#{...}`, graduations) s'écrivent `2,5` ; `point` | `point` |

Une propriété inconnue est une erreur.

## 3. `[figures]`

| Mot-clé | Définit |
|---|---|
| `deffigure nom =` | une figure (§6, §10) |
| `defstyle nom =` | un style |
| `defgroup nom =` | un groupe |
| `defparam nom = valeur` | un paramètre animable |
| `deffunction f(x, y) = formule` | une fonction |
| `defmodel nom(a, b) =` / `defrepeat` | un modèle de figure, une répétition |

### 3.1 Styles

```
defstyle base =
    stroke : 2px
defstyle rouge =
    extends : base        # hérite, puis surcharge
    color   : red

deffigure c =
    type  : circle
    style : rouge
    color : blue          # la figure surcharge le style
```

Un style accepte toute propriété connue d'au moins un type.

### 3.2 Paramètres

```
defparam k = 1
...
2.000 => { anim : [k to(3, 2s)] }      # k va de sa valeur actuelle à 3, vitesse constante
```

- Utilisables partout où un nombre est attendu, avec son unité : `radius : (2*k)%`, `stroke : (k+1)px`, `opacity : k/4`, `rotate : 30k`, `pos : (k*10%, 50%)`, `matrix : (1, k, 0, 1)`, coordonnées `r(a, sin(a))`, `move(to=...)`, `fill : hsl(360*k, 80%, 50%)`, formules (`expr`, ...), `#{k}` dans un texte (§4.6).
- Seule l'animation `to(valeur, durée)` les change ; ils gardent leur valeur ensuite. `start`/`end`/`destroy` ne les concernent pas.
- Nom : pas un nom réservé des formules (`x`, `e`, `pi`, `sin`, ...).

### 3.3 Fonctions

```
deffunction f(x) = 0.5*x^2 - 1
deffunction g(x, y) = sin(x)*cos(y) + k     # k : un paramètre
```

Une ligne, au moins un argument (pour une constante : `defparam c = 2`). Utilisable dans toutes les formules qui suivent. Le corps n'utilise que ses arguments, les paramètres et les fonctions définies plus haut (pas de récursion). Le nom ne peut pas être celui d'une fonction prédéfinie, d'une figure, d'un style ou d'un paramètre.

### 3.4 Modèles et répétitions

```
defmodel approx(formule, couleur) =
    type  : plot
    axes  : r
    expr  : @formule
    color : @couleur

deffigure t1 = approx(x, #DC2626)
deffigure t3 = approx("x - x^3/6", #EA580C)
defrepeat barres = barre(@i, 0.5) for i in 0..7
```

- `@nom` : l'argument, recopié tel quel partout dans le modèle (aussi dans une chaîne, une formule, ou collé à une unité : `@x%`) ; `@(calcul)` : le résultat (arguments numériques, mêmes opérations que les formules).
- Un argument avec une virgule hors parenthèses ou commençant par une espace s'écrit entre guillemets (retirés).
- `defrepeat` crée `barres_0` … `barres_7` (bornes incluses, entiers ≥ 0, 1000 au plus) et le groupe `barres`.
- Les erreurs d'une figure créée par un modèle indiquent la ligne de l'appel.

### 3.5 Groupes

```
defgroup chapitre1 =
    items : [repere, courbe, titre]     # figures ou groupes définis avant
    offset : (10%, 0%)                  # décalage de tout le groupe (facultatif)
```

- `start` / `end` / `destroy` d'un groupe s'appliquent à toutes ses figures (une figure déjà cachée ne réapparaît pas pour un `end`).
- Les animations s'appliquent à l'ensemble, autour du centre des figures affichées ; `draw`/`write` les révèlent l'une après l'autre. Pas de `transform` sur un groupe.
- `center(groupe)` = centre de l'ensemble ; une figure définie après le groupe qui référence une de ses figures suit les déplacements du groupe. Une figure appartient à un seul groupe.

## 4. Valeurs

### 4.1 Positions

- `(x%, y%)` : % de la largeur et de la hauteur, **origine en bas à gauche**, y vers le haut. Les % négatifs sont permis.
- `r(x, y)` : point du repère `r` (une figure `axes`), coordonnées mathématiques (paramètres permis).
- Positions dynamiques : §7. Pas de position en px (refusée).

### 4.2 Tailles et unités

| Unité | Usage |
|---|---|
| `%` | positions (de la largeur/hauteur) ; **tailles** (`radius`, `size`, `width`, `height`, `cell`...) en % du **plus petit côté** : `width = height` donne un carré sur tout écran |
| `px` | épaisseurs, polices, petites tailles : px d'une scène de 1080 px de haut (ou de `window`), mis à l'échelle |
| `deg` (défaut), `rad` | angles ; positif = sens trigonométrique |
| `s`, `ms` | durées |

`cm`, `mm`, `pt`, `em` sont refusés. La géométrie exacte (angles, droites qui se rejoignent) se fait dans un repère `axes` (échelle égale en x et y par défaut) ou avec les positions dynamiques : en % de l'écran, `(0%,0%)→(10%,10%)` ne fait pas 45° en 16:9.

### 4.3 Couleurs

Noms : `black white red green blue yellow orange purple pink brown gray` (ou `grey`) `gold cyan navy` ; `#RGB`, `#RRGGBB`, `#RRGGBBAA` ; `none` ; `hsl(teinte, s%, l%)` ou `hsl(t, s%, l%, opacité)` (teinte en degrés, opacité 0–1 ou %, paramètres permis).

### 4.4 Textes et blocs

- Chaîne : `"..."` (`\"` pour un guillemet à l'intérieur). Listes : `[a, b, c]`. Couples : `(a, b)`. Booléens : `true`, `false`. Un élément peut être un calcul : `xrange : [0, 2*pi]`, `center : (sqrt(2), 1/3)`.
- `expr`, `polar`, `part`, `upper`, `lower`, `where`, `cmap`, `domain`, `escape`, `z0`, `x`, `y`, `sample`, `edges`, `values`, `weights` sont lues **telles quelles jusqu'à la fin de la ligne** (pas de guillemets : `expr : |x| + 1`).
- `|` **seul** après `:` ouvre un bloc multiligne : les lignes suivantes plus indentées, retours à la ligne gardés.
- Commentaire de fin de ligne : ` # ...` (aussi après une valeur lue telle quelle ; `#{...}` n'est pas un commentaire).

### 4.5 Formules

Utilisées par `expr`, coordonnées, paramètres, `deffunction`, etc.

- Variables selon le contexte (`x`, `y`, `z`, `t`, `k`, `u`, `v`, `n`), paramètres, constantes `pi`, `e`. `+ - * / ^`, multiplication implicite (`2x`, `3(x+1)`, `2sin(x)`).
- `sin cos tan asin acos atan sinh cosh tanh exp ln log sqrt abs floor ceil round sign` (`ln` népérien, `log` en base 10, angles en radians) ; `min max` (n arguments), `atan2(y, x)`, `mod(a, b)` (≥ 0), `hypot`, `clamp(v, a, b)`.
- `< > <= >= == !=` (1 ou 0), `and or not`, `if(c, a, b)`.
- `sum(k, 1, n, terme)`, `prod(j, 1, k, terme)` (imbricables, 100 000 termes au plus). Borne non entière animée : le dernier terme compte en partie (apparition douce).
- `fact(n)`, `binom(n, k)`, `gamma`, `erf` ; hasard reproductible `rand(k)` (uniforme [0, 1[), `randn(k)` (normale), graine facultative `rand(k, 7)`.
- Complexes (`cmap`, `domain`, `escape`) : `z`, `i`, `re im arg conj abs exp ln sqrt sin cos ...`.

### 4.6 Nombres dans le texte

Dans `content`, l'`expr` d'une `equation`, `rows`, `values`, étiquettes : `#{expr}` est remplacé à chaque image par la valeur arrondie : `#{k}` au plus 2 décimales (`2.5`), `#{k:0}` entier, `#{k:3}` exactement 3 décimales (`2.500`). `expr` est une formule (paramètres, fonctions). Avec `decimal : comma` dans `[config]` : `2,5`.

### 4.7 LaTeX

Formules (`equation`), `$...$` dans les textes, blocs `latex`. LaTeX mathématique courant : fractions, racines, sommes, matrices, `cases`, `\partial \nabla \langle\rangle \cap \hbar \Re \Im \mathbb \mathcal \mathscr \boxed \underbrace \cancel`, `\not\in`. Virgule décimale : `2{,}5`. `|x|` et `\|x\|` sont appariés automatiquement (une barre seule reste une barre). Couleurs : `\textcolor{red}{x}`, `\color{blue}`, hexadécimal `\textcolor{22C55E}{x}`. Texte : `\textbf`, `\textit`, `\emph`. Police : Latin Modern.

## 5. Propriétés communes

| Propriété | Rôle | Défaut |
|---|---|---|
| `color` | trait, texte | `black` |
| `fill` | remplissage | `none` |
| `opacity` | 0 à 1 | 1 |
| `stroke` | épaisseur du trait | `2px` |
| `style` | style appliqué | — |
| `layer` | ordre de dessin (petit = dessous ; égal : ordre du fichier) | 0 |
| `rotate` | rotation fixe : `30`, `0.5rad`, `auto_tangent(courbe)` | 0 |
| `glow` | halo lumineux de ce rayon (`10px`) | — |
| `allow` | `outside`, `overlap` ou les deux : avertissements de `--check` voulus | — |
| `clip` | `false` : pas coupée au cadre de son repère | `true` |
| `axes` | repère de la figure (coordonnées, découpe) | — |
| `matrix`, `map`, `cmap`, `amount` | déformation (§6.7) | — |
| `anchor` | point posé sur `pos` : `topleft top topright left center right bottomleft bottom bottomright` ; types : `rect`, `text`, `equation`, `latex`, `raw.latex`, `table`, `graph`, `array`, `image` (la boîte englobante est ancrée) | `center` |

## 6. Figures 2D

Propriété obligatoire en **gras**.

### 6.1 Géométrie

| Type | Propriétés |
|---|---|
| `line` | **`from`**, **`to`**, `arrow : none \| start \| end \| both`, `extend : true \| start \| end` (droite ou demi-droite passant par `from` et `to`, prolongée jusqu'au bord de l'écran, ou du repère avec `axes`) |
| `circle` | **`center`**, **`radius`** (`2%`, `10px`) ; ellipse `radius : (rx, ry)` ; avec `axes : r`, un rayon sans unité est en unités du repère (sans `axes`, l'unité est obligatoire) |
| `rect` | **`pos`** (centre), **`width`**, **`height`**, `corner_radius` |
| `polygon` | **`points : [(x%, y%), r(1, 2), ...]`**, `closed : false` (ligne brisée, 2 points suffisent) ; ou polygone régulier : **`sides`**, **`center`**, **`radius`** (premier sommet en haut ; `rotate` pour le tourner) |
| `brace` | accolade : **`from`**, **`to`**, `size` (profondeur, 2%) ; bombée à droite du sens `from` → `to` (donc sous un segment écrit de gauche à droite ; inverser `from` et `to` pour l'autre côté). Étiquette : `pos : offset(accolade, 3%)` |
| `arc` | **`center`**, **`radius`** (comme `circle` : sans unité = unités du repère avec `axes`), **`from_angle`**, **`to_angle`** (sens trigonométrique si `to > from`), `arrow` |
| `star` | **`pos`**, **`size`** (rayon), `points` (5), `inner` (rapport des rayons, 0.382) |
| `arrow` | **`from`**, **`to`**, `head` (longueur de la pointe, px) ; points en % ou dans un repère (`axes`) |
| `image` | **`file`** (.png ou .svg, relatif au .dct), **`pos`**, **`size`** (plus grand côté). Proportions gardées, SVG net à toute taille. Animations : fondu, `move`, `scale`, `grow`, `rotate` ; `draw`/`write` = fondu ; pas de `transform`, `color(part=)` ni déformation. Sur le web, le fichier est lu à côté du .dct ; `--export html` l'inclut |

### 6.2 Texte et maths

| Type | Propriétés |
|---|---|
| `text` | **`content`** (texte, `$maths$` ; plusieurs lignes : bloc `|`), **`pos`**, `size` (32px), `align` |
| `equation` | **`expr`** (LaTeX mathématique, jusqu'à la fin de la ligne), **`pos`**, `size` (40px), `align` |
| `latex` / `raw.latex` | **`content`** / **`code`** : LaTeX en mode texte (`align`, `itemize`, paragraphes ; `\\` pour aller à la ligne), **`pos`**, `size` (32px), `align` |
| `table` | **`rows`** (bloc `|` : une ligne par rangée, cases séparées par `\|`), `header : ["$x$", "$f(x)$"]`, **`pos`**, `size`, `align` |

- `align : left | center | right` : `pos` est le bord gauche, le centre ou le bord droit du bloc, et les lignes s'alignent de même (listes toujours à gauche). Avec `left`, une formule qui s'allonge par `transform` ne bouge pas à gauche.
- Les textes sont centrés sur `pos`, verticalement aussi (ou selon `anchor`). Pour des textes de longueurs différentes au même endroit : `anchor : top` (ou `topleft`) garde la première ligne à la même hauteur.

```
deffigure etape =
    type    : text
    content : |
        Première ligne
        Deuxième ligne avec $x^2$
    pos     : (75%, 60%)
    anchor  : top
```

### 6.3 Repères et courbes

```
deffigure r =
    type   : axes
    from   : (10%, 10%)        # coin bas-gauche de la boîte
    to     : (90%, 90%)        # coin haut-droit
    xrange : [-5, 5]
    yrange : [-2, 2]
```

| Type | Propriétés |
|---|---|
| `axes` | **`from`**, **`to`**, **`xrange`**, **`yrange`**, `equal` (true : même échelle en x et y, centré dans la boîte ; false : remplit la boîte), `grid` (false), `ticks` (true), `step` (`auto`, `0.5`, `pi/2`, `(pi/2, 0.5)` ; un pas en π écrit les graduations en π), `xlabel`, `ylabel` (`"$x$"`), `center : (x, y)` et `zoom` (caméra 2D, paramètres permis), `align_x : r2` (mêmes abscisses à l'écran que `r2`), `align_y : r2`, `same_scale : r2` (mêmes pixels par unité que `r2` : la taille du repère dépend alors de ses `xrange`/`yrange`, pas de sa boîte, où il est centré) |
| `plot` | **`axes`**, **`expr`** (en `x`), `xrange : [a, b]` (partie tracée ; `[0, T]` animé : la courbe se dessine), `samples` (n points réguliers ; défaut : adaptatif) |
| `curve` | **`axes`** ; **`expr : (x(t), y(t))`** + **`t : [a, b]`**, ou **`polar : r(t)`** (`t` défaut [0, 2π]) ; `samples` (1000) |
| `tangent` | **`axes`**, **`expr`** (en `x`), **`at`** (abscisse du contact), `length` (longueur totale du segment, en unités du repère, 3) |
| `riemann` | **`axes`**, **`expr`**, **`xrange`**, `n` (10 ; animé, les rectangles se fondent d'un nombre au suivant), `method : left \| mid \| right` ; remplissage par défaut : `color` transparent |
| `grid` | **`axes`**, `step` (`auto` : celui des graduations ; `(1, 0.5)`), `xrange`, `yrange` : droites x = k·pas, y = k·pas (faites pour être déformées) |
| `scatter` | **`axes`**, **`points : [(1, 2), ...]`** ou **`count`** + **`x`**, **`y`** (en `k` = 1..n, ex. `rand(2k)`) + `where` (filtre en `x`, `y` du point, ex. `x^2 + y^2 <= 1`) ; `size` (rayon, 6px), `marker : circle \| square \| diamond \| triangle \| cross \| plus` ; couleur : `fill` sinon `color` |
| `histogram` | **`axes`**, **`data : [...]`** ou **`count`** + **`sample`** (en `k`) ; `bins` (10), `range : [a, b]` (classes fixes, sinon min et max des données), `density : true` (aire 1) ; couleur : `fill` sinon `color` |

- Une figure avec `axes : r` est coupée au cadre du repère (`clip : false` pour l'empêcher).
- Un repère sert de système de coordonnées même s'il n'est jamais démarré (repère invisible).
- Avec `equal : true` (défaut), le repère peut être plus petit que sa boîte (il garde la même échelle en x et y) ; `equal : false` remplit la boîte.
- `plot` est coupé hors de `yrange` et là où `expr` n'est pas définie.

### 6.4 Champs, niveaux, régions, pixels

| Type | Propriétés |
|---|---|
| `contour` | **`axes`**, **`expr`** : f(x, y) (courbes de niveau) ou `g = h` (courbe implicite) ; `levels` (10, ou liste ; `[0]` pour une équation), `colormap`, `resolution` (160), `xrange`, `yrange` |
| `region` | **`axes`** ; **`upper`** (+ `lower`, défaut 0, + `xrange`, bornes animables) : entre deux courbes ; ou **`where`** : inégalités en x, y (`and(...)`, `or(...)`, `not(...)`), `xrange`, `yrange`, `resolution` ; `fill` (défaut bleu translucide), bord tracé si `color` est donnée |
| `field` | **`axes`**, **`expr : (fx, fy)`** (en x, y), `density` (flèches par axe : `12` ou `(12, 8)`, défaut 12), `length` (part de la case, 0.85), `normalize : true` (même longueur), `colormap` (selon l'intensité), `xrange`, `yrange` |
| `pixels` | **`axes`** et une des trois : **`expr`** (valeur → `colormap`, `crange`), **`domain`** (coloration de domaine : teinte = argument, rouge = réel positif, zéros noirs, pôles blancs ; `bands : false` enlève les bandes du module), **`escape`** (fractale z ← f(z), `c` = point du pixel, `z0` (0 ; `z0 : c` pour Julia), `iterations` (100), `bailout` (2), `inside` (noir), `colormap` (magma)) ; `resolution` (1 = un calcul par pixel, 0.5 = quatre fois moins) |
| `trajectory` | **`axes`** (ou `scene`), **`ode : (x', y')`** (en x, y, z, `t`, paramètres), **`init`**, **`duration`** (t de 0 à duration ; animé : la courbe s'allonge) ; `vars : (theta, omega)` (1 à 6 noms ; les 2 ou 3 premiers sont dessinés), `dt` (0.01, Runge-Kutta 4), `count` (n trajectoires, `init` en `k`), `trail` (seulement la dernière durée), `head` (rayon du point courant, px), `colormap` (une couleur par trajectoire), `tube` |
| `iterate` | **`axes`** (ou `scene`), `steps` (`draw(d)` fait apparaître les pas un à un) ; récurrence : `vars : (x, y)`, `init : (2, 0)`, `update : (x - 0.1*y, y + 0.1*x)` (calculé avec les anciennes valeurs ; `n` = numéro du pas ; avec `on : f` aussi `f dx dy hxx hxy hyy`) ; optimiseurs : `on : f` (une `surface` ou un `plot`), `start : (x, y)`, `method : gd \| momentum \| nesterov \| rmsprop \| adam \| newton`, `rate`, `beta`, `beta2`, `eps` ; dessin : `markers` (rayon des points, 4px), `fill` (points), `color` (ligne ; `none`), `lift` (hauteur au-dessus de la surface, en unités de z), `drape : true` (suit la surface entre deux pas) |

```
deffigure niveaux =
    type   : contour
    axes   : r
    expr   : x^2 + 2y^2          # ou : (x^2 + y^2)^2 = 4*(x^2 - y^2)
    levels : [0.5, 1, 2]         # ou 14
deffigure aire =
    type   : region
    axes   : r
    upper  : f(x)
    xrange : [0, b]
deffigure mandelbrot =
    type   : pixels
    axes   : r
    escape : z^2 + c
deffigure pendule =
    type     : trajectory
    axes     : r
    ode      : (y, -sin(x) - 0.25*y)
    init     : (-4 + 8*rand(k), -3 + 6*rand(k, 1))
    count    : 50
    duration : T
```

Palettes (`colormap`) : `viridis plasma inferno magma coolwarm turbo blues gray` ou `[#000, #FFF]`.

### 6.5 Graphes et tableaux

| Type | Propriétés |
|---|---|
| `graph` | **`nodes : [A, B, C]`**, **`edges : [A-B, B->C]`** (`->` orientée ; `A->A` : boucle au-dessus du nœud ; `A->B` et `B->A` : deux arêtes courbes), `initial : A` (flèche d'entrée, automate), `accepting : [C]` (double cercle), **`pos`** (centre), `size` (étendue, 60%), `weights : [4, 2, "a,b"]` (étiquettes des arêtes, dans l'ordre ; guillemets pour une virgule), `layout : circle \| grid \| tree \| force` (circle), `root` (racine de `tree`, défaut le 1er nœud), `positions : [(0, 1), (1, 2), ...]` (ramenées dans `size`), `radius` (nœuds, 3.5%), `fill` (nœuds, blanc), `labels : false \| [...]`, `label_color`, `font` |
| `array` | **`values : [[1, 2], [3, $\frac{1}{2}$]]`** (ou une ligne `[1, 2, 3]` ; `$...$` et `#{...}` permis), **`pos`** (centre), `cell` (côté d'une case, 9%), `brackets : true` (crochets de matrice), `grid` (traits entre cases, true), `grid_color`, `font` ; `color` = texte, `fill` = fond des cases (transparent) |

Exemples : Dijkstra / Bellman-Ford (graphe orienté pondéré, `color(part="A->C")`, distances en textes `pos : above(g, 1%, part="B")`), automate (`initial`, `accepting`, boucles, étiquettes `"a,b"`), réseau de neurones (`positions` par couche, `labels : ["$x_1$", ...]`, arêtes `x1->h1`).

Morceaux pour `color(to=..., part=...)` : nœud `B`, arête `"A-C"`, case `"(2,3)"` (ligne, colonne, à partir de 1), `"row 2"`, `"col 3"` (le fond se colore) ; `"text (2,3)"`, `"text row 2"`, `"text col 1"` (le texte).

### 6.6 Cadre

| Type | Propriétés |
|---|---|
| `frame` | **`around`** (figure entourée, suivie si elle bouge), `part` (seulement ce morceau : d'un texte, d'une formule, nœud d'un graph, case d'un array), `padding` (1%), `corner_radius` |

### 6.7 Déformations

Sur toute figure 2D dans un repère et tout objet 3D, une seule par figure, dans les coordonnées du repère :

- `matrix : (a, b, c, d)` : (x, y) → (ax + by, cx + dy) ; 9 nombres en 3D.
- `map : (u, v)` en x, y (`(u, v, w)` en 3D).
- `cmap : f(z)` : application complexe (2D).
- `amount` (1) : 0 = d'origine, 1 = transformée ; l'animer déforme progressivement.

Les traits sont redécoupés avant transformation (une droite devient courbe).

## 7. Positions dynamiques

Recalculées à chaque image : ce qui en dépend suit.

| Écriture | Point |
|---|---|
| `r(x, y)` | point du repère `r` |
| `center(f)` | centre de la figure ou du groupe |
| `top(f)`, `bottom(f)`, `left(f)`, `right(f)`, `topleft(f)`, ..., `bottomright(f)` | milieu d'un bord, coin de sa boîte |
| `center(eq, part="\frac{a}{b}")`, `top(eq, part="x^2")` | idem pour un morceau d'un texte ou d'une formule (1re occurrence), un nœud ou une arête d'un `graph` (`part="B"`, `part="A->B"`), une case d'un `array` (`part="(1,2)"`) |
| `below(f, 3%)`, `above(f, 2%, left)` | sous / au-dessus de `f`, à cet écart, alignement `left \| center \| right` |
| `left_of(f, 1%)`, `right_of(f, 1%, top)` | à gauche / à droite, alignement `top \| center \| bottom` |
| `offset(f, d)` | place d'une étiquette, à la distance `d` : au-delà de la pointe d'une `line` ou d'une `arrow` (même déformée par `matrix`), à l'extérieur du milieu d'un `arc`, sous la pointe d'une `brace` ; pour les autres figures, au coin haut-droit de la boîte |
| `start(f)`, `end(f)` | premier et dernier point du tracé (ligne, flèche, courbe, arc) |
| `vertex(p, i)`, `side(p, i)` | sommet n° i d'un polygone (à partir de 1, dans l'ordre de `points`), milieu du côté qui va du sommet i au suivant ; suivent le polygone quand il bouge |
| `angle(f)` | angle d'une figure (direction d'une `line`), pour `rotate`, `from_angle`, `to_angle` |
| `auto_tangent(c)` | angle de la tangente à `c` au point le plus proche, pour `rotate` |

`below`/`above`/`left_of`/`right_of` acceptent `part=...` et donnent un `anchor` implicite à la figure placée (`below` → son haut, `right_of` → sa gauche, ...), sauf `anchor` explicite.

## 8. `[timeline]`

```
5.000 => {
    destroy : [d]
    end     : [c fadeout(0.5s)]
    start   : [a, b fadein(1s), audio musique.mp3 loop volume=0.3]
    anim    : [a move(to=(80%, 50%), 2s), k to(3, 2s)]
    comment : ["Texte FR", "Text EN"]
}
```

- Instant en `secondes.millisecondes` ; chaque instant une seule fois, blocs écrits dans l'ordre du temps. Pour insérer une étape : `didact shift f.dct 16 +2` décale tous les blocs à partir de 16 s.
- Dans un bloc, l'ordre est toujours `destroy → end → start → anim → comment`, quel que soit l'ordre d'écriture.
- `start` : affiche (avec ou sans animation d'entrée). `end` : cache (la figure reste affichée pendant son animation de sortie et peut revenir). `destroy` : supprime ; interdit si une autre figure la référence encore.
- Les figures ne sont visibles qu'entre `start` et `end`. Une figure référencée par une autre (`center(a)`, `transform(to=b)`) est calculée même cachée.
- Dans une liste, éléments séparés par des virgules : **en parallèle**. Crochets sur une figure : **à la suite** : `anim : [f [move(to=(50%,50%), 1s), color(to=blue, 0.5s)]]`.
- Les sous-titres occupent le bas de l'image (jusqu'à environ 12 % de la hauteur pour une ligne) : garder cette zone libre.
- `comment` : une traduction par langue de `languages`, en liste (`["Texte FR", "Text EN"]`) ; avec une seule langue, `comment : "Texte"` suffit (`$...$` permis) ; `comment : none` efface le sous-titre. Il reste affiché jusqu'au `comment` suivant.
- **Audio :** `start : [audio fichier.mp3 volume=0.3 loop]` (mp3, wav, ogg, flac, m4a ; chemin relatif au .dct) ; joue jusqu'à `end : [audio fichier.mp3]` ou la fin. Suit pause et retours en arrière ; mixé dans les exports.

## 9. Animations

Durée obligatoire (`2s`, `500ms` ou `duration=2s`). Easing : `ease=smooth` (défaut), `linear` (défaut de `to`), `ease_in`, `ease_out`. Une valeur est un nombre ou un calcul de constantes (`to(2*pi, 3s)`, `rotate(-pi/2 rad, 1s)`). Une animation terminée garde son effet.

| Animation | Effet |
|---|---|
| `fadein(d)` / `fadeout(d)` | opacité 0 → 1 / 1 → 0 |
| `grow(d)` / `shrink(d)` | taille 0 → 1 / 1 → 0 autour du centre |
| `draw(d)` | le contour se trace puis se remplit |
| `write(d)` | pareil, morceau par morceau (lettre après lettre) |
| `slidein(dir, d)` / `slideout(dir, d)` | glissé de 12 % + fondu ; `slidein(left)` arrive depuis la gauche ; dir : `left right up down` (`top`, `bottom`) |
| `move(to=pos, d)` | va vers `pos` (toute position, §4.1, §7), en ligne droite : le point posé sur `pos` est celui de sa propriété `pos` (avec `anchor : left`, le bord gauche), sinon le centre |
| `move(dir, 20%, d)` | déplacement relatif (% de la largeur ou hauteur) |
| `follow(path=c, d, rotate=tangent)` | le centre parcourt tout le contour de `c` à vitesse régulière ; `rotate=tangent` l'oriente |
| `rotate(angle, d)` | tourne **de** `angle` autour du centre (s'additionne) |
| `rotate(angle, d, around=pos)` | tourne autour du point `pos` (un point fixe : `(30%, 40%)`, `r(1, 2)`, un point d'une autre figure) |
| `scale(f, d)` | taille → f × la taille d'origine |
| `swap("(1,2)", "(1,3)", d)` | sur un `array` : les contenus de deux cases s'échangent (`swap(2, 3, d)` sur une seule ligne) ; les échanges s'enchaînent (tri) |
| `color(to=c, d)` | couleur du trait (et du texte) |
| `color(to=c, d, part="...")` | seulement ce morceau : LaTeX dans une équation, texte dans un `text`, morceau d'un `graph`/`array` (§6.5) ; toutes les occurrences |
| `fill(to=c, d)` | couleur de remplissage (part de `fill` ; depuis `none`, la couleur apparaît en fondu) |
| `transform(to=b, d)` | se déforme en `b` (forme, position, couleurs) puis `b` la remplace ; `b` doit être définie, pas forcément démarrée ; entre textes/équations : lettre par lettre (les lettres communes glissent, les autres en fondu) |
| `transform(to=b, d, rigid=true)` | glisse et tourne en bloc jusqu'à `b` (même forme ; les sommets sont appariés automatiquement, retournement si besoin) ; le centre va en ligne droite |
| `to(v, d)` | un **paramètre** va vers `v` |
| `rotate3d(angle, axis=x, d)` | 3D (§10) |

Après `transform(to=b)`, `b` est affichée à sa propre place et peut se transformer à son tour : une chaîne `e1 → e2 → e3` (une figure par étape) déroule un calcul.

Deux animations qui fixent la même chose en même temps sur une figure (`move`/`follow`/`transform`, `scale`, `fill`, `color` du même morceau) sont une erreur : les enchaîner avec `[ ... ]`. `rotate` et `move(dir, d)` s'additionnent.

Un nouveau `start` annule les fondus, `grow`/`shrink` et `transform` antérieurs ; déplacements, rotations, échelles et couleurs restent. Sans animation, une figure apparaît et disparaît instantanément.

## 10. 3D

Une `scene3d` porte la caméra ; les objets s'y rattachent par `scene : nom`. Axe z vers le haut. Un objet n'est dessiné que s'il est démarré (`start`), le repère de la scène que si la scène l'est. Les objets sont découpés, projetés et triés du plus loin au plus proche ; les figures 2D restent par-dessus. Dans la fenêtre, la souris tourne la caméra (aperçu seulement).

```
deffigure espace =
    type   : scene3d
    camera : (30, theta)       # (élévation, azimut) en degrés, paramètres permis
    zrange : [0, 5]
```

| Type | Propriétés |
|---|---|
| `scene3d` | `xrange`, `yrange`, `zrange` ([-3, 3]), `camera : (élévation, azimut)` en degrés ((30, -60)), `projection : perspective \| ortho`, `fov` (30°), `zoom` (1), `target : (x, y, z)` (centre de la boîte), `equal` (true : unités réelles ; false : proportions `box : (1, 1, 0.8)`), `light : (x, y, z)` (direction ; défaut liée à la caméra), `ambient` (0.35), `frame : lines \| box \| none` (affichage du repère 3D : none ; `true` = lines), `floor : 8` (lignes de la grille au sol), `labels : ("x", "y", "z")` (LaTeX), `color` (repère), `from`, `to` (zone de l'écran, défaut tout) |
| `surface` | **`expr`** : z = f(x, y), ou `(x(u,v), y(u,v), z(u,v))` + **`urange`**, **`vrange`** ; `xrange`, `yrange` (ceux de la scène), `resolution` (48 ou `(nu, nv)`), `grid : (20, 20)` (lignes, 0), `colormap`, `color_by : x \| y \| z \| u \| v` (z), `crange : [a, b]`, `clip` (coupée à zrange), lignes de niveau : `levels` (n ou liste), `levels_on : surface \| floor \| both`, `level_color`, `level_stroke` (2px) |
| `curve3d` | **`expr : (x(t), y(t), z(t))`** + **`t : [a, b]`**, `samples` (200) ; ou **`points : [espace(0, 0, 0), f(1, 2), ...]`** ; `markers` (rayon des points, px) |
| `cube`, `box`, `plane` | `size` (cube : arête, 1 ; box : `(a, b, c)` ; plane : `(a, b)`, (2, 2)), `grid` (lignes sur les faces) |
| `sphere` ; `cylinder`, `cone` | `radius` (1) ; `radius`, `height` |
| `polyhedron` | **`points : [(x, y, z), ...]`**, **`faces : [(0, 1, 2), ...]`** |
| `arrow` | **`from`**, **`to`** (points 3D) : tige et pointe conique, `head` (longueur de la pointe, 0.2 × la flèche) |
| `field` | **`expr : (fx, fy, fz)`**, `density` (5 ou `(nx, ny, nz)`), `length` (0.9), `normalize`, `colormap`, `xrange`, `yrange`, `zrange` |
| `iterate`, `trajectory` | comme en 2D avec `scene` et 3 variables (x, y, z) ; sur une surface, les points sont posés dessus |

- Communes aux objets 3D : `scene`, `center : (x, y, z)`, `rotate : (rx, ry, rz)` (degrés, autour de x puis y puis z), `resolution` (finesse du découpage), `shading : flat \| none` (éclairage des faces), `tube : 6px` (lignes dessinées en tube éclairé), `glow`, `fill` (faces), `color` (arêtes, lignes), `stroke`, `opacity`, déformations (§6.7).

```
deffigure f =
    type     : surface
    scene    : espace
    expr     : sin(x)*cos(y)
    colormap : viridis
deffigure tore =
    type   : surface
    scene  : espace
    expr   : (cos(u)*(2 + cos(v)), sin(u)*(2 + cos(v)), sin(v))
    urange : [0, 2*pi]
    vrange : [0, 2*pi]
deffigure bille =
    type   : sphere
    scene  : espace
    center : f(1, 0.5)          # point de la surface f
    radius : 0.15
```
- Points 3D : `espace(x, y, z)`, `f(a, b)` (point de la surface `f`), `center(objet)`, `(x, y, z)` (scène de l'objet). Une figure 2D posée sur un point 3D suit la caméra.
- Animations : comme en 2D autour du centre ; `move(up, ...)` agit sur z ; `rotate` tourne autour de z ; `rotate3d(angle, axis=x | y | z | (a, b, c), d)` ; `draw` balaie ; `follow` suit une `curve3d`, une trajectoire ou une `arrow` ; `transform` entre surfaces se fait point par point. Sur la scène, `fadein`/`fadeout`/`draw` agissent sur son repère.

## 11. Outils

| Commande | Effet |
|---|---|
| `didact f.dct` | fenêtre (rechargée à chaque sauvegarde) |
| `--check [--format json]` | vérifie sans jouer : erreurs, figures hors du cadre, textes qui se chevauchent, remarques (figure/style/paramètre inutilisés). Retour 0 / 2 (avertissements) / 1 (erreurs) |
| `--png t img.png` | une image à l'instant t, avec le sous-titre de cet instant (`--no-subtitles` pour l'enlever) |
| `--export mp4` (`-o`, `--fps`, `--height`, `--no-subtitles`, `--ffmpeg`) | vidéo (ffmpeg) |
| `--export html` (`-o`, `--engine`) | page autonome |
| `--export audio` | son seul (MP3) |
| `--voice` | lit les `comment` par une voix de synthèse configurée dans `didact-voix.txt` (maths prononcées : « x au carré », « a sur b ») ; si une phrase est plus longue que l'écart avant le commentaire suivant, l'animation l'attend |
| `--lang XX`, `--summary`, `--debug` | langue, résumé, tokens et AST |
| `didact shift f.dct 16 +2` | décale tous les blocs à partir de 16 s |

JSON de `--check` : `{severity, code, line, time, figure, other, message}`, codes `parse validation render outside overlap unused_figure unused_style unused_param`.

`didact-voix.txt`, une ligne par langue : `FR : python -m piper -m fr_FR-siwis-medium -f {out}` (`{out}` obligatoire ; `{text}`, `{text_file}`, `{lang}` ; phrase aussi sur l'entrée standard). Fichiers dans `cours_voix/`, seuls les modifiés sont refaits.

## 12. Recettes

| Besoin | Écriture |
|---|---|
| Point mobile sur une courbe | `defparam a = 1` ; cercle `center : r(a, f(a))` ; `anim : [a to(3, 2s)]` |
| Sécante, droite par deux points | `line` avec `from : r(a, f(a))`, `to : r(a + h, f(a + h))`, `extend : true`, `axes : r` |
| Trace laissée par un point mobile | `curve` avec `expr : (x(t), y(t))` et `t : [0, T]`, `T` animé (même formule que le point) |
| Courbe qui se dessine jusqu'à un paramètre | `plot` avec `xrange : [0, T]` |
| Figure qui apparaît quand `n` dépasse 3 | `opacity : clamp(n - 3, 0, 1)` |
| Compteur, valeur en direct | `content : "n = #{round(n):0}"`, `expr : f'(a) = #{pente(a):2}` |
| Calcul étape par étape | une `equation` par étape (même `pos`, `align : left`) ; `e1 transform(to=e2, 1s)` puis `e2 transform(to=e3, 1s)` |
| Mettre un terme en valeur | `color(to=red, 1s, part="x^2")`, ou une figure `frame` avec `part` |
| Étiquette qui suit une figure | `pos : above(f, 1%)`, `offset(vecteur, 2%)`, `side(triangle, 1)` |
| Mesure (dx, longueur d'un côté) | `brace` + texte en `offset(accolade, 3%)` |
| Rotation du plan d'angle `th` | `matrix : (cos(th), -sin(th), sin(th), cos(th))` (dans un `defstyle` pour l'appliquer à plusieurs figures) ; donner au `grid` un `xrange`/`yrange` plus large que le repère pour qu'il couvre tout le cadre une fois tourné |
| Même déformation sur plusieurs figures | `defstyle d =` avec `matrix : ...` puis `style : d` sur chacune (un groupe ne porte pas de déformation) |
| Case avec son contenu qui bougent ensemble | un `rect`, puis un `text` avec `pos : center(le_rect)` ; animer le `rect` |
| Trier un tableau | `array` + `color(to=gold, 0.3s, part="(1,2)")` pour comparer, `swap(2, 3, 0.6s)` pour échanger |
| Échanger deux figures | deux `move(to=...)` croisés ; passer l'une par-dessus : `a [move(up, 8%, 0.3s), move(to=..., 0.6s)]` |
| Repère invisible | définir l'`axes` et ne pas le démarrer |
| Zoom sur une courbe | `zoom : z` et `center : (cx, cy)` sur l'`axes`, `z` animé |
| Fond sombre | `background : #0F172A` et `color : white` dans un style commun |

## 13. Limites

- `raw.matplotlib` est accepté par le validateur mais pas encore dessiné.
- Un fichier par projet (pas de fichiers `.cfg` / `.spc` / `.tl` séparés).
- Pas de boucle ni de variable dans `[timeline]` : un algorithme pas à pas s'écrit bloc par bloc.
- Le nombre de pas d'une trajectoire et la finesse d'un maillage sont fixés pour toute l'animation.
- 3D : deux grandes faces qui se coupent peuvent être mal ordonnées (augmenter `resolution`) ; pas d'ombres portées.
