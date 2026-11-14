#!/usr/bin/env bash
set -euo pipefail

global_release="${GLOBAL_RELEASE:-0}"
month_year="$(TZ=Europe/Moscow date +%m%y)"

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

[[ "$global_release" =~ ^[0-9]+$ ]] || fail "GLOBAL_RELEASE должен быть целым неотрицательным числом."
[[ "$month_year" =~ ^(0[1-9]|1[0-2])[0-9]{2}$ ]] || fail "RELEASE_MONTH_YEAR должен иметь формат mmyy."

if [[ -v PREVIOUS_RELEASE_TAG ]]; then
  previous_tag="$PREVIOUS_RELEASE_TAG"
else
  previous_tag="$(gh release list --limit 1 --json tagName --jq '.[0].tagName // ""')"
fi

version_counter=0
commit_count=1

if [[ -n "$previous_tag" ]]; then
  if [[ "$previous_tag" =~ ^([0-9]+)\.([0-9]{4})\.([0-9]+)\.([0-9]+)$ ]]; then
    previous_global="${BASH_REMATCH[1]}"
    previous_month_year="${BASH_REMATCH[2]}"
    previous_version="${BASH_REMATCH[3]}"
  else
    fail "Последний опубликованный релиз имеет неизвестный номер: $previous_tag"
  fi

  previous_commit="$(git rev-parse --verify "${previous_tag}^{commit}")" \
    || fail "Не найдена версия коммита последнего релиза: $previous_tag"
  git merge-base --is-ancestor "$previous_commit" HEAD \
    || fail "Коммит последнего релиза не является предком текущей main."

  previous_parent="$(git rev-parse --verify "${previous_commit}^" 2>/dev/null || true)"
  if [[ -n "$previous_parent" ]]; then
    commit_count="$(git rev-list --count "${previous_parent}..HEAD")"
  else
    commit_count="$(git rev-list --count HEAD)"
  fi
  (( commit_count > 0 )) || commit_count=1

  if [[ "$previous_global" == "$global_release" && "$previous_month_year" == "$month_year" ]]; then
    version_counter=$((previous_version + 1))
  fi
fi

printf '%s.%s.%s.%s\n' "$global_release" "$month_year" "$version_counter" "$commit_count"
