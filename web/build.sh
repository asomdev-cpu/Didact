#!/bin/sh
# Compile Didact en WebAssembly pour le web (Linux / macOS). Voir web/build.ps1 pour les prérequis.
set -e
cargo build --lib --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg --out-name didact \
    target/wasm32-unknown-unknown/release/didact.wasm
echo "OK : web/pkg/didact_bg.wasm ($(du -h web/pkg/didact_bg.wasm | cut -f1))"
echo "Pour tester : python3 -m http.server 8000   puis ouvrir http://localhost:8000/web/"
