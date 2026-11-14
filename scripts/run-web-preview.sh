#!/usr/bin/env bash
set -euo pipefail

script_directory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(dirname "$script_directory")"
public_directory="$project_root/artifacts/north-seas-adventure/dist/public"

bash "$project_root/scripts/build-web.sh"
cd "$project_root"
"$project_root/scripts/with-rust-toolchain.sh" cargo run \
  --release \
  --bin web-preview \
  -- "$public_directory"
