#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(dirname "$script_dir")"
target_directory="${CARGO_TARGET_DIR:-$project_root/target}"
if [[ "$target_directory" != /* ]]; then
  target_directory="$project_root/$target_directory"
fi
export CARGO_TARGET_DIR="$target_directory"

manifest_backup_directory=""
restore_manifest() {
  if [[ -n "$manifest_backup_directory" ]]; then
    cp -- "$manifest_backup_directory/Cargo.toml" "$project_root/Cargo.toml"
    cp -- "$manifest_backup_directory/Cargo.lock" "$project_root/Cargo.lock"
    rm -rf -- "$manifest_backup_directory"
  fi
}
trap restore_manifest EXIT

if [[ -n "${ANDROID_PACKAGE_VERSION:-}" ]]; then
  if [[ ! "$ANDROID_PACKAGE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+\+[0-9]+\.[0-9]{4}\.[0-9]+\.[1-9][0-9]*$ ]]; then
    printf 'ANDROID_PACKAGE_VERSION должен быть версией SemVer с номером релиза в метаданных.\n' >&2
    exit 1
  fi
  manifest_backup_directory="$(mktemp -d)"
  cp -- "$project_root/Cargo.toml" "$manifest_backup_directory/Cargo.toml"
  cp -- "$project_root/Cargo.lock" "$manifest_backup_directory/Cargo.lock"
  sed -i "0,/^version = /{s|^version = .*|version = \"$ANDROID_PACKAGE_VERSION\"|}" \
    "$project_root/Cargo.toml"
fi

export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$project_root/.local/toolchains/android-sdk}}"
unset ANDROID_SDK_ROOT
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$project_root/.local/toolchains/android-ndk-r30}"
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$ANDROID_NDK_HOME}"

if [[ -z "${JAVA_HOME:-}" ]]; then
  java_binary="$(command -v java || true)"
  if [[ -n "$java_binary" ]]; then
    export JAVA_HOME="$(dirname -- "$(dirname -- "$(readlink -f -- "$java_binary")")")"
  fi
fi
if [[ -n "${JAVA_HOME:-}" ]]; then
  export PATH="$JAVA_HOME/bin:$PATH"
fi

if [[ -d "$ANDROID_HOME/cmdline-tools/latest/bin" ]]; then
  export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
fi
if [[ -d "$ANDROID_HOME/platform-tools" ]]; then
  export PATH="$ANDROID_HOME/platform-tools:$PATH"
fi

if ! command -v java >/dev/null 2>&1; then
  printf 'Для сборки APK требуется Java, но команда java не найдена.\n' >&2
  exit 1
fi

cd "$project_root"
"$project_root/scripts/with-rust-toolchain.sh" cargo apk build \
  --lib \
  --target aarch64-linux-android

apk_source="$target_directory/debug/apk/north-sea.apk"
apk_output="$project_root/generated-artifacts/north-sea.apk"
mkdir -p "$(dirname -- "$apk_output")"
cp "$apk_source" "$apk_output"
printf 'APK для тестирования: %s\n' "$apk_output"
