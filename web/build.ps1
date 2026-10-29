# ─────────────────────────────────────────────
#  Compile Didact en WebAssembly pour le web (Windows / PowerShell)
#  À lancer depuis la racine du projet :  .\web\build.ps1
#
#  Une seule fois avant :
#    rustup target add wasm32-unknown-unknown
#    cargo install wasm-bindgen-cli --version 0.2.129
#  (la version doit être exactement celle de Cargo.toml)
# ─────────────────────────────────────────────
$ErrorActionPreference = "Stop"

cargo build --lib --release --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

wasm-bindgen --target web --no-typescript --out-dir web/pkg --out-name didact `
    target/wasm32-unknown-unknown/release/didact.wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$size = [math]::Round((Get-Item web/pkg/didact_bg.wasm).Length / 1MB, 1)
Write-Host "OK : web/pkg/didact_bg.wasm ($size Mo)"
Write-Host "Pour tester : python -m http.server 8000   puis ouvrir http://localhost:8000/web/"
