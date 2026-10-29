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
15. [À décider](#15-à-décider)

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

Le préfixe `def` évite l'ambiguïté avec les noms de propriétés :

```
deffigure ma_ligne =
    style : trait_fin        # ← "style" est ici une propriété

defstyle trait_fin =         # ← "defstyle" est un mot-clé de définition
    color : red
```

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
    anim  : [rect_mobile follow(path=courbe_sin, speed=1cm/s)]
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

Le contenu de `expr` est du LaTeX en mode mathématique, lu tel quel jusqu'à la fin de la ligne (pas de guillemets), affiché en formule centrée (bornes d'intégrales et de sommes au-dessus et en dessous). `size` : taille (défaut 40px).

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
- Variable `x`, constantes `pi` et `e`, `+ - * / ^`, parenthèses, multiplication implicite (`2x`, `3(x+1)`, `2sin(x)`), fonctions `sin cos tan asin acos atan sinh cosh tanh exp ln log sqrt abs floor ceil`.
- Tracée sur tout `xrange` du repère ; coupée hors de `yrange` et là où elle n'est pas définie. `samples` : nombre de points (défaut : un point tous les 2 pixels environ).

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
    pos   : (50%, 50%)
```

Un groupe se déplace, s'anime et apparaît/disparaît comme un seul objet.

---

## 11. Animations

### 11.1 Apparition

```
fadein(durée)
slidein(direction, durée)      # direction : left / right / top / bottom
grow(durée)
write(durée)                   # s'écrit progressivement (texte/équation)
draw(durée)                    # se dessine progressivement (géométrie)
```

### 11.2 Mouvement

```
move(to=position, durée)
move(direction, vitesse)       # ex: move(down, 3cm/s)
follow(path=fig, durée, rotate=tangent)    # parcourt tout le contour de fig, à vitesse régulière
rotate(angle, durée)
```

### 11.3 Transformation

```
color(to=couleur, durée)
scale(facteur, durée)
transform(to=fig, durée)       # morphe vers une autre figure
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
| `rotate(angle, d)` | tourne **de** `angle` degrés autour du centre (positif = sens trigonométrique), s'additionne |

| `follow(path=fig, d)` | le **centre** parcourt le contour de `fig` (courbe, ligne, arc, cercle, ...) du début à la fin, à vitesse régulière ; `rotate=tangent` : la figure s'oriente selon la tangente |

Pas encore implémentées (ignorées avec un avertissement) : `slidein`, `slideout`, `write`, `draw`, `transform`, `move(direction, vitesse)`.

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

### 13.2 Commentaires multilingues

L'ordre des traductions correspond à l'ordre déclaré dans `languages` du `[config]` :

```
# si languages = [FR, NL, EN]
comment : ["la barre descend", "de balk daalt", "the bar goes down"]
```

### 13.3 Sélection de la langue au rendu

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

## 15. À décider

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

### Une figure dans plusieurs groupes

Une même figure peut-elle appartenir à plusieurs groupes simultanément ?

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