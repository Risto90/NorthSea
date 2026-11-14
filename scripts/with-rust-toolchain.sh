#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(dirname "$script_dir")"
export CARGO_HOME="${CARGO_HOME:-$project_root/.local/share/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$project_root/.local/share/.rustup}"
export RUSTUP_TOOLCHAIN=stable
unset RUSTC CARGO
unset LD_AUDIT

exec "$@"
