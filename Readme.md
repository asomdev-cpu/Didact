# Didact

> Langage de description d'animations éducatives — style 3Blue1Brown, accessible à tous, embarquable partout.

**Statut : prototype — parser v0.1**

---

## Qu'est-ce que Didact ?

Didact permet de créer des vidéos animées éducatives en écrivant un fichier texte simple. Pas de code Python, pas de PowerPoint, pas de compilation vers MP4 à chaque modification.

```
# mon_cours.dct

[config]
window : [16:9]
languages : [FR, EN]

[figures]

deffigure mes_axes =
    type   : axes
    origin : (10%, 50%)
    xrange : [0, 10]
    yrange : [-1, 1]

deffigure courbe_sin =
    type  : plot
    expr  : sin(x)
    axes  : mes_axes
    color : red

[timeline]

0.000 => {
    start   : [mes_axes, courbe_sin]
    comment : ["Voici la fonction sinus", "Here is the sine function"]
}

5.000 => {
    anim : [courbe_sin color(to=blue, 1s)]
}
```

Le fichier tourne directement dans le navigateur. On le partage comme du code, pas comme une vidéo.

---

## Pourquoi Didact ?

| Outil | Problème |
|---|---|
| Manim | Faut coder en Python, pas embarquable |
| PowerPoint | Fermé, pas versionnable, pas IA-friendly |
| LaTeX Beamer | Statique, pas animé |
| Canva/Prezi | Fermé, pas open source |
| **Didact** | Texte lisible, embarquable, open source, IA-friendly |

---

## Installation

```bash
# Installer Rust si pas déjà fait : https://rustup.rs
cargo build
cargo run -- exemple.dct
```

---

## Documentation complète

→ Voir **[DIDACT_SPEC.md](DIDACT_SPEC.md)** pour la spécification complète du langage.

---

## Structure du projet

```
didact/
  Cargo.toml
  exemple.dct       ←  exemple complet
  README.md
  DIDACT_SPEC.md    ←  spécification complète
  src/
    main.rs         ←  point d'entrée
    lexer.rs        ←  tokenisation
    ast.rs          ←  structures de données
    parser.rs       ←  parser
```

---

## Contribuer

Le projet en est à ses débuts. Les contributions bienvenues :
- Renderer navigateur (Canvas/WebGL/WebAssembly)
- Intégration LaTeX
- Intégration Matplotlib
- Interface Overleaf-like

---

## Licence

À définir.