#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(dirname "$script_dir")"
export CARGO_HOME="${CARGO_HOME:-$project_root/.local/share/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$project_root/.local/share/.rustup}"
export RUSTUP_TOOLCHAIN=stable
unset RUSTC CARGO
unset LD_AUDIT

has_target=false
for argument in "$@"; do
  if [[ "$argument" == "--target" || "$argument" == --target=* ]]; then
    has_target=true
    break
  fi
done

if [[ "$has_target" == false && -n "${NIX_LDFLAGS:-}" ]]; then
  nix_library_paths=()
  for flag in $NIX_LDFLAGS; do
    if [[ "$flag" == -L* ]]; then
      library_path="${flag#-L}"
      if [[ -d "$library_path" ]]; then
        nix_library_paths+=("$library_path")
      fi
    fi
  done

  if ((${#nix_library_paths[@]} > 0)); then
    joined_library_paths="$(IFS=:; printf '%s' "${nix_library_paths[*]}")"
    export LIBRARY_PATH="${LIBRARY_PATH:+$LIBRARY_PATH:}$joined_library_paths"
    export LD_LIBRARY_PATH="${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}$joined_library_paths"
  fi
fi

exec "$@"
