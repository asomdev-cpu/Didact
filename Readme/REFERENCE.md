# Didact — fiche de référence

Tout le langage sur une page. Détails : [SPEC.md](SPEC.md).

```
[config]
window : [16:9]                 # ou [1920, 1080], auto
background : white              # couleur de fond ; default_lang : FR ; decimal : comma (2,5)
languages : [FR, EN]            # ordre des traductions de comment

[figures]
defparam k = 1                  # nombre animable : k to(3, 2s)
deffunction f(x) = x^2 - k      # utilisable dans toutes les formules
defstyle s =
    color : red                 # extends : autre_style
deffigure nom =
    type : ...
    style : s
defgroup g =
    items : [a, b]              # offset : (dx%, dy%)
defmodel m(a, b) =              # @a, @(calcul) ; deffigure x = m(1, red)
defrepeat rr = m(@i, 2) for i in 0..7     # rr_0 … rr_7 + groupe rr

[timeline]
1.500 => {
    destroy : [x]               # ordre fixe : destroy, end, start, anim, comment
    end     : [a fadeout(0.5s)]
    start   : [b, c draw(1s), audio son.mp3 volume=0.5 loop]
    anim    : [b move(to=(80%, 50%), 2s), k to(3, 2s), c [rotate(90, 1s), scale(2, 1s)]]
    comment : ["Texte FR", "Text EN"]          # comment : none efface
}

```

**Règles :** référencer seulement ce qui est défini plus haut · une propriété par ligne · instants uniques et croissants (`s.mmm` ; insérer : `didact shift`) · une seule langue : `comment : "Texte"` · virgule = en parallèle, `fig [a, b]` = à la suite · nom inconnu = erreur.

## Méthode de travail

1. Écrire le `.dct`.
2. `didact f.dct --check` jusqu'à « aucun problème » : chaque erreur donne la ligne et souvent une suggestion ; les avertissements signalent une figure hors du cadre ou deux textes qui se chevauchent, avec l'instant.
3. `didact f.dct --png 2.5 img.png` aux instants clés, et regarder l'image.

Mise en page : écran 16:9, marges d'environ 5 %, bas de l'écran (y < 12 %) réservé aux sous-titres (`comment`). Tailles usuelles : titre `48px`, texte `32px`, formule `40px`.

## Valeurs

| | |
|---|---|
| Position | `(x%, y%)` origine en bas à gauche · `r(x, y)` dans le repère `r` · positions dynamiques ci-dessous |
| Taille | `%` du plus petit côté (`width = height` → carré) · `px` (sur 1080 px de haut) |
| Angle | degrés ou `rad`, positif = anti-horaire |
| Couleur | `red`, `#RRGGBB[AA]`, `hsl(t, s%, l%[, a])`, `none` |
| Formule | `x`, params, `pi e`, `+ - * / ^`, `2x`, `sin cos tan asin acos atan sinh cosh tanh exp ln log sqrt abs floor ceil round sign`, `min max mod clamp atan2 hypot`, `if(c, a, b)`, `< ==`, `and or not`, `sum(k, 1, n, …)`, `prod`, `rand(k) randn(k)`, `fact binom gamma erf` ; complexes : `z i re im arg conj` |
| Texte | `"…"` ; `$LaTeX$` ; `#{expr}`, `#{expr:0}` ; bloc : `|` seul puis lignes indentées |
| Listes | `[a, b]`, couples `(a, b)` ; un élément peut être un calcul : `[0, 2*pi]` |
| Brut | `expr`, `polar`, `part`, `where`, `upper`, `lower`, `cmap`… : lus jusqu'à la fin de la ligne, sans guillemets |

**Positions dynamiques :** `center(f)` · `top(f)` `bottom` `left` `right` `topleft` `topright` `bottomleft` `bottomright` · `center(eq, part="\frac{a}{b}")`, `above(g, 1%, part="B")` (nœud, case) · `below(f, 2%[, left|center|right])`, `above`, `left_of(f, 1%[, top|center|bottom])`, `right_of` (ancre implicite) · `offset(f, d)` (étiquette : au bout d'une flèche, hors d'un arc, sous une accolade) · `start(f)` `end(f)` · `vertex(p, i)` `side(p, i)` · `angle(f)` · `auto_tangent(c)`.

**Communes :** `color` `fill` `opacity` `stroke` (2px) `style` `layer` `rotate` `glow` `allow : outside|overlap` `clip` `axes` `matrix` `map` `cmap` `amount` · `anchor` (topleft … bottomright) sur rect text equation latex raw.latex table graph array image (boîte ancrée)

## Types (obligatoires en gras)

| Type | Propriétés |
|---|---|
| `line` | **from to** arrow extend (droite, demi-droite) |
| `circle` | **center radius** (`(rx, ry)` ; sans unité = unités de `axes`, sinon `%` ou `px`) |
| `rect` | **pos width height** corner_radius |
| `polygon` | **points** closed ; ou régulier : **sides center radius** |
| `brace` | **from to** size (accolade ; étiquette en `offset`) |
| `arc` | **center radius from_angle to_angle** arrow |
| `star` | **pos size** points inner |
| `arrow` | **from to** head axes |
| `image` | **file pos size** |
| `text` | **content pos** size align |
| `equation` | **expr pos** size align |
| `latex` / `raw.latex` | **content** / **code**, **pos** size align (`\\` = nouvelle ligne) |
| `table` | **rows pos** header size align |
| `axes` | **from to xrange yrange** equal grid ticks step xlabel ylabel center zoom align_x align_y same_scale |
| `plot` | **axes expr** xrange samples |
| `curve` | **axes** + **expr (x(t), y(t)) t** ou **polar** ; samples |
| `tangent` | **axes expr at** length |
| `riemann` | **axes expr xrange** n method (left mid right) |
| `frame` | **around** part padding corner_radius |
| `grid` | **axes** step xrange yrange |
| `scatter` | **axes points** (ou count x y where) size marker |
| `histogram` | **axes data** (ou count sample) bins range density |
| `contour` | **axes expr** (f ou g = h) levels colormap resolution xrange yrange |
| `region` | **axes upper** (lower xrange) ou **where** (xrange yrange) ; resolution |
| `field` | **axes expr (fx, fy)** (ou scene, (fx, fy, fz)) density length normalize colormap xrange yrange zrange |
| `pixels` | **axes** + **expr** / **domain** (bands) / **escape** (z0 iterations bailout inside) ; colormap crange resolution |
| `trajectory` | **axes** (ou scene) **ode init duration** vars dt count trail head colormap tube |
| `iterate` | **axes** (ou scene) vars init update steps ; ou on start method (gd momentum nesterov rmsprop adam newton) rate beta beta2 eps ; markers lift drape |
| `graph` | **nodes edges pos** size weights layout (circle grid tree force) root positions radius labels label_color font initial accepting (automate ; `A->A` boucle) |
| `array` | **values pos** cell brackets grid grid_color font |
| `scene3d` | xrange yrange zrange camera projection fov zoom target equal box light ambient frame floor labels from to |
| `surface` | scene **expr** (urange vrange) xrange yrange resolution grid colormap color_by crange levels levels_on level_color level_stroke |
| `curve3d` | scene **expr t** (ou points) samples markers |
| `cube box plane sphere cylinder cone polyhedron` | scene size grid / radius height / **points faces** ; center rotate resolution shading tube |

## Animations (durée obligatoire : `1s`, `500ms` ; `ease=smooth|linear|ease_in|ease_out`)

| Entrée / sortie | Mouvement | Changement |
|---|---|---|
| `fadein` `fadeout` `grow` `shrink` `draw` `write` `slidein(left, d)` `slideout(right, d)` | `move(to=pos, d)` `move(down, 20%, d)` `follow(path=c, d, rotate=tangent)` `rotate(90, d[, around=pos])` `rotate3d(90, axis=x, d)` | `scale(2, d)` `swap(2, 3, d)` (cases d'un array) `color(to=red, d[, part="…"])` `fill(to=red, d)` `transform(to=b, d[, rigid=true])` `to(3, d)` (paramètre) |

`part` : morceau LaTeX d'une équation, texte d'un `text`, `B` / `"A-C"` d'un graph, `"(2,3)"`, `"row 2"`, `"col 1"`, `"text (2,3)"` d'un array.

## Recettes

- Point mobile : `center : r(a, f(a))` + `a to(3, 2s)` · sécante : `line` + `extend : true` · trace : `curve` avec `t : [0, T]`.
- Apparition sur seuil : `opacity : clamp(n - 3, 0, 1)` · compteur : `"n = #{round(n):0}"`.
- Calcul par étapes : `e1 transform(to=e2, 1s)`, puis `e2 transform(to=e3, 1s)` (équations `align : left`).
- Rotation du plan : `matrix : (cos(th), -sin(th), sin(th), cos(th))` dans un `defstyle`.
- Repère invisible : ne pas démarrer l'`axes`. Plus de recettes : SPEC §12.

## Commandes

`didact f.dct` (fenêtre) · `--check [--format json]` · `--png 2.5 img.png` · `--export mp4|html|audio` · `--voice` · `--lang EN` · `didact shift f.dct 16 +2`
