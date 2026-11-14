#!/usr/bin/env bash
set -euo pipefail

script_directory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(dirname "$script_directory")"
artifact_directory="$project_root/artifacts/north-seas-adventure"
wasm_directory="$artifact_directory/src/wasm-bindgen"
public_directory="$artifact_directory/dist/public"

cd "$project_root"
"$project_root/scripts/with-rust-toolchain.sh" wasm-pack build \
  --target web \
  --release \
  --out-dir artifacts/north-seas-adventure/src/wasm-bindgen \
  --out-name north_sea

rm -rf -- "$public_directory"
mkdir -p "$public_directory/src/wasm-bindgen"
cp -- "$artifact_directory/index.html" "$public_directory/index.html"
cp -- "$artifact_directory/public/game.css" "$public_directory/game.css"
cp -- "$artifact_directory/public/compass.svg" "$public_directory/compass.svg"
cp -- "$project_root/assets/icons/north-sea.png" "$public_directory/north-sea.png"
cp -- "$wasm_directory/north_sea.js" "$public_directory/src/wasm-bindgen/"
cp -- "$wasm_directory/north_sea_bg.wasm" "$public_directory/src/wasm-bindgen/"

printf 'Сборка браузерной игры готова: %s\n' "$public_directory"
