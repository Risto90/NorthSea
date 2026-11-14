#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 2 ]]; then
  printf 'Использование: %s <пакет.deb> <пакет.rpm>\n' "$0" >&2
  exit 2
fi

deb_package="$(realpath "$1")"
rpm_package="$(realpath "$2")"
for tool in cpio curl dpkg-deb rpm2cpio; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Для проверки Linux-пакетов требуется команда %s.\n' "$tool" >&2
    exit 1
  fi
done

temporary_directory="$(mktemp -d)"
server_pid=""

cleanup() {
  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf -- "$temporary_directory"
}
trap cleanup EXIT INT TERM

deb_root="$temporary_directory/deb"
rpm_root="$temporary_directory/rpm"
mkdir -p "$deb_root" "$rpm_root"
dpkg-deb --extract "$deb_package" "$deb_root"
rpm2cpio "$rpm_package" | (cd "$rpm_root" && cpio --extract --make-directories --quiet)

smoke_test_package() {
  local package_root="$1"
  local package_type="$2"
  local server="$package_root/usr/libexec/north-seas-adventure/web-preview"
  local public_directory="$package_root/usr/share/north-seas-adventure/www"
  local port="$((20000 + RANDOM % 40000))"
  local base_url="http://127.0.0.1:${port}"
  local index_file="$temporary_directory/${package_type}-index.html"
  local wasm_file="$temporary_directory/${package_type}-game.wasm"
  local ready=0

  if [[ ! -x "$server" || ! -d "$public_directory" ]]; then
    printf 'Пакет %s не содержит сервер или файлы игры.\n' "$package_type" >&2
    return 1
  fi

  BIND_ADDRESS=127.0.0.1 PORT="$port" "$server" "$public_directory" \
    >"$temporary_directory/${package_type}-server.log" 2>&1 &
  server_pid=$!

  for _ in {1..50}; do
    if ! kill -0 "$server_pid" 2>/dev/null; then
      cat "$temporary_directory/${package_type}-server.log" >&2
      return 1
    fi
    if curl --fail --silent "$base_url/" -o "$index_file"; then
      ready=1
      break
    fi
    sleep 0.1
  done

  if [[ "$ready" -ne 1 ]]; then
    printf 'Сервер пакета %s не начал отвечать.\n' "$package_type" >&2
    cat "$temporary_directory/${package_type}-server.log" >&2
    return 1
  fi

  grep -Fq '<title>Моря Туманного Севера</title>' "$index_file"
  grep -Fq './src/wasm-bindgen/north_seas_adventure.js' "$index_file"
  curl --fail --silent --show-error "$base_url/game.css" -o "$temporary_directory/${package_type}-game.css"
  curl --fail --silent --show-error "$base_url/compass.svg" -o "$temporary_directory/${package_type}-compass.svg"
  curl --fail --silent --show-error \
    "$base_url/src/wasm-bindgen/north_seas_adventure.js" \
    -o "$temporary_directory/${package_type}-game.js"
  curl --fail --silent --show-error \
    "$base_url/src/wasm-bindgen/north_seas_adventure_bg.wasm" \
    -o "$wasm_file"
  test -s "$temporary_directory/${package_type}-game.css"
  test -s "$temporary_directory/${package_type}-compass.svg"
  test -s "$temporary_directory/${package_type}-game.js"
  test "$(od -An -N4 -tx1 "$wasm_file" | tr -d ' \n')" = "0061736d"
  curl --fail --silent --show-error --head \
    "$base_url/src/wasm-bindgen/north_seas_adventure_bg.wasm" \
    | tr -d '\r' | grep -qi '^Content-Type: application/wasm$'

  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  server_pid=""
  printf 'Пакет %s запустил сервер и отдал HTML, CSS, SVG и WebAssembly.\n' "$package_type"
}

smoke_test_package "$deb_root" "DEB"
smoke_test_package "$rpm_root" "RPM"
