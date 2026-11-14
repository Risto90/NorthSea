#!/usr/bin/env bash
set -euo pipefail

install_root="${NORTH_SEAS_PACKAGE_ROOT:-}"
if [[ -n "$install_root" ]]; then
  binary="$install_root/usr/libexec/north-sea/north-sea"
else
  binary="/usr/libexec/north-sea/north-sea"
fi

if [[ ! -x "$binary" ]]; then
  printf 'Не найден исполняемый файл игры: %s\n' "$binary" >&2
  exit 1
fi

exec "$binary" "$@"
