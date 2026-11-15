#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
release_tag="${1:?Нужно передать номер релиза.}"

previous_tag="$(gh release list --limit 1 --json tagName --jq '.[0].tagName // ""')"
previous_code=16777216

if [[ -n "$previous_tag" ]]; then
  previous_body="$(gh release view "$previous_tag" --json body --jq '.body')"
  previous_code="$(sed -n 's/^Android versionCode: //p' <<< "$previous_body" | tail -n 1)"
  if [[ -z "$previous_code" ]]; then
    if [[ "$previous_tag" == "0.1126.2.2" ]]; then
      previous_code=16777472
    else
      printf 'В описании последнего релиза %s не найден Android versionCode.\n' "$previous_tag" >&2
      exit 1
    fi
  fi
fi
if [[ -z "$previous_tag" ]]; then
  previous_tag=none
fi

if [[ ! "$previous_code" =~ ^[0-9]+$ ]]; then
  printf 'Некорректный Android versionCode предыдущего релиза: %s\n' "$previous_code" >&2
  exit 1
fi

version_tool="$(mktemp)"
trap 'rm -f -- "$version_tool"' EXIT
"$script_dir/with-rust-toolchain.sh" rustc \
  --edition=2024 "$script_dir/android-release-version.rs" -o "$version_tool"

next_version="$("$version_tool" "$release_tag" "$previous_code")"
baseline_version="$("$version_tool" for-code "$release_tag" "$previous_code")"
read -r next_code next_package_version <<< "$next_version"
read -r baseline_code baseline_package_version <<< "$baseline_version"
printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
  "$previous_tag" "$previous_code" "$next_code" "$next_package_version" \
  "$baseline_code" "$baseline_package_version"
