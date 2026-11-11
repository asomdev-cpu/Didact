# Didact — Spécification du langage

**Version : 0.2 (draft)**

Document de référence pour écrire des fichiers Didact. Décrit uniquement la syntaxe et les comportements qui ont été **explicitement décidés**.

Les points encore à trancher sont regroupés dans la dernière section "À décider".

---

## Table des matières

1. [Introduction](#1-introduction)
2. [Structure d'un projet](#2-structure-dun-projet)
3. [Syntaxe générale](#3-syntaxe-générale)
4. [Section `[config]`](#4-section-config)
5. [Section `[figures]`](#5-section-figures)
6. [Section `[timeline]`](#6-section-timeline)
7. [Système de coordonnées](#7-système-de-coordonnées)
8. [Types de figures](#8-types-de-figures)
9. [Styles et héritage](#9-styles-et-héritage)
10. [Groupes](#10-groupes)
11. [Animations](#11-animations)
12. [Références dynamiques](#12-références-dynamiques)
13. [Audio et commentaires multilingues](#13-audio-et-commentaires-multilingues)
14. [Escape hatches : LaTeX et Matplotlib bruts](#14-escape-hatches--latex-et-matplotlib-bruts)
15. [3D, champs et récurrences](#15-3d-champs-et-récurrences)
16. [À décider](#16-à-décider)

---

## 1. Introduction

Didact est un langage déclaratif pour décrire des animations éducatives. Un fichier Didact décrit **quoi afficher** (figures) et **quand l'afficher** (timeline).

**Un fichier Didact minimal :**

```
[config]
window : [16:9]

[figures]

deffigure ma_ligne =
    type  : line
    from  : (20%, 50%)
    to    : (80%, 50%)
    color : red

[timeline]

0.000 => {
    start : [ma_ligne]
}

5.000 => {
    end : [ma_ligne]
}
```

---

## 2. Structure d'un projet

Un projet Didact peut être organisé de deux façons. **Pas de mélange possible** : soit tout dans un fichier, soit tout séparé.

### Mode A — fichier unique

```
mon_projet.dct
```

```
[config]
...

[figures]
...

[timeline]
...
```

### Mode B — fichiers séparés

```
mon_projet.dct.cfg    ←  contenu de [config]
mon_projet.dct.spc    ←  contenu de [figures]
mon_projet.dct.tl     ←  contenu de [timeline]
```

L'ordre des sections est obligatoire : `config`, `figures`, `timeline`.

---

## 3. Syntaxe générale

### 3.1 Les trois symboles fondamentaux

| Symbole | Rôle | Exemple |
|---|---|---|
| `=` | Définition d'un objet | `defstyle trait_fin =` |
| `:` | Assignation d'une propriété | `color : red` |
| `=>` | Moment dans le temps | `5.000 => { ... }` |

### 3.2 Commentaires

```
# Tout ce qui suit le # jusqu'à la fin de la ligne est ignoré
```

### 3.3 Sections

Un fichier `.dct` (mode unique) contient trois sections marquées par des headers :

```
[config]
[figures]
[timeline]
```

### 3.4 Sensibilité à la casse

Les noms sont sensibles à la casse : `Ma_Ligne` et `ma_ligne` sont deux éléments distincts.

### 3.5 Ordre de définition

Un style, une figure ou un groupe ne peut référencer qu'un élément défini **plus haut** dans le fichier. Une référence vers un élément défini plus bas produit une erreur de compilation.

```
deffigure a =
    type : line
    from : center(b)     # ✗ erreur : b est défini plus bas

deffigure b =
    type   : circle
    center : (50%, 50%)
    radius : 10%
```

Conséquence : les références circulaires (A dépend de B qui dépend de A) sont impossibles par construction.

---

## 4. Section `[config]`

### 4.1 Propriétés

```
[config]
window       : [16:9] / [1920, 1080] / auto
background   : white
languages    : [FR, NL, EN]
default_lang : FR
```

### 4.2 La propriété `window`

```
window : [16:9]            # ratio fixe, taille adaptative (par défaut)
window : [1920, 1080]      # taille fixe en pixels
window : auto              # s'adapte au conteneur, sans ratio fixe
```

Avec un ratio fixe, la scène garde ses proportions et s'adapte en taille, avec des bandes si besoin (comme une vidéo). Tout le monde voit la même composition. Avec `auto`, la composition change selon l'écran du spectateur : à réserver aux cas où c'est voulu.

### 4.3 Langues

L'ordre de la liste `languages` définit l'ordre des traductions partout dans le projet :

```
languages : [FR, NL, EN]
# → index 0 = FR, index 1 = NL, index 2 = EN
```

Cet ordre est réutilisé dans les `comment` de la timeline :

```
comment : ["Bonjour", "Hallo", "Hello"]
#          ↑ FR      ↑ NL     ↑ EN
```

---

## 5. Section `[figures]`

Définit les objets visuels et les styles réutilisables. **Aucune notion de temps ici.**

### 5.1 Les trois mots-clés de définition

| Mot-clé | Définit |
|---|---|
| `defstyle` | Un style réutilisable |
| `deffigure` | Une figure (objet visuel) |
| `defgroup` | Un groupe de figures |
| `defparam` | Un paramètre : un nombre qui évolue dans le temps (§12) |
| `defmodel` | Un modèle de figure, avec des arguments (§5.2) |
| `defrepeat` | Plusieurs figures créées à partir d'un modèle, plus leur groupe (§5.2) |

Le préfixe `def` évite l'ambiguïté avec les noms de propriétés :

```
deffigure ma_ligne =
    style : trait_fin        # ← "style" est ici une propriété

defstyle trait_fin =         # ← "defstyle" est un mot-clé de définition
    color : red
```

### 5.2 Modèles et répétitions

Un **modèle** évite de réécrire des figures presque identiques. Ses lignes sont indentées sous `defmodel`. Dans ces lignes, `@nom` est remplacé par l'argument, et `@(calcul)` par le résultat du calcul (les arguments doivent alors être des nombres ; mêmes opérations que `expr`).

```
defmodel approx(formule, couleur) =
    type  : plot
    expr  : @formule
    axes  : repere
    color : @couleur
    style : courbe

deffigure t1 = approx(x, #DC2626)
deffigure t3 = approx("x - x^3/6", #EA580C)
```

Un argument contenant une virgule ou commençant par une espace s'écrit entre guillemets ; les guillemets sont retirés. Les parenthèses protègent aussi les virgules : `repere(1, 2)` est un seul argument.

Une **répétition** crée une figure par valeur de `i`, puis un groupe qui les contient toutes :

```
defmodel barre(i, h) =
    type   : polygon
    points : [repere(@(i*h), 0), repere(@((i+1)*h), 0), repere(@((i+1)*h), 1), repere(@(i*h), 1)]
    fill   : blue

defrepeat barres = barre(@i, 0.5) for i in 0..7
```

Crée `barres_0` … `barres_7` (bornes incluses, entiers ≥ 0, 1000 figures au plus) et le groupe `barres`. On anime l'ensemble (`start : [barres draw(2s)]`) ou une seule figure (`anim : [barres_5 color(to=red, 1s)]`).

Un modèle doit être défini plus haut que son utilisation. Les erreurs d'une figure créée par un modèle indiquent la ligne de l'appel. `defmodel` et `defrepeat` ne s'utilisent qu'en section `[figures]`.

---

## 6. Section `[timeline]`

### 6.1 Format des timestamps

```
61.500 =>
```

Format : `secondes.millisecondes`.

| Écriture | Signification |
|---|---|
| `0.000` | 0 secondes, 0 ms |
| `1.500` | 1 seconde, 500 ms |
| `61.500` | 61 secondes, 500 ms |

**Unicité :** chaque timestamp n'apparaît qu'une fois. Deux blocs avec le même instant produisent une erreur de compilation : tout ce qui se passe à un instant donné est regroupé dans un seul bloc.

**Décaler la suite :** pour ajouter ou retirer du temps au milieu, inutile de réécrire chaque timestamp :

```
didact shift cours.dct 16 +2      # tous les blocs à partir de 16 s avancent de 2 s
didact shift cours.dct 16 -1.5    # ils reculent de 1,5 s
```

Seuls les en-têtes de blocs sont réécrits ; le reste du fichier (commentaires, espaces) ne change pas. La commande refuse, sans rien modifier, un décalage qui ferait passer un bloc avant 0 ou sur un bloc qui ne bouge pas, et donne alors le recul maximal possible. Si la fenêtre est ouverte, elle se recharge d'elle-même ; les voix déjà générées restent valables.

### 6.2 Structure d'un événement

Un événement timeline regroupe **tout ce qui se passe au même instant** :

```
5.000 => {
    start   : [figure_a, figure_b]
    end     : [figure_c fadeout(0.5s)]
    destroy : [figure_d]
    anim    : [figure_a move(to=(80%, 50%), 2s)]
    comment : ["texte FR", "text EN"]
}
```

### 6.3 Les commandes

| Commande | Effet |
|---|---|
| `start` | Rend visible / démarre |
| `end` | Cache (mais garde en mémoire) |
| `destroy` | Supprime complètement, libère la mémoire |
| `anim` | Déclenche une animation |
| `comment` | Affiche un sous-titre multilingue |

**Ordre d'exécution :** dans un bloc, les commandes s'exécutent toujours dans cet ordre, **quel que soit l'ordre d'écriture** :

```
destroy  →  end  →  start  →  anim  →  comment
```

Les deux blocs suivants sont donc équivalents :

```
1.000 => {
    start : [rect_mobile]
    anim  : [rect_mobile move(to=(80%, 50%), 2s)]
}

1.000 => {
    anim  : [rect_mobile move(to=(80%, 50%), 2s)]
    start : [rect_mobile]
}
```

Conséquence : animer une figure détruite dans le même bloc produit une erreur de compilation (`destroy` passe avant `anim`).

### 6.4 `end` vs `destroy`

```
end : [ma_ligne]               # disparition visuelle, reste en mémoire
end : [ma_ligne fadeout(0.5s)] # disparition animée

destroy : [ma_ligne]           # suppression complète
```

**Règle :** `destroy` sur une figure encore référencée par une autre figure produit une erreur de compilation.

**Exemple typique de la distinction :**

```
1.000 => {
    start : [courbe_sin, rect_mobile]
    anim  : [rect_mobile follow(path=courbe_sin, 5s)]
}

# La courbe disparaît visuellement mais rect_mobile continue à la suivre
30.000 => {
    end : [courbe_sin fadeout(0.5s)]
}

# rect_mobile finit son trajet
55.000 => {
    end : [rect_mobile fadeout(0.3s)]
}

# Maintenant on peut détruire : plus aucune référence
56.000 => {
    destroy : [courbe_sin, rect_mobile]
}
```

### 6.5 Animations enchaînées

Pour enchaîner plusieurs animations sur la même figure :

```
anim : [ma_figure [move(to=(50%, 50%), 1s), color(to=blue, 0.5s)]]
```

Les animations s'exécutent **séquentiellement**.

---

## 7. Système de coordonnées

### 7.1 Origine et orientation

```
(0%, 0%)     ←  bas gauche de la scène
(100%, 100%) ←  haut droit de la scène
```

L'origine est en **bas à gauche**, l'axe Y vers le haut, comme en mathématiques.

### 7.2 Positions en pourcentages

Les positions sont écrites en pourcentages de la taille de la scène :

```
pos : (50%, 50%)
```

### 7.3 Positions dans un repère mathématique

Pour les graphes, on peut référencer le repère d'une figure `axes` :

```
pos : axes_xy(3, 4)    # position (3, 4) dans le repère de la figure axes_xy
```

### 7.4 Convention des angles

```
angle positif  →  sens trigonométrique (anti-horaire)
angle négatif  →  sens horaire
```

Le sens est implicite dans le signe, pas besoin de paramètre `direction`.

### 7.5 Positions vs tailles

Deux règles différentes, pour que les formes ne se déforment jamais :

| | Unité `%` | Exemple |
|---|---|---|
| **Positions** (placer un objet) | x en % de la largeur, y en % de la hauteur | `pos : (50%, 50%)` = centre |
| **Tailles** (`radius`, `size`, `width`/`height` d'un rect) | % du **plus petit côté** de la scène | `width : 20%, height : 20%` = un vrai carré, sur tout écran |

Conséquence : sur un écran 16:9, une même valeur en % de position ne fait pas la même longueur en x et en y. Une ligne de `(0%, 0%)` à `(10%, 10%)` ne fait donc **pas** 45°. Les positions en % servent à placer ; la **géométrie exacte** (angles, droites qui doivent se rejoindre) se fait dans un repère `axes`, dont l'échelle est **la même en x et en y par défaut**, ou avec les références dynamiques (`center()`, `angle()`, ...).

---

## 8. Types de figures

Cette section liste les types et propriétés **explicitement décidés**. Les détails supplémentaires possibles sont dans la section §15 "À décider".

### 8.1 Propriétés communes

```
color    : red / #FF0000
fill     : none / red / #FF0000
opacity  : 0.0 → 1.0
stroke   : épaisseur du trait (ex: 2px)
style    : nom d'un style à appliquer
layer    : ordre d'affichage
anchor   : point d'ancrage (ex: center)
```

**`stroke` vs `width` :** `stroke` est l'épaisseur du trait, pour toutes les figures. `width` et `height` sont uniquement les dimensions d'un `rect`.

**`layer` :** un nombre. Les petites valeurs sont dessinées en premier (donc en dessous). Sans `layer`, la valeur est 0. À `layer` égal, l'ordre de définition dans le fichier est conservé.

### 8.2 Géométrie

#### `line`

```
deffigure ma_ligne =
    type  : line
    from  : (20%, 50%)
    to    : (80%, 50%)
    color : red
    stroke: 2px
    arrow : none / start / end / both
```

#### `circle`

```
deffigure mon_cercle =
    type    : circle
    center  : (50%, 50%)
    radius  : 10%
    color   : blue
    fill    : blue
    opacity : 0.5
```

#### `rect`

```
deffigure mon_rect =
    type   : rect
    pos    : (20%, 20%)
    width  : 30%
    height : 20%
    color  : green
    fill   : none
```

#### `polygon`

```
deffigure mon_triangle =
    type   : polygon
    points : [(20%, 80%), (50%, 20%), (80%, 80%)]
    color  : black
    fill   : yellow
```

Les points peuvent aussi être dans un repère : `points : [repere(0, 0), repere(1, 0), repere(1, 2)]`.

#### `arc`

```
deffigure mon_arc =
    type       : arc
    center     : (50%, 50%)
    radius     : 10%
    from_angle : 0
    to_angle   : 90
    color      : red
    stroke     : 2px
    arrow      : end
```

Le sens est implicite par le signe : si `to_angle > from_angle`, l'arc va dans le sens trigonométrique.

#### `star`

```
deffigure mon_etoile =
    type   : star
    points : 5
    pos    : (50%, 50%)
    size   : 2%
    color  : gold
    fill   : gold
```

### 8.3 Texte et maths

#### `text`

```
deffigure mon_texte =
    type    : text
    content : "Bonjour"
    pos     : (50%, 50%)
    font    : 24px
    color   : black
    align   : center / left / right
```

- `pos` est le point d'ancrage : le texte est centré verticalement dessus, et aligné horizontalement selon `align` (défaut : `center`). `font` : taille en px (défaut 32px) ou en %.
- Police : Latin Modern (celle de LaTeX), embarquée.
- **Plusieurs lignes** : un vrai retour à la ligne dans la chaîne (pas de `\n`, pour que `$\nu$` reste la lettre ν).
- **Maths en ligne** entre `$...$` : du vrai LaTeX (fractions, racines, sommes, matrices, ...), composé dans la ligne de texte.

#### `equation`

```
deffigure mon_eq =
    type  : equation
    expr  : \int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
    pos   : (50%, 50%)
    size  : 32px
    color : black
```

Le contenu de `expr` est du LaTeX en mode mathématique, lu tel quel jusqu'à la fin de la ligne (pas de guillemets), affiché en formule centrée (bornes d'intégrales et de sommes au-dessus et en dessous). `size` : taille (défaut 40px). `align` : `center` (défaut), `left` ou `right` par rapport à `pos` ; avec `left`, le début d'une formule qui s'allonge par `transform` reste immobile.

#### `latex`

```
deffigure mon_latex =
    type    : latex
    pos     : (50%, 50%)
    content : |
        \begin{align}
            f(x) &= x^2 + 2x + 1 \\
                 &= (x+1)^2
        \end{align}
```

Le caractère `|` introduit un bloc multilignes : les lignes suivantes plus indentées que la propriété, retours à la ligne conservés. Le contenu est du LaTeX en mode texte (environnements `aligned`, `align`, `cases`, paragraphes...). `font` : taille.

#### `table`

```
deffigure ma_table =
    type   : table
    pos    : (50%, 50%)
    header : [x, f(x), f'(x)]
    rows   : |
        0 | 1  | 0
        1 | e  | e
        2 | e² | 2e²
```

Une ligne du bloc = une ligne du tableau, cases séparées par `|`. Les cases et l'en-tête acceptent les maths `$...$` (dans `header`, mettre la case entre guillemets : `["$x$", "$f(x)$"]`). `font` : taille.

### 8.4 Graphes

#### `axes`

```
deffigure mes_axes =
    type   : axes
    from   : (10%, 10%)       # coin bas-gauche de la boîte
    to     : (90%, 90%)       # coin haut-droit de la boîte
    xrange : [-10, 10]
    yrange : [-5, 5]
    xlabel : "$x$"
    ylabel : "$f(x)$"
    grid   : true / false     # défaut : false
    ticks  : true / false     # défaut : true
    equal  : true / false     # défaut : true
```

- Le repère s'inscrit dans la boîte `from` / `to`. Avec `equal : true` (défaut), l'échelle est **la même en x et en y** et le repère est centré dans la boîte ; avec `equal : false`, il remplit la boîte (échelles différentes).
- Les axes passent par 0 si 0 est dans l'intervalle, sinon par le bord. Graduations automatiques (pas de 1, 2 ou 5 × 10ⁿ, environ 8 graduations).
- Position dans le repère : `pos : mes_axes(3, 4)`.
- **Caméra 2D :** `center : (x, y)` (point au centre, défaut : milieu de xrange × yrange) et `zoom : z` (2 = deux fois plus près, défaut 1). Avec des paramètres, on zoome et on se déplace en les animant : `zoom : 10^k` puis `k to(3, 5s)` donne un zoom régulier jusqu'à × 1000. Les graduations, les nombres (autant de décimales que nécessaire), les courbes et les figures du repère suivent. xrange et yrange acceptent aussi des paramètres.
- **Découpe :** toute figure qui a `axes : mes_axes` est coupée au cadre xrange × yrange du repère (`clip : false` pour l'empêcher).

#### `plot`

```
deffigure mon_plot =
    type  : plot
    expr  : sin(x)
    axes  : mes_axes
    color : red
    stroke: 2px
```

- `expr` est lue telle quelle jusqu'à la fin de la ligne : `expr : x^2 + 2x - 1` (pas de guillemets).
- Variable `x`, constantes `pi` et `e`, `+ - * / ^`, parenthèses, multiplication implicite (`2x`, `3(x+1)`, `2sin(x)`).
- Fonctions à un argument : `sin cos tan asin acos atan sinh cosh tanh exp ln log sqrt abs floor ceil round sign`.
- Fonctions à plusieurs arguments : `min(a, b, ...)`, `max(a, b, ...)`, `atan2(y, x)`, `mod(a, b)` (toujours positif), `hypot(a, b)`, `clamp(v, bas, haut)`.
- Comparaisons `< > <= >= == !=` (valent 1 ou 0), `and(a, b)`, `or(a, b)`, `not(a)`.
- Par morceaux : `if(condition, alors, sinon)`, ex. `if(x < 0, -x, x^2)` ; ou `(x > 0) * x`.
- Sommes et produits : `sum(k, 1, n, sin(k*x)/k)`, `prod(j, 1, k, j)` (imbricables ; la variable n'existe qu'à l'intérieur). Avec une borne animée non entière (`n` = 3,4), le terme suivant compte pour 0,4 : les termes apparaissent en douceur. Au plus 100 000 termes.
- Nombres complexes (dans `cmap`, `domain`, `escape`) : variable `z`, unité `i`, `re im arg conj abs`, `exp ln sqrt sin cos ...`.
- Probabilités : `fact(n)`, `binom(n, k)`, `gamma(x)`, `erf(x)`.
- Hasard reproductible : `rand(k)` (uniforme dans [0, 1[) et `randn(k)` (loi normale centrée réduite) ; la même formule donne toujours la même valeur, `rand(1)`, `rand(2)`, ... sont des tirages indépendants. Graine facultative : `rand(k, 7)`. Ex. : `sum(k, 1, n, rand(k))/n` tend vers 1/2 quand n grandit.
- Tracée sur tout `xrange` du repère ; coupée hors de `yrange` et là où elle n'est pas définie.
- Points : un tous les 2 pixels environ, puis des points ajoutés là où la courbe s'écarte de plus d'un demi-pixel d'un segment droit (oscillations rapides, tangente verticale, zoom). `samples : n` impose n points réguliers.

#### `grid`

```
deffigure quadrillage =
    type   : grid
    axes   : mes_axes
    step   : 1           # ou (1, 0.5)
    xrange : [-2, 2]     # défaut : celui du repère
```

Droites x = k·pas et y = k·pas du repère. Fait pour être déformé par `matrix`, `map` ou `cmap` (§15.8). `step : auto` : même pas que les graduations (suit le zoom).

#### `scatter`

```
deffigure mon_scatter =
    type   : scatter
    points : [(1, 2), (3, 4), (5, 1)]
    axes   : mes_axes
    color  : blue
    size   : 5px
```

Les points sont en coordonnées du repère. `size` = rayon des points.

**Points tirés par formule :** `count : n`, `x : rand(2k)`, `y : rand(2k + 1)` (fonctions de k = 1, ..., n), et un filtre facultatif `where : x^2 + y^2 <= 1` (seuls les points qui le vérifient sont dessinés). Avec `count` animé, les points apparaissent au fur et à mesure (Monte-Carlo).

#### `histogram`

```
deffigure mon_histo =
    type  : histogram
    data  : [1, 2, 2, 3, 3, 3, 4]
    bins  : 10
    axes  : mes_axes
    color : green
```

Les données sont réparties en `bins` classes de même largeur entre le minimum et le maximum ; la hauteur d'une barre = nombre de valeurs. Couleur : `fill`, sinon `color`.

- **Valeurs tirées par formule :** `count : n` et `sample : randn(k)` (fonction de k = 1, ..., n) à la place de `data`.
- `range : [a, b]` : classes fixes entre a et b (un histogramme qui se remplit ne bouge pas).
- `density : true` : hauteurs divisées par (nombre × largeur) : l'aire vaut 1, comparable à une densité tracée par `plot`.

---

## 9. Styles et héritage

### 9.1 Définir un style

```
defstyle trait_fin =
    color   : red
    stroke  : 2px
    opacity : 1.0
```

### 9.2 Appliquer un style

```
deffigure ma_ligne =
    type  : line
    style : trait_fin
    from  : (20%, 50%)
    to    : (80%, 50%)
```

### 9.3 Surcharger des propriétés

Les propriétés définies directement sur la figure **écrasent** celles du style :

```
defstyle trait_fin =
    color : red
    stroke: 2px

deffigure ma_ligne =
    type  : line
    style : trait_fin
    color : blue        # ← écrase le rouge du style
    from  : (20%, 50%)
    to    : (80%, 50%)
```

### 9.4 Héritage entre styles

```
defstyle base =
    opacity : 1.0
    stroke  : 2px

defstyle rouge_epais =
    extends : base
    color   : red
    stroke  : 4px       # ← écrase le stroke de base
```

---

## 10. Groupes

```
defgroup mon_graphe =
    items : [mes_axes, mon_plot, mon_scatter]
    pos   : (10%, 0%)
```

Un groupe se déplace, s'anime et apparaît/disparaît comme un seul objet.

- **`items`** (obligatoire) : les figures du groupe, définies **avant** lui. Un groupe peut contenir d'autres groupes.
- **`pos`** (optionnel) : **décalage** appliqué à toutes ses figures, en % (x de la largeur, y de la hauteur, vers le haut). `(10%, 0%)` = tout 10 % plus à droite ; `(-5%, 0%)` est permis. Sans `pos`, les figures restent où elles sont définies.
- **`start` / `end` / `destroy`** d'un groupe s'appliquent à toutes ses figures. On peut toujours démarrer une figure du groupe seule. Un `end` de groupe ne touche pas les figures déjà cachées (elles ne réapparaissent pas pour sa sortie). Pratique : un groupe par chapitre, puis `end : [chapitre fadeout(1s)]`.
- **Animations** (`move`, `rotate`, `scale`, `fadein`, `draw`, ...) : appliquées à l'ensemble, autour du **centre de l'ensemble** des figures affichées. `draw` et `write` révèlent les figures l'une après l'autre.
- **Références** : `center(groupe)` = centre de l'ensemble ; `center(figure)` suit le groupe pour les figures définies **après** le groupe.
- Une figure peut être dans plusieurs groupes : les effets s'additionnent (provisoire, cf. §15).
- `transform` n'est pas encore possible sur un groupe.

---

## 11. Animations

### 11.1 Apparition

```
fadein(durée)
slidein(direction, durée)      # arrive depuis : left / right / up / down (ou top / bottom)
grow(durée)
write(durée)                   # s'écrit progressivement (texte/équation)
draw(durée)                    # se dessine progressivement (géométrie)
```

### 11.2 Mouvement

```
move(to=position, durée)
move(direction, distance, durée)   # ex: move(down, 20%, 2s)
follow(path=fig, durée, rotate=tangent)    # parcourt tout le contour de fig, à vitesse régulière
rotate(angle, durée)
```

### 11.3 Transformation

```
color(to=couleur, durée)
scale(facteur, durée)
transform(to=fig, durée)       # se déforme en fig, qui la remplace à la fin
```

### 11.4 Disparition

```
fadeout(durée)
slideout(direction, durée)
shrink(durée)
```

### 11.5 Animation par défaut

Une figure apparaît et disparaît **instantanément** si aucune animation n'est précisée. Toutes les animations sont opt-in.

### 11.6 Où écrire une animation

```
start : [fig fadein(1s)]                          # animation d'entrée
end   : [fig fadeout(0.5s)]                       # animation de sortie : la figure reste
                                                  # affichée jusqu'à la fin du fondu
anim  : [fig move(to=(80%, 50%), 2s)]             # animation sur une figure affichée
anim  : [fig [rotate(45, 1s), color(to=red, 1s)]] # enchaînées : l'une après l'autre
anim  : [a fadein(1s), b move(to=center(a), 1s)]  # séparées par une virgule : en parallèle
```

### 11.7 Paramètres

- **Durée** : `2s` ou `500ms` (ou `duration=2s`). Obligatoire.
- **Easing** : `ease=smooth` (défaut : démarre et finit lentement), `ease=linear`, `ease=ease_in`, `ease=ease_out`.
- Une animation terminée **garde son effet** : une figure déplacée reste à sa nouvelle position.

| Animation | Effet |
|---|---|
| `fadein(d)` / `fadeout(d)` | opacité 0 → 1 / 1 → 0 |
| `grow(d)` / `shrink(d)` | taille 0 → 1 / 1 → 0, autour du centre |
| `move(to=pos, d)` | le **centre** de la figure va vers `pos` : `(x%, y%)` ou `center(fig)` |
| `color(to=couleur, d)` | la couleur du **trait** (`color`) change ; `fill` ne change pas |
| `scale(f, d)` | la taille va vers `f` × la taille d'origine (`scale(2)` deux fois = toujours 2) |
| `to(valeur, d)` | un **paramètre** va vers `valeur` (§12) ; ne s'applique pas à une figure |
| `rotate(angle, d)` | tourne **de** `angle` degrés autour du centre (positif = sens trigonométrique), s'additionne |

| `follow(path=fig, d)` | le **centre** parcourt le contour de `fig` (courbe, ligne, arc, cercle, ...) du début à la fin, à vitesse régulière ; `rotate=tangent` : la figure s'oriente selon la tangente |

| `slidein(dir, d)` / `slideout(dir, d)` | glissé court (12 % de la hauteur) + fondu ; `slidein(left)` arrive **depuis** la gauche, `slideout(right)` part **vers** la droite |
| `draw(d)` | le contour se trace, puis la figure se remplit en fondu (les morceaux presque en même temps) |
| `write(d)` | pareil, morceau par morceau, comme une écriture (texte, équations : lettre après lettre) |
| `move(dir, dist%, d)` | se déplace de `dist` % dans la direction `up` / `down` / `left` / `right` (% de la largeur ou de la hauteur) |
| `transform(to=b, d)` | les formes se déforment vers celles de `b` (position, forme, couleurs) ; à la fin, la figure est cachée et `b` est affichée à sa place. `b` doit être définie (pas forcément démarrée). **Entre deux textes ou équations**, c'est lettre par lettre : les lettres communes glissent à leur nouvelle place sans se déformer, les autres apparaissent ou disparaissent en fondu |
| `color(to=c, d, part="...")` | seul ce morceau d'un texte ou d'une équation change de couleur. Dans une équation, `part` est du LaTeX (`part="\frac{x^3}{3!}"`) ; dans un texte, du texte avec `$...$`. Toutes les occurrences changent. Un morceau introuvable est une erreur de compilation |

Les apparitions et disparitions (`fadein`, `slidein`, `grow`, `draw`, `write`, `fadeout`, `slideout`, `shrink`, `transform`) ne comptent plus après un nouveau `start` de la figure.

Rotation fixe d'une figure : propriété `rotate : 30` (degrés) ou `rotate : auto_tangent(courbe)` (parallèle à la courbe, au point le plus proche).

Après un nouveau `start`, les fondus et `grow`/`shrink` antérieurs ne comptent plus (la figure réapparaît normalement) ; les déplacements, rotations, tailles et couleurs sont conservés.

---

## 12. Références dynamiques

Certaines valeurs sont calculées à chaque frame plutôt que fixées à la définition.

### `center(fig)`

Position du centre d'une figure :

```
deffigure ligne =
    type : line
    from : center(rect_mobile)   # ← se met à jour quand rect_mobile bouge
    to   : (80%, 80%)
```

### `axes_xy(x, y)`

Position dans le repère d'une figure `axes` :

```
deffigure point =
    type   : circle
    center : axes_xy(3, 4)
    radius : 2%
```

### `offset(fig, distance)`

Position décalée par rapport à une figure : pour un `arc`, à `distance` vers l'extérieur du milieu de l'arc (idéal pour une étiquette d'angle) ; pour les autres figures, en haut à droite de la figure, à `distance` du coin. `distance` en % du plus petit côté ou en px.

```
deffigure label =
    type    : text
    content : "θ"
    pos     : offset(arc_theta, 1%)
```

### `angle(fig)`

Angle dynamique d'une figure :

```
deffigure arc =
    type       : arc
    from_angle : 0
    to_angle   : angle(ligne_direction)
```

### `auto_tangent(fig)`

Rotation parallèle à la tangente d'une courbe :

```
deffigure rectangle =
    type   : rect
    rotate : auto_tangent(courbe_sin)
```

### Paramètres : `defparam`

Un paramètre est un nombre nommé qui évolue dans le temps. Tout ce qui l'utilise est recalculé à chaque image.

```
defparam k = 1                 # valeur de départ (obligatoire)

deffigure onde =
    type : plot
    expr : sin(k*x)            # dans une formule
    axes : repere

deffigure point =
    type   : circle
    center : repere(a, a^2)    # dans des coordonnées de repère
    radius : 1%

2.000 => {
    anim : [k to(3, 2s)]       # k va de sa valeur actuelle à 3 en 2 s
}
```

- **Animation :** seule `to(valeur, durée)` s'applique à un paramètre (avec `ease=...` si besoin, enchaînable). Un paramètre garde sa valeur à la fin. `start`, `end` et `destroy` ne le concernent pas.
- **Où l'utiliser :** partout où un nombre est attendu, avec son unité : `radius : (2*k)%`, `stroke : (k + 1)px`, `opacity : k/4`, `rotate : 30k`, `pos : (k*10%, 50%)`, `points : [(10%, 10%), (k*15%, 30%)]`, `matrix : (1, k, 0, 1)`. Aussi dans `expr` d'un `plot` et les coordonnées d'un repère (`repere(a - 1, sin(a))`, `move(to=...)`). Mêmes opérations et fonctions que `expr`, sans `x`. Un calcul qui ne se lit pas est signalé à sa ligne (« valeur '2*q' non comprise »).
- **Nombres dans le texte :** dans `content`, l'`expr` d'une `equation`, `rows`, ... `#{expr}` est remplacé par la valeur à chaque image : `content : "n = #{n}"`. `#{k}` affiche au plus 2 décimales, `#{k:0}` un entier arrondi, `#{k:3}` 3 décimales. Ex. : `expr : \sum_{i=1}^{#{round(n):0}} i = #{round(n)*(round(n)+1)/2:0}`.
- **Nom :** lettres, chiffres et `_`, sans les noms réservés des formules (`x`, `e`, `pi`, `sin`, ...). Comme tout élément, un paramètre doit être défini plus haut que ce qui l'utilise.

---

## 13. Audio et commentaires multilingues

### 13.1 Audio

```
0.000 => {
    start : [audio musique.mp3 volume=0.3]
}

45.000 => {
    end : [audio musique.mp3]
}
```

- Le chemin est relatif au fichier `.dct`. Formats : mp3, wav, ogg, flac, m4a.
- `volume` : 1 par défaut (0.3 = plus bas).
- Une piste joue de son `start` jusqu'à son `end` (ou la fin de la timeline). Un nouveau `start` du même fichier la relance depuis le début.
- Le son suit la lecture : pause, saut dans la barre de lecture, retour en arrière.
- Joué dans la fenêtre, sur le web (`<audio>`), mixé dans l'export MP4 et inclus dans la page HTML autonome.

### 13.2 Voix : les commentaires lus à voix haute

```
didact cours.dct --voice
```

Chaque `comment` est lu par un programme de synthèse vocale **choisi par l'utilisateur** (Piper, Kokoro, …), décrit dans `didact-voix.txt` à côté du `.dct` (une ligne par langue) :

```
FR : python -m piper -m fr_FR-siwis-medium --data-dir voix -f {out}
EN : python -m piper -m en_US-lessac-medium --data-dir voix -f {out}
```

- `{out}` = fichier .wav à écrire (obligatoire) ; aussi `{text}`, `{text_file}`, `{lang}`. La phrase est envoyée sur l'entrée standard.
- Les fichiers vont dans `cours_voix/` (un par phrase et par langue). Relancer `--voice` ne refait que les phrases modifiées (ou toutes, si la commande d'une langue a changé).
- Les maths sont prononcées simplement : `$x^2$` → « x au carré », `\frac{a}{b}` → « a sur b », `\sqrt{2}` → « racine de 2 » ; le reste du LaTeX est retiré.
- **Phrase trop longue** : si une phrase lue dure plus longtemps que l'écart avant le commentaire suivant, **l'animation attend** juste avant ce commentaire que la phrase soit finie (+ 0,25 s de respiration), puis reprend. Rien n'est coupé, image et voix restent synchronisées ; la vidéo dure un peu plus longtemps. `--voice` liste chaque phrase concernée (ligne, durée lue, écart, attente ajoutée), puis le total par langue.
- Ensuite, la voix de la langue choisie est jouée **partout** : fenêtre (elle suit le menu de langue), web (si `cours_voix/` est à côté du `.dct`), `--export mp4`, `--export html` (incluse dans la page).
- `--export audio` : le son seul (pistes audio + voix) en MP3, le « podcast ».

### 13.3 Commentaires multilingues

L'ordre des traductions correspond à l'ordre déclaré dans `languages` du `[config]` :

```
# si languages = [FR, NL, EN]
comment : ["la barre descend", "de balk daalt", "the bar goes down"]
```

### 13.4 Sélection de la langue au rendu

La langue choisie est passée en paramètre au compilateur ou à l'embarquement.

---

## 14. Escape hatches : LaTeX et Matplotlib bruts

Pour les utilisateurs qui connaissent déjà ces outils.

### 14.1 `raw.latex`

```
deffigure mon_bloc =
    type : raw.latex
    pos  : (50%, 50%)
    code : |
        \begin{equation}
            \int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
        \end{equation}
```

Interprété comme le type `latex` (LaTeX en mode texte).

### 14.2 `raw.matplotlib`

```
deffigure mon_graphe =
    type : raw.matplotlib
    pos  : (50%, 50%)
    code : |
        x = np.linspace(0, 10, 100)
        plt.plot(x, np.sin(x), color='red')
```

---

## 15. 3D, champs et récurrences

### 15.1 Principe

Une **scène 3D** (`scene3d`) est une figure qui porte une caméra. Les objets 3D s'y rattachent par `scene : nom` (comme une courbe à son repère par `axes`). À chaque image, Didact découpe les objets en petits morceaux (faces, segments, points), les projette avec la caméra et les **trie du plus loin au plus proche** : la transparence et l'anti-crénelage restent parfaits. Les figures 2D (textes, équations) restent par-dessus, nettes.

- Axes : **z vers le haut**.
- Toutes les animations existantes marchent sur les objets 3D (voir 15.5), et les paramètres (`defparam`) sont utilisables partout : caméra, formules, positions.
- Un objet 3D n'est dessiné que s'il est démarré (`start`) ; le repère de la scène n'est dessiné que si la scène l'est.

### 15.2 La scène : `scene3d`

```
defparam theta = -60

deffigure espace =
    type       : scene3d
    xrange     : [-3, 3]          # défaut [-3, 3] pour les trois axes
    yrange     : [-3, 3]
    zrange     : [0, 5]
    camera     : (30, theta)      # (élévation, rotation autour de z) en degrés
    projection : perspective      # ou ortho
    fov        : 30               # angle de vue (perspective), en degrés
    zoom       : 1
    target     : (0, 0, 2)        # point regardé (défaut : centre de la boîte)
    equal      : true             # unités réelles ; false : boîte de proportions `box`
    box        : (1, 1, 0.8)      # proportions si equal : false
    light      : (1, 1, 2)        # direction de la lumière (défaut : liée à la caméra)
    ambient    : 0.35             # part de lumière ambiante
    axes       : lines            # lines (ou true), box, none (défaut)
    floor      : 8                # grille au sol (n lignes)
    labels     : ("x", "y", "z")  # LaTeX
    color      : #888888          # couleur du repère
    from       : (5%, 5%)         # zone de l'écran (défaut : toute la scène)
    to         : (95%, 95%)
```

Tout nombre peut être une expression avec des paramètres : faire tourner la caméra, c'est animer un paramètre (`anim : [theta to(30, 6s)]`).

**Dans la fenêtre** : glisser la souris tourne la caméra, la molette zoome, un double-clic revient à la caméra du fichier. C'est un aperçu : les exports gardent la caméra du fichier.

**Points 3D.** Partout où une position est attendue :

| Écriture | Signification |
|---|---|
| `espace(x, y, z)` | point de la scène `espace` |
| `f(a, b)` | point de la surface `f` : `(a, b, f(a, b))`, ou le point de paramètres (u, v) = (a, b) |
| `center(objet)` | centre d'un objet 3D (il suit ses animations) |
| `(x, y, z)` | point de la scène de l'objet (avec `scene : ...`) |

Une figure 2D accepte aussi ces points : `pos : espace(1, 2, 3)` place une équation sur ce point, et elle suit la caméra.

### 15.3 Objets 3D

**Apparence commune :** `fill` (faces), `color` (lignes, arêtes, grille), `stroke` (épaisseur des lignes), `opacity`, `shading : flat | none` (éclairage selon l'orientation des faces), `glow` (halo, §15.6), `tube` (lignes dessinées comme un tube éclairé, épaisseur en px).

**`surface`** — `z = f(x, y)` ou paramétrée :

```
deffigure f =
    type       : surface
    scene      : espace
    expr       : sin(x)*cos(y)       # z = f(x, y)
    xrange     : [-3, 3]             # défaut : ceux de la scène
    yrange     : [-3, 3]
    resolution : 48                  # ou (nu, nv)
    grid       : (20, 20)            # lignes de grille (0 : aucune)
    colormap   : viridis             # viridis, plasma, inferno, magma, coolwarm, turbo, blues, gray, ou [#000, #FFF]
    color_by   : z                   # valeur colorée (x, y, z, u, v) ; défaut z
    crange     : [-1, 1]             # défaut : min et max
    clip       : true                # coupée à zrange (défaut)

deffigure tore =
    type   : surface
    scene  : espace
    expr   : (cos(u)*(2 + cos(v)), sin(u)*(2 + cos(v)), sin(v))
    urange : [0, 6.2832]
    vrange : [0, 6.2832]
```

**`curve3d`** — `expr : (x(t), y(t), z(t))` avec `t : [a, b]` et `samples` (défaut 200), ou `points : [espace(0, 0, 0), f(1, 2), ...]`. `markers` : rayon des points en px.

**Solides :** `cube` (`size`), `box` (`size : (a, b, c)`), `sphere` (`radius`), `cylinder` et `cone` (`radius`, `height`), `plane` (`size : (a, b)`, `grid`), `polyhedron` (`points : [(x, y, z), ...]`, `faces : [(0, 1, 2), ...]`). Position : `center` ; orientation fixe : `rotate : (rx, ry, rz)` en degrés (autour de x, puis y, puis z) ; `resolution` : finesse du découpage.

**`arrow`** — `from`, `to`. En 3D (points 3D ou `scene`) : tige et pointe conique, `head` = longueur de la pointe. En 2D (points en % ou dans un repère) : flèche plate.

**`field`** — champ de vecteurs : `expr : (fx, fy)` dans un repère 2D (`axes`), ou `(fx, fy, fz)` dans une scène. `density` (nombre de flèches par axe), `length` (part de la case, défaut 0.85), `normalize : true` (toutes de même longueur), `colormap` (selon l'intensité), `xrange`/`yrange`/`zrange`.

Pour un tenseur ou tout objet composé : un `defgroup` (cube + flèches) s'anime comme un seul objet (`rotate3d` sur le groupe).

### 15.4 `iterate` : récurrences et optimiseurs

Une suite de points calculée par Didact, dessinée pas à pas (`draw` fait apparaître les pas un à un).

**Récurrence libre :**

```
deffigure orbite =
    type   : iterate
    axes   : repere                     # ou scene : espace (3 variables : x, y, z)
    vars   : (x, y)
    init   : (2.5, 0)
    update : (x - 0.1*y, y + 0.1*(x - 0.3*y))
    steps  : 120
```

Dans `update` : les variables, `n` (numéro du pas), les paramètres, et si `on` est donné : `f` (valeur), `dx`, `dy` (dérivées), `hxx`, `hxy`, `hyy` (dérivées secondes), calculées par Didact. Les nouvelles valeurs sont toutes calculées à partir des anciennes. On peut donc écrire n'importe quelle méthode :

```
    on     : f
    vars   : (x, y, vx, vy)
    init   : (-2, 3, 0, 0)
    update : (x + vx, y + vy, 0.9*vx - 0.02*dx, 0.9*vy - 0.02*dy)
```

**Optimiseurs intégrés** (raccourcis) : `on : f` (surface z = f(x, y), ou un `plot` 2D), `start : (x, y)`, `method : gd | momentum | nesterov | rmsprop | adam | newton`, `rate`, `beta`, `beta2`, `eps`, `steps`.

**Dessin :** sur une surface, les points sont posés dessus (`lift` : hauteur au-dessus, défaut 1 % de zrange) ; `drape : true` fait suivre la surface entre deux pas. `color` (ligne, `none` pour l'enlever), `stroke` ou `tube`, `markers` (rayon des points, défaut 4px), `fill` (couleur des points), `glow`. Une bille peut suivre la trajectoire : `follow(path=chemin, 5s)`.

### 15.5 Animations en 3D

| Animation | En 3D |
|---|---|
| `fadein`, `fadeout`, `grow`, `shrink`, `scale` | comme en 2D, autour du centre de l'objet |
| `draw`, `write` | les morceaux apparaissent dans l'ordre (surface balayée, courbe tracée, pas d'une trajectoire) |
| `move(to=espace(x, y, z), d)` | le centre va vers ce point ; `move(up, 20%, d)` : up/down = z, left/right = x, en % de la plus grande étendue |
| `rotate(angle, d)` | rotation autour de l'axe vertical (z) passant par le centre |
| `rotate3d(angle, axis=x, d)` | rotation autour de l'axe `x`, `y`, `z` ou `(a, b, c)` passant par le centre |
| `follow(path=courbe, d)` | le centre parcourt une `curve3d`, une trajectoire `iterate` ou une `arrow` |
| `color(to=c, d)` / `fill(to=c, d)` | couleur des lignes / des faces (`fill` marche aussi en 2D) |
| `transform(to=b, d)` | entre deux surfaces : la surface se déforme point par point ; entre autres objets : les morceaux glissent s'ils correspondent, sinon fondu enchaîné |

Sur un groupe, l'animation s'applique à tous ses objets 3D autour du centre de l'ensemble. Sur la scène elle-même : `fadein`, `fadeout`, `draw` s'appliquent à son repère.

### 15.6 Halo : `glow`

`glow : 10px` sur n'importe quelle figure (2D ou 3D) ajoute un halo lumineux de ce rayon. Il éclaircit ce qui est dessous (sur un fond sombre surtout) et se voit à travers les surfaces translucides. Les halos de toute l'image sont calculés en une seule fois à la fin.

### 15.7 Limites connues

- Deux grandes faces qui se coupent peuvent être mal ordonnées par endroits : augmenter `resolution` les découpe plus finement.
- Une ligne posée sur une surface passe devant les faces voisines (pour rester visible) : vue de dessous, elle peut se voir à travers la surface.
- Pas d'ombres portées ni de reflets : éclairage simple selon l'orientation des faces.
- Le nombre de pas d'une trajectoire et la finesse d'un maillage sont fixés pour toute l'animation (ils ne peuvent pas suivre un paramètre animé).

### 15.8 Transformations du plan et de l'espace

Une figure peut être déformée point par point, dans les coordonnées de son repère (2D : `axes : repere` obligatoire ; 3D : celles de sa scène). Une seule transformation par figure :

```
defparam t = 0

deffigure quadrillage =
    type   : grid               # droites x = k·pas et y = k·pas
    axes   : r
    step   : 0.5                # ou (1, 0.5)
    xrange : [-2, 2]            # défaut : celui du repère
    yrange : [-2, 2]
    matrix : (1, 1, 0, 1)       # (x, y) → (x + y, y)
    amount : t                  # 0 : d'origine, 1 : transformée

deffigure conforme =
    type : grid
    axes : r
    cmap : z^2                  # fonction complexe, z = x + iy

deffigure cube_tordu =
    type  : cube
    scene : espace
    map   : (x + 0.4*z*sin(2y), y, z + 0.3*x^2)
```

- `matrix : (a, b, c, d)` : (x, y) → (a·x + b·y, c·x + d·y) ; en 3D, 9 nombres ligne par ligne.
- `map : (u, v)` en fonction de `x`, `y` (et `(u, v, w)` en `x`, `y`, `z` en 3D), avec les paramètres.
- `cmap : f(z)` : application conforme (2D seulement), formule complexe lue jusqu'à la fin de la ligne.
- `amount` (défaut 1) : interpolation entre chaque point et son image. Animer un paramètre (`amount : t`, `t to(1, 3s)`) déforme la figure progressivement.
- Marche sur toute figure 2D placée dans un repère (`grid`, `plot`, `line`, `circle`, `polygon`, `arrow`, ...) et sur tout objet 3D. Traits et faces sont redécoupés finement avant la transformation : une droite devient courbe si l'application n'est pas linéaire. Un point non défini (division par zéro) coupe le trait.
- La figure transformée est coupée au cadre du repère (`clip : false` pour tout garder).

### 15.9 Régions et courbes de niveau

**`contour`** — courbes de niveau f(x, y) = c dans un repère 2D, ou courbe implicite :

```
deffigure niveaux =
    type     : contour
    axes     : r
    expr     : 0.3*x^2 + (y - 0.6*x^2)^2
    levels   : 14                 # ou [0.5, 1, 2]
    colormap : viridis

deffigure lemniscate =
    type  : contour
    axes  : r
    expr  : (x^2 + y^2)^2 = 4*(x^2 - y^2)
    color : #7C3AED
```

- `expr` : f(x, y) (variables `x`, `y`, paramètres), ou une équation `g = h` (alors la courbe g = h, `levels` par défaut [0]).
- `levels : n` : n niveaux répartis selon les valeurs de f (autant de lignes dans un creux que sur les pentes) ; ou une liste. Défaut : 10.
- `colormap` : une couleur par niveau, dans l'ordre ; sinon `color`. `resolution` : finesse du calcul (défaut 160 cases en largeur). `xrange`, `yrange` : zone (défaut : celle du repère).

**`region`** — zone remplie, de deux façons :

```
deffigure aire =
    type   : region
    axes   : r
    upper  : x^2/2                # courbe du haut, en x
    lower  : 0                    # courbe du bas (défaut 0)
    xrange : [0, b]               # b animé : l'aire grandit
    fill   : #2563EB

deffigure admissible =
    type   : region
    axes   : r
    where  : and(x + y >= 1.2, x^2 + y^2 <= 6)
    fill   : #F59E0B
    color  : #B45309              # bord tracé seulement si color est donnée
```

- `where` : une inégalité en x et y, ou plusieurs avec `and(...)` (intersection), `or(...)` (réunion), `not(...)`.
- `fill` (défaut : bleu translucide) et `opacity` ; avec `upper`/`lower`, une borne de `xrange` peut partir de l'autre (zone vide au début).

**Lignes de niveau d'une surface 3D** — sur une `surface` z = f(x, y) : `levels : 12` (ou une liste), `levels_on : surface | floor | both` (sur la surface, au sol de la scène, ou les deux), `level_color` (sinon la `colormap` de la surface, sinon blanc), `level_stroke` (défaut 2px). Avec une `colormap`, les couleurs de la surface couvrent maintenant la partie visible (zrange) et non toute la fonction.

Le calcul des courbes de niveau, des régions et des surfaces est gardé d'une image à l'autre tant que rien dont il dépend ne change (paramètres, repère, propriétés).

---

### 15.10 Équations différentielles : `trajectory`

```
deffigure pendule =
    type     : trajectory
    axes     : r                         # ou scene : espace
    ode      : (y, -sin(x) - 0.25*y)     # (x', y') ; (x', y', z') en 3D
    init     : (2.5, 0)
    duration : T                         # t va de 0 à duration
```

- Résolution par Runge–Kutta d'ordre 4, pas `dt` (défaut 0.01). Variables `x`, `y` (et `z`), le temps `t`, les paramètres. Autres noms : `vars : (theta, omega)` (de 1 à 6 variables ; les deux premières sont dessinées).
- `count : n` : n trajectoires, `init` dépendant de `k` (0 à n - 1) : portrait de phase (`init : (-4 + 8*mod(k, 9)/8, -3 + 6*floor(k/9)/8)`, `count : 81`), nuage de particules (`init : (randn(k), randn(k, 1))`).
- `duration` animé : les courbes s'allongent. `trail : 1.5` : seulement la dernière 1,5 unité de temps (particules avec traînée). `head : 4px` : un point à la position actuelle.
- `colormap` : une couleur par trajectoire ; sinon `color`, `stroke`, `tube`, `glow` comme les autres courbes. En 3D, `follow(path=...)` suit la première trajectoire.
- Les solutions sont gardées d'une image à l'autre : quand `duration` grandit, le calcul n'est refait que lorsqu'elle double. Au plus 6 millions de points (trajectoires × pas).

### 15.11 Graphes et tableaux

**`graph`**

```
deffigure reseau =
    type    : graph
    nodes   : [A, B, C, D]
    edges   : [A-B, A-C, B->D, C-D]      # A->B : arête orientée
    weights : [4, 2, 5, 8]               # étiquettes des arêtes (facultatif)
    layout  : circle                     # circle, grid, tree, force
    pos     : (50%, 50%)                 # centre
    size    : 50%                        # étendue
```

- `layout : tree` : racine en haut (`root : A`, défaut : le premier nœud), chaque parent au milieu de ses enfants. `force` : disposition par ressorts (toujours la même). `positions : [(0, 1), (1, 2), ...]` : positions données (ramenées dans `size`).
- Apparence : `radius` (nœuds, défaut 3,5 %), `fill` (nœuds, défaut blanc), `color` (traits), `stroke`, `label_color`, `labels : false` ou une liste d'étiquettes.

**`array`** — tableau de valeurs, matrice :

```
deffigure m =
    type     : array
    values   : [[1, 2, 3], [4, 5, $\frac{1}{2}$]]
    pos      : (50%, 50%)
    cell     : 8%                        # taille d'une case
    brackets : true                      # crochets de matrice (défaut false)
    grid     : false                     # traits entre les cases (défaut true)
```

`color` : texte ; `fill` : fond des cases (transparent par défaut) ; `grid_color`, `font`. Les valeurs acceptent `$...$` et `#{...}`.

**Surligner un morceau :** l'animation `color(to=..., part=...)` vise un nœud (`part=B`), une arête (`part="A-C"`, dans un sens ou l'autre si elle n'est pas orientée), une case (`part="(2,3)"`, ligne puis colonne à partir de 1), une ligne ou une colonne (`part="row 2"`, `part="col 3"`). Pour une case, c'est le fond qui se colore. Enchaîner ces animations déroule un algorithme pas à pas (parcours, plus court chemin, pivot de Gauss).

### 15.12 Une couleur par pixel : `pixels`

Dans un repère 2D, une couleur pour chaque pixel du cadre, donnée d'une des trois façons :

```
deffigure chaleur =
    type     : pixels
    axes     : r
    expr     : sin(x*y) + cos(x)         # valeur → couleur
    colormap : coolwarm                  # défaut viridis ; crange : [min, max]

deffigure domaine =
    type   : pixels
    axes   : r
    domain : (z^2 - 1)/(z^2 + 2i)         # coloration de domaine

deffigure mandelbrot =
    type       : pixels
    axes       : r
    escape     : z^2 + c                  # fractale : z ← f(z), c = le point du pixel
    iterations : 200                      # défaut 100
```

- `domain` : teinte = argument de f(z) (rouge pour un réel positif), clarté selon |f(z)| (zéros noirs, pôles blancs) ; `bands : false` enlève les bandes aux puissances de 2 du module.
- `escape` : z part de `z0` (défaut 0 ; `z0 : c` et une constante dans la formule pour un ensemble de Julia : `escape : z^2 + (-0.8 + 0.156i)`, `z0 : c`), couleur selon le nombre d'itérations avant |z| > `bailout` (défaut 2), lissée ; `inside` : couleur des points qui ne s'échappent pas (défaut noir) ; `colormap` (défaut magma).
- Avec la caméra 2D du repère (`center`, `zoom`), on zoome dans une fractale : penser à augmenter `iterations` avec le zoom (`iterations : 100 + 80*k`).
- `resolution` : calculs par pixel (défaut 1 ; 0.5 = quatre fois moins de calculs). Le calcul est réparti sur les cœurs et gardé d'une image à l'autre tant que la figure et son repère ne changent pas. La figure couvre le cadre du repère : la définir avant les courbes à poser dessus, ou lui donner `layer : -1`.

---

### 15.13 Vérifier un fichier sans le jouer : `--check`

```
didact cours.dct --check
didact cours.dct --check --format json
```

- **Erreurs** : celles qui empêchent de jouer le fichier (lecture, validation), et les figures qui ne peuvent pas être dessinées.
- **Avertissements**, trouvés en calculant la géométrie à chaque bloc, à chaque fin d'animation et toutes les 0,25 s :
  - une figure qui sort du cadre (à partir de 2 px sur une image de 1080 px de haut) ;
  - deux textes (`text`, `equation`, `latex`, `table`) qui se chevauchent alors qu'ils sont tous les deux pleinement affichés (un fondu enchaîné au même endroit n'est pas signalé).
- Si c'est voulu (une figure qui entre par le bord, deux étiquettes superposées exprès) : `allow : outside`, `allow : overlap` ou `allow : [outside, overlap]` sur la figure.
- `--format json` : une liste d'objets `{"severity": "error" | "warning", "code": "parse" | "validation" | "render" | "outside" | "overlap", "line", "time", "figure", "other", "message"}` (`null` quand l'information n'existe pas ; `other` : l'autre texte d'un chevauchement).
- Code de retour : 0 aucun problème, 2 seulement des avertissements, 1 au moins une erreur.

---

## 16. À décider

Cette section liste les points évoqués pendant le design mais qui n'ont pas été tranchés explicitement, ainsi que les questions ouvertes qui mériteraient une décision avant l'implémentation complète.

### Détails de propriétés non spécifiés

Sur les figures déjà définies, des propriétés supplémentaires pourraient être utiles :

- `rect` : un `corner_radius` pour des coins arrondis ?
- `circle` : différencier `circle` et `ellipse` (radius_x, radius_y) ?
- `star` : un ratio rayon intérieur / extérieur ?
- `text` : `bold`, `italic`, `font_family` ?
- `scatter` : forme des marqueurs (cercle, carré, croix, ...) ?
- `axes` : pas des graduations (`tick_step`) ?
- `table` : style des bordures ?
- `polygon` : possibilité de ne pas fermer la forme ?
- `audio` : option `loop` ?

### Format des couleurs

On utilise des noms (`red`, `blue`) et le hexadécimal (`#FF0000`) dans les exemples, mais on n'a pas formellement décidé quels formats sont acceptés. À trancher : `rgb()`, `rgba()`, `hsl()` ?

### Valeurs valides pour `anchor`

`center` a été utilisé, mais on n'a pas listé les autres valeurs possibles (`topleft`, `bottomright`, etc.).

### Précision des timestamps

On a décidé du format `XX.XXX` (secondes.millisecondes) mais la précision exacte au-delà du millième n'est pas définie. Arrondir ? Tolérer plus ?

### Une figure dans plusieurs groupes (implémenté, à valider)

Permis pour l'instant : les décalages et animations des groupes s'additionnent, dans l'ordre de définition des groupes.

### Effacer un commentaire sans le remplacer

Comment terminer un sous-titre sans en afficher un nouveau ? `comment : []` ? Une commande explicite ?

### Règles de rendu provisoires (implémentées, à valider)

Choisies pour pouvoir dessiner ; à confirmer ou changer :

- **`pos` d'un rect** : coin bas-gauche par défaut ; `anchor : center` → centre.
- **`px`** : pixels d'une scène de référence (taille de `window`, sinon 1080 px de haut), mis à l'échelle avec la fenêtre.
- **Valeurs par défaut** : `color : black`, `stroke : 2px`, `fill : none`, `opacity : 1`, `layer : 0`.
- **Étoile** : rayon intérieur = 0,382 × rayon extérieur (étoile régulière).
- **Couleurs** : noms (`red`, `blue`, `gold`, ...), `#RGB`, `#RRGGBB`, `#RRGGBBAA`, `none`.

### Type `image` : importer un visuel externe

Une figure `type : image` qui affiche un fichier PNG ou SVG :

```
deffigure schema =
    type : image
    file : schema.svg
    pos  : (50%, 50%)
    size : 40%
```

Permettrait d'utiliser des visuels produits par n'importe quel outil (Manim pour une image fixe, GeoGebra, Inkscape, ...) sans dépendance dans Didact. Le SVG serait dessiné avec `resvg` (même moteur que la scène, tiny-skia). À préciser : chemins relatifs au fichier `.dct`, `size` = largeur ou plus grand côté, embarquement web.

### Effets de bord du compilateur

Quels avertissements (non bloquants) le compilateur produit-il ? Par exemple :
- Figure définie mais jamais utilisée dans la timeline
- Style défini mais jamais référencé

### Unités acceptées

On a vu `%`, `px`, `s`, `ms`, `cm`, `deg`. Lesquelles sont obligatoirement supportées ? D'autres (`pt`, `em`, `rad`) ?

### Easing par défaut (implémenté, à valider)

Implémenté : `smooth` par défaut (démarre et finit lentement, comme 3Blue1Brown), au choix `linear`, `ease_in`, `ease_out` via `ease=...`. Les noms utilisent `_` car `-` n'est pas autorisé dans un identifiant.

### Coordonnées en pixels absolus

On a décidé que les positions étaient en pourcentages. Mais peut-on aussi utiliser des pixels absolus quand nécessaire ? Avec quelle syntaxe ?

### Détection des erreurs

Le compilateur produit des erreurs claires, mais on n'a pas formalisé la liste exhaustive ni le format des messages.