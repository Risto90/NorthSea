#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 3 ]]; then
  printf 'Использование: %s <пакет.deb> <пакет.rpm> <иконка.png>\n' "$0" >&2
  exit 2
fi

deb_package="$(realpath "$1")"
rpm_package="$(realpath "$2")"
expected_icon="$(realpath "$3")"
for tool in cpio dpkg-deb rpm2cpio; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Для проверки Linux-пакетов требуется команда %s.\n' "$tool" >&2
    exit 1
  fi
done
for tool in timeout xvfb-run; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Для запуска нативной игры при проверке пакетов требуется команда %s.\n' "$tool" >&2
    exit 1
  fi
done

temporary_directory="$(mktemp -d)"
cleanup() {
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
  local binary="$package_root/usr/libexec/north-sea/north-sea"
  local launcher="$package_root/usr/bin/north-sea"
  local package_icon="$package_root/usr/share/icons/hicolor/128x128/apps/north-sea.png"
  local log_file="$temporary_directory/${package_type}-native.log"
  local timeout_seconds="${NORTH_SEAS_SMOKE_TIMEOUT:-8}"
  local status=0

  if [[ ! -x "$binary" || ! -x "$launcher" ]]; then
    printf 'Пакет %s не содержит нативное приложение и его запускатель.\n' "$package_type" >&2
    return 1
  fi
  if [[ ! -f "$package_icon" ]] || ! cmp -s "$expected_icon" "$package_icon"; then
    printf 'Пакет %s не содержит исходную иконку North Sea.\n' "$package_type" >&2
    return 1
  fi
  if [[ "$(od -An -N4 -tx1 "$binary" | tr -d ' \n')" != "7f454c46" ]]; then
    printf 'Файл игры в пакете %s не является исполняемым ELF-приложением.\n' "$package_type" >&2
    return 1
  fi
  if [[ -e "$package_root/usr/libexec/north-sea/web-preview" \
     || -e "$package_root/usr/share/north-sea/www" ]]; then
    printf 'Пакет %s ошибочно содержит браузерный сервер или веб-файлы.\n' "$package_type" >&2
    return 1
  fi

  set +e
  NORTH_SEAS_PACKAGE_ROOT="$package_root" xvfb-run -a timeout \
    --signal=TERM --kill-after=2s "$timeout_seconds" \
    "$launcher" >"$log_file" 2>&1
  status=$?
  set -e
  if [[ "$status" -ne 124 ]]; then
    printf 'Нативная игра из пакета %s не осталась запущенной (код %s).\n' "$package_type" "$status" >&2
    cat "$log_file" >&2
    return 1
  fi
  if ! grep -Fq 'Графический адаптер:' "$log_file"; then
    printf 'Игра из пакета %s не создала графический адаптер.\n' "$package_type" >&2
    cat "$log_file" >&2
    return 1
  fi

  printf 'Пакет %s запустил нативное приложение и создал графический адаптер в виртуальном дисплее.\n' "$package_type"
}

smoke_test_package "$deb_root" "DEB"
smoke_test_package "$rpm_root" "RPM"
