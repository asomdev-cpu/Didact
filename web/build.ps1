# ─────────────────────────────────────────────
#  Compile Didact en WebAssembly pour le web (Windows / PowerShell)
#  À lancer depuis la racine du projet :  .\web\build.ps1
#
#  Une seule fois avant :
#    rustup target add wasm32-unknown-unknown
#    cargo install wasm-bindgen-cli --version 0.2.129
#  (la version doit être exactement celle de Cargo.toml)
#
#  .\web\build.ps1 -Opt : passe en plus wasm-opt -O2 (Binaryen, à installer à part) ;
#  gain mesuré faible une fois le fichier compressé par le serveur (voir Readme/WEB.md).
# ─────────────────────────────────────────────
param([switch]$Opt)
$ErrorActionPreference = "Stop"

# Profil « web » (Cargo.toml) : optimisation sur tout le programme (LTO)
cargo build --lib --profile web --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Sans les noms de fonctions (7 Mo, utiles seulement pour déboguer)
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section `
    --out-dir web/pkg --out-name didact target/wasm32-unknown-unknown/web/didact.wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$wasm = "web/pkg/didact_bg.wasm"
if ($Opt) {
    wasm-opt $wasm -O2 --enable-bulk-memory --enable-nontrapping-float-to-int `
        --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue -o $wasm
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$size = [math]::Round((Get-Item $wasm).Length / 1MB, 1)
Write-Host "OK : $wasm ($size Mo)"
Write-Host "Pour tester : python -m http.server 8000   puis ouvrir http://localhost:8000/web/"
