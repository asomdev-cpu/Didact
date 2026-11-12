# Didact

> Langage de description d'animations éducatives — style 3Blue1Brown, accessible à tous, embarquable partout.

**Statut :** prototype fonctionnel — 2D et 3D, fenêtre, export MP4, HTML et audio, voix de synthèse.

---

## Qu'est-ce que Didact ?

Didact permet de créer des animations éducatives en écrivant un fichier texte simple. Pas de code Python, pas de PowerPoint, pas de compilation vers MP4.

```
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

Le compilateur lit ce fichier et ouvre une fenêtre avec l'animation — comme matplotlib avec ses graphes.

---

## Pourquoi Didact ?

| Outil | Limite |
|---|---|
| Manim | Code Python obligatoire, pas embarquable |
| PowerPoint | Fermé, pas versionnable, pas IA-friendly |
| LaTeX Beamer | Statique, pas animé |
| Canva / Prezi | Fermé, pas open source |
| **Didact** | Texte lisible, embarquable, open source, IA-friendly |

---

## Documentation

| Document | Pour qui |
|---|---|
| [**REFERENCE.md**](Readme/REFERENCE.md) | Tous (et les IA) — tout le langage sur une page |
| [**SPEC.md**](Readme/SPEC.md) | Utilisateurs — la référence complète pour écrire un fichier `.dct` |
| [**ARCHITECTURE.md**](Readme/ARCHITECTURE.md) | Développeurs — comment fonctionne le compilateur |
| [**DECISIONS.md**](Readme/DECISIONS.md) | Tous — journal de design, état d'avancement, roadmap |
| [**WEB.md**](Readme/WEB.md) | Intégrer une animation dans une page web |

---

## Installation et test

```bash
# Installer Rust : https://rustup.rs
cargo build
cargo run -- exemple.dct
```

Exports :

```bash
cargo run --release -- cours.dct --export mp4            # vidéo (nécessite ffmpeg)
cargo run --release -- cours.dct --export mp4 --lang EN  # sous-titres anglais → cours_EN.mp4
cargo run --release -- cours.dct --export html           # page autonome (nécessite web/pkg, voir Readme/WEB.md)
cargo run --release -- cours.dct --voice                 # commentaires lus par une voix de synthèse (voir didact-voix.txt)
cargo run --release -- cours.dct --export audio          # le son seul en MP3 (« podcast »)
```

Toutes les options : `cargo run -- --help`.

---

## Structure du projet

```
didact/
├── README.md            ← ce fichier
├── Readme/              ← REFERENCE, SPEC, ARCHITECTURE, DECISIONS, WEB
├── demo_*.dct           ← démos (une par thème)
├── exemple.dct          ← exemple complet
├── Cargo.toml
└── src/
    ├── main.rs          ← point d'entrée CLI
    ├── lexer.rs         ← tokenisation
    ├── ast.rs           ← structures de données
    └── parser.rs        ← parser récursif descendant
```

---

## Contribuer

Le projet est aux tout débuts. Pour contribuer :

1. Lire `Readme/REFERENCE.md` puis `Readme/SPEC.md` (comprendre le langage)
2. Lire `ARCHITECTURE.md` (comprendre le compilateur)
3. Lire `DECISIONS.md` (voir ce qui est prioritaire)
4. Ouvrir une issue ou une PR

---

## Licence

MIT — voir [LICENSE](LICENSE).