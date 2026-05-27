# Didact — Spécification complète v0.1

> Document de référence du projet. À lire en entier avant toute contribution ou nouvelle session de développement.

---

## 1. Vision du projet

Didact est un **langage de description d'animations éducatives**. L'idée de départ : permettre à n'importe qui — enseignant, vidéaste, scientifique, étudiant — de créer des vidéos de qualité style [3Blue1Brown](https://www.youtube.com/c/3blue1brown) sans avoir à coder en Python.

### Ce que Didact n'est pas

- Ce n'est **pas** un outil qui compile vers MP4 (du moins pas principalement)
- Ce n'est **pas** un clone de Manim ou PowerPoint
- Ce n'est **pas** un langage de programmation général

### Ce que Didact est

Un **format de description déclaratif** d'animations, pensé pour être :
- Lu et modifié par des humains et des IAs
- Embarqué partout (sites web, PDF, PowerPoint) comme une vidéo YouTube
- Open source natif (on partage le source, pas la vidéo compilée)
- Versionnable avec Git (un `git diff` sur une timeline est lisible)

---

## 2. Public cible

- **Enseignants** — cours animés, explications visuelles
- **Vidéastes** — style 3Blue1Brown, vulgarisation scientifique
- **Scientifiques** — présentations de conférences, papers animés
- **Étudiants** — exposés, projets
- **IAs** — génération automatique de contenu éducatif

---

## 3. Philosophie de design

### 3.1 Séparation stricte : définition vs comportement temporel

```
.dct.spc   ←  CE QUE c'est  (définition statique, sans temps)
.dct.tl    ←  CE QUI SE PASSE et QUAND (tout le comportement temporel)
```

Les figures ne savent pas ce qui leur arrivera. Tout le comportement est dans la timeline.

### 3.2 Lisibilité IA-first

La structure linéaire de la timeline permet à une IA de :
- Lire et comprendre le fichier en un seul passage
- Modifier un timing en changeant un seul chiffre
- Ajouter une figure sans risque de casser le reste

### 3.3 Deux niveaux de complexité

```
Niveau 2 — syntaxe Didact structurée      ←  usage principal
Niveau 3 — escape hatch raw.latex / raw.matplotlib  ←  pour les experts
```

Il n'y a pas de niveau 1 ultra-simplifié : **l'IA joue ce rôle**. L'utilisateur décrit en langage naturel, l'IA génère le niveau 2.

### 3.4 Moteur en arrière-plan

Didact ne réinvente pas le rendu mathématique :

```
Didact  →  compile vers  →  LaTeX       (équations, tableaux)
Didact  →  compile vers  →  Matplotlib  (graphes, courbes)
Didact  →  gère lui-même →  positionnement, animations, timeline
```

Comme TypeScript compile vers JavaScript : l'utilisateur écrit du Didact propre, les moteurs existants font le rendu lourd.

### 3.5 Embarquabilité universelle

```html
<didact src="cours.dct" lang="FR" />
```

Un fichier Didact peut être embarqué dans n'importe quel contexte, comme une vidéo YouTube. Le renderer tourne dans le navigateur (WebAssembly). Pas besoin de compiler vers MP4 sauf cas exceptionnel.

---

## 4. Structure des fichiers

### Mode fichier unique — petits projets

```
cours.dct
```

Contient toutes les sections dans l'ordre obligatoire :
```
[config]
[figures]
[timeline]
```

### Mode séparation complète — grands projets

```
cours.dct.cfg    ←  configuration
cours.dct.spc    ←  définitions des figures, styles, groupes
cours.dct.tl     ←  timeline (visuel + audio + commentaires)
```

**Règle stricte : pas de demi-mesure.** Soit tout dans un `.dct`, soit tout séparé en `.dct.cfg` + `.dct.spc` + `.dct.tl`.

---

## 5. Les trois symboles fondamentaux

```
=    ←  définition d'un objet          defstyle mon_style =
:    ←  assignation d'une propriété    color : red
=>   ←  moment dans la timeline        5.000 => { ... }
```

Ces trois symboles ont des rôles **strictement distincts** et non ambigus.

---

## 6. Fichier de configuration `.dct.cfg`

```
[config]
window       : auto / [1920, 1080] / [16:9]
background   : white
languages    : [FR, NL, EN]
default_lang : FR
```

### La variable `window`

```
window : auto          ←  s'adapte au conteneur (défaut)
window : [1920, 1080]  ←  taille fixe en pixels
window : [16:9]        ←  ratio fixe, taille adaptive
```

### Les langues

L'ordre de la liste `languages` définit l'ordre des traductions partout dans le projet :
```
languages : [FR, NL, EN]
# → index 0 = FR, index 1 = NL, index 2 = EN
```

---

## 7. Fichier de spécifications `.dct.spc`

### 7.1 Les mots-clés de définition

```
defstyle   ←  définit un style réutilisable
deffigure  ←  définit une figure (objet visuel)
defgroup   ←  définit un groupe de figures
```

**Pourquoi des préfixes `def` ?**
Sans préfixe, `style` serait ambigu : mot-clé de définition ou nom de propriété ?
```
style : mon_style        ←  propriété qui référence un style (dans une figure)
defstyle mon_style =     ←  définition d'un nouveau style
```

### 7.2 Les styles

```
defstyle trait_fin =
    color   : red
    width   : 2px
    opacity : 1.0

# Héritage
defstyle trait_epais =
    extends : trait_fin    ←  hérite de trait_fin
    width   : 4px          ←  écrase width
```

### 7.3 Propriétés communes à toutes les figures

```
color    : red / #FF0000 / rgb(255, 0, 0)
fill     : none / red / #FF0000
opacity  : 0.0 → 1.0
width    : épaisseur du trait (ex: 2px)
style    : nom_du_style
layer    : ordre d'affichage (entier)
anchor   : center / topleft / topright / bottomleft / bottomright
```

### 7.4 Le système de coordonnées

**Positionnement dans la scène :**
```
pos : (50%, 50%)    ←  pourcentages, relatifs à la taille de la scène
                        (0%, 0%) = bas gauche
                        (100%, 100%) = haut droit
```

**Cohérence mathématique :** l'origine (0,0) est en bas à gauche, comme en mathématiques. Cohérent avec matplotlib et les conventions scientifiques.

**Référence à un repère d'axes :**
```
pos : axes_xy(3, 4)    ←  coordonnées dans le repère de la figure axes_xy
```

**Références dynamiques :**
```
center(rect_mobile)         ←  centre d'une figure, recalculé à chaque frame
offset(arc_theta, 1%)       ←  décalage relatif à une figure
angle(ligne_direction)      ←  angle dynamique d'une figure
auto_tangent(courbe_sin)    ←  rotation parallèle à la tangente d'une courbe
```

### 7.5 Convention des angles

```
angle positif  →  sens trigonométrique (anti-horaire)
angle négatif  →  sens horaire
```

Identique aux conventions mathématiques standard.

### 7.6 Types de figures — Géométrie

**`line` — segment / vecteur / flèche**
```
deffigure ma_ligne =
    type    : line
    from    : (20%, 50%)
    to      : (80%, 50%)
    color   : red
    width   : 2px
    arrow   : none / start / end / both
    style   : mon_style
```

**`circle` — cercle / ellipse**
```
deffigure mon_cercle =
    type    : circle
    center  : (50%, 50%)
    radius  : 10%
    color   : blue
    fill    : blue
    opacity : 0.5
```

**`rect` — rectangle / carré**
```
deffigure mon_rect =
    type    : rect
    pos     : (20%, 20%)
    width   : 30%
    height  : 20%
    color   : green
    fill    : none
```

**`polygon` — forme libre**
```
deffigure mon_triangle =
    type    : polygon
    points  : [(20%, 80%), (50%, 20%), (80%, 80%)]
    color   : black
    fill    : yellow
```

**`arc` — portion de cercle (notation d'angle)**
```
deffigure mon_arc =
    type        : arc
    center      : (50%, 50%)
    from_angle  : 0
    to_angle    : 90
    radius      : 10%
    color       : red
    width       : 2px
    arrow       : end
    # direction implicite : positif = trigo, négatif = horaire
```

**`star` — étoile**
```
deffigure mon_etoile =
    type    : star
    points  : 5
    pos     : (50%, 50%)
    size    : 2%
    color   : gold
    fill    : gold
```

### 7.7 Types de figures — Texte et maths

**`text` — texte simple**
```
deffigure mon_texte =
    type    : text
    content : "Bonjour"
    pos     : (50%, 50%)
    font    : 24px
    color   : black
    align   : center / left / right
```

**`equation` — équation LaTeX inline**
```
deffigure mon_eq =
    type    : equation
    expr    : \int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
    pos     : (50%, 50%)
    size    : 32px
    color   : black
```

**`latex` — bloc LaTeX complet**
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

**`table` — tableau**
```
deffigure ma_table =
    type    : table
    pos     : (50%, 50%)
    header  : [x, f(x), f'(x)]
    rows    : |
        0 | 1  | 0
        1 | e  | e
        2 | e² | 2e²
```

### 7.8 Types de figures — Graphes

**`axes` — système d'axes**
```
deffigure mes_axes =
    type    : axes
    origin  : (10%, 50%)
    xrange  : [0, 10]
    yrange  : [-1, 1]
    xlabel  : "x"
    ylabel  : "f(x)"
    grid    : true / false
    ticks   : true / false
```

**`plot` — courbe f(x)**
```
deffigure mon_plot =
    type    : plot
    expr    : sin(x)
    axes    : mes_axes    ←  se place dans le repère de mes_axes
    color   : red
    width   : 2px
```

**`scatter` — nuage de points**
```
deffigure mon_scatter =
    type    : scatter
    points  : [(1, 2), (3, 4), (5, 1)]
    axes    : mes_axes
    color   : blue
    size    : 5px
```

**`histogram` — histogramme**
```
deffigure mon_histo =
    type    : histogram
    data    : [1, 2, 2, 3, 3, 3, 4]
    bins    : 10
    axes    : mes_axes
    color   : green
```

### 7.9 Groupes

```
defgroup mon_graphe =
    items : [mes_axes, mon_plot, mon_scatter]
    pos   : (50%, 50%)
```

Un groupe se déplace, s'anime et apparaît/disparaît comme un seul objet.

### 7.10 Escape hatches

Pour les utilisateurs qui connaissent déjà LaTeX ou Matplotlib :

```
deffigure mon_raw_latex =
    type    : raw.latex
    code    : |
        \begin{equation}
            \int_0^\infty e^{-x^2} dx
        \end{equation}

deffigure mon_raw_matplotlib =
    type    : raw.matplotlib
    code    : |
        x = np.linspace(0, 10, 100)
        plt.plot(x, np.sin(x), color='red')
```

---

## 8. Fichier timeline `.dct.tl`

### 8.1 Format des timestamps

```
61.500 =>    ←  61 secondes et 500 millisecondes
```

**Règle :** secondes entières + millisecondes après le point.

### 8.2 Structure d'un bloc timeline

```
5.000 => {
    start   : [figure1, figure2]
    end     : [figure3 fadeout(0.5s)]
    destroy : [figure4]
    anim    : [figure1 move(to=(80%, 50%), 2s)]
    comment : ["texte FR", "tekst NL", "text EN"]
}
```

Tout ce qui se passe au même instant est regroupé dans un seul bloc `{}`.

### 8.3 Les commandes

**`start`** — rend une figure visible
```
start : [ma_ligne, mon_cercle]
start : [audio musique.mp3 volume=0.3]
```

**`end`** — disparition visuelle, **la figure reste en mémoire**
```
end : [ma_ligne]
end : [ma_ligne fadeout(0.5s)]
```

**`destroy`** — suppression complète, libère la mémoire
```
destroy : [ma_ligne]
```

> ⚠️ **Règle importante :** `destroy` sur une figure encore référencée par une autre → erreur de compilation.

**Différence `end` vs `destroy` :**
```
# Cas d'usage typique :
30.000 => {
    end     : [courbe_sin fadeout(0.5s)]   ←  invisible mais le chemin existe encore
}
# rect_mobile continue à suivre courbe_sin comme référence de chemin

55.000 => {
    end     : [rect_mobile fadeout(0.3s)]
}
56.000 => {
    destroy : [courbe_sin, rect_mobile]    ←  maintenant safe
}
```

**`anim`** — déclenche une animation
```
anim : [ma_ligne move(to=(80%, 50%), 2s)]
anim : [ma_ligne [move(to=(50%, 50%), 1s), color(to=blue, 0.5s)]]   ←  enchaînées
```

**`comment`** — texte/sous-titre multilingue
```
comment : ["texte FR", "tekst NL", "text EN"]
```
L'ordre correspond à l'ordre de `languages` dans `[config]`.

### 8.4 Toutes les animations

**Apparition**
```
fadein(0.5s)
slidein(left, 0.5s)     ←  left / right / top / bottom
grow(0.5s)
write(1s)               ←  s'écrit progressivement (texte/équation)
draw(1s)                ←  se dessine progressivement (géométrie)
```

**Mouvement**
```
move(to=(80%, 50%), 2s)
move(down, 3cm/s)                              ←  direction + vitesse
follow(path=courbe_sin, speed=1cm/s, rotate=tangent)
rotate(360deg, 2s)
```

**Transformation**
```
transform(to=mon_cercle, 1s)    ←  morphe vers une autre figure
color(to=blue, 1s)
scale(2.0, 1s)
```

**Disparition**
```
fadeout(0.5s)
slideout(right, 0.5s)
shrink(0.5s)
```

**Par défaut :** apparition instantanée (sans animation). Toutes les animations sont opt-in dans la timeline.

---

## 9. Audio et commentaires

L'audio et les commentaires sont dans la même timeline `.dct.tl` (pas de fichier séparé) :

```
0.000 => {
    start   : [audio musique.mp3 volume=0.3]
}

5.000 => {
    comment : ["la barre descend", "de balk daalt", "the bar goes down"]
}

45.000 => {
    end : [audio musique.mp3]
}
```

### Traduction

Un seul fichier source, autant de langues que nécessaire. Le choix de langue est un paramètre à l'embarquement :

```html
<didact src="cours.dct" lang="FR" />
<didact src="cours.dct" lang="EN" />
```

---

## 10. Architecture technique

### 10.1 Le compilateur

Le compilateur Didact est écrit en **Rust** :
- Performances maximales
- Compile vers WebAssembly → tourne dans le navigateur
- Un seul exécutable, rien à installer pour l'utilisateur final
- Typage strict → moins de bugs

### 10.2 Pipeline de compilation

```
Fichier .dct
    ↓
Lexer (tokenisation)
    ↓
Parser (AST — arbre syntaxique)
    ↓
Validation sémantique
    ↓
Renderers
    ├── LaTeX        →  équations, tableaux
    ├── Matplotlib   →  graphes, courbes
    └── Didact natif →  géométrie, animations, timeline
    ↓
Output
    ├── Navigateur (WebAssembly / Canvas)   ←  principal
    ├── MP4                                 ←  optionnel
    └── Podcast (TTS)                       ←  plus tard
```

### 10.3 Structure du projet Rust

```
didact/
  Cargo.toml
  src/
    main.rs      ←  point d'entrée CLI
    lexer.rs     ←  tokenisation
    ast.rs       ←  structures de données (AST)
    parser.rs    ←  parser récursif descendant
```

---

## 11. Exemple complet

Voir `exemple.dct` dans le repo pour un exemple complet avec :
- Axes sans graduation
- Courbe sinus
- Rectangle orange qui suit la tangente de la courbe
- Étoile à une position précise dans le repère
- Ligne dynamique vers l'étoile
- Arc pour l'angle theta avec label

---

## 12. État d'avancement

### ✅ Fait

- Spécification du langage (v0.1)
- Lexer complet
- Parser (config + figures + timeline)
- Fichier exemple

### 🔲 Prochaines étapes

- [ ] Validation sémantique (vérifier les références, les destroy, etc.)
- [ ] Renderer basique dans le navigateur (Canvas/WebGL)
- [ ] Intégration LaTeX pour les équations
- [ ] Intégration Matplotlib pour les graphes
- [ ] Support fichiers séparés (`.dct.cfg` / `.dct.spc` / `.dct.tl`)
- [ ] Interface Overleaf-like (éditeur web + prévisualisation)
- [ ] Export MP4 optionnel
- [ ] Mode podcast (TTS)
- [ ] Packages communautaires (figures réutilisables)

### 🔲 Reporté à plus tard

- Optimisation stockage / format binaire
- Mode syllabus (idée incertaine)
- Traduction automatique par IA

---

## 13. Décisions de design actées

Cette section résume les choix importants et leurs raisons, pour ne pas les remettre en question inutilement.

| Décision | Choix | Raison |
|---|---|---|
| Format de sortie principal | Navigateur (pas MP4) | Embarquable, vivant, pas besoin de recompiler |
| Nombre de niveaux de syntaxe | 2 (pas 3) | L'IA remplace le niveau 1 simplifié |
| Unité de temps | Secondes.millisecondes | Simple, non ambigu, facile à parser |
| Noms des figures | Libres (style Python) | Lisibilité humaine et IA |
| Coordonnées | Pourcentages | Responsive, indépendant de la résolution |
| Origine du repère | Bas gauche (0,0) | Convention mathématique standard |
| Angles | Positif = trigo | Convention mathématique standard |
| Animation par défaut | Instantanée | Opt-in pour les animations |
| end vs destroy | Deux commandes distinctes | end = invisible, destroy = libère la mémoire |
| Mots-clés de définition | defstyle / deffigure / defgroup | Non ambigus, pas de lookahead nécessaire |
| Langage du compilateur | Rust | WebAssembly, performances, un seul exécutable |
| Moteur de rendu | LaTeX + Matplotlib en arrière-plan | Ne pas réinventer le rendu mathématique |
| Format binaire | Non (pour l'instant) | Gain négligeable, perd la lisibilité |
| Timeline audio | Dans .dct.tl (pas séparé) | Vision globale en un coup d'œil |