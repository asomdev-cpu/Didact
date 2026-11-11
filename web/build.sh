#!/bin/sh
# Compile Didact en WebAssembly pour le web (Linux / macOS). Voir web/build.ps1 pour les prérequis.
# OPT=1 ./web/build.sh : passe en plus wasm-opt -O2 (Binaryen, à installer à part).
set -e
cargo build --lib --profile web --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section \
    --out-dir web/pkg --out-name didact target/wasm32-unknown-unknown/web/didact.wasm
WASM=web/pkg/didact_bg.wasm
if [ -n "$OPT" ]; then
    wasm-opt "$WASM" -O2 --enable-bulk-memory --enable-nontrapping-float-to-int \
        --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue -o "$WASM"
fi
echo "OK : $WASM ($(du -h "$WASM" | cut -f1))"
echo "Pour tester : python3 -m http.server 8000   puis ouvrir http://localhost:8000/web/"
