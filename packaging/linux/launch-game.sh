#!/usr/bin/env bash
set -euo pipefail

server="/usr/libexec/north-seas-adventure/web-preview"
public_directory="/usr/share/north-seas-adventure/www"
port="${NORTH_SEAS_PORT:-23162}"
url="http://127.0.0.1:${port}/"

BIND_ADDRESS=127.0.0.1 PORT="$port" "$server" "$public_directory" &
server_pid=$!

stop_server() {
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
}
trap stop_server EXIT INT TERM

ready=0
for _ in {1..50}; do
  if ! kill -0 "$server_pid" 2>/dev/null; then
    wait "$server_pid"
    exit 1
  fi
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    ready=1
    break
  fi
  sleep 0.1
done

if [[ "$ready" -ne 1 ]]; then
  printf 'Не удалось запустить игровой сервер на порту %s.\n' "$port" >&2
  exit 1
fi

printf 'Игра открывается в браузере: %s\n' "$url"
if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$url" || printf 'Откройте адрес вручную: %s\n' "$url" >&2
else
  printf 'Откройте адрес вручную: %s\n' "$url"
fi

wait "$server_pid"
