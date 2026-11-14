#!/usr/bin/env bash
set -euo pipefail

# Явные директивы в описании коммита:
# Issue-Task-ID: webgpu-all-platforms
# Issue-Status: open|closed
# Issue-Comment: Однострочный комментарий к задаче.
repo="${GITHUB_REPOSITORY:?Не задан репозиторий GitHub}"
before="${TASKS_BEFORE:?Не задан предыдущий коммит}"
after="${TASKS_AFTER:?Не задан текущий коммит}"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
declare -A seen_blobs=()

fail() {
  printf 'Ошибка синхронизации задач: %s\n' "$*" >&2
  exit 1
}

find_issue_number() {
  local task_id="$1"
  local marker="<!-- task-id:${task_id} -->"
  local issues matches count

  issues="$(gh issue list --repo "$repo" --state all --limit 1000 --json number,body)"
  matches="$(jq --arg marker "$marker" '[.[] | select((.body // "") | contains($marker))]' <<< "$issues")"
  count="$(jq 'length' <<< "$matches")"
  [[ "$count" -le 1 ]] || fail "найдено несколько Issues для идентификатора '$task_id'"
  jq -r '.[0].number // empty' <<< "$matches"
}

read_task_file() {
  local commit="$1"
  local path="$2"
  local content id title dependency body_file issue_number state url

  content="$(git show "${commit}:${path}")"
  id="$(sed -n 's/^id:[[:space:]]*//p' <<< "$content" | head -n 1 | tr -d '\r')"
  title="$(sed -n 's/^title:[[:space:]]*//p' <<< "$content" | head -n 1 | tr -d '\r')"
  dependency="$(sed -n 's/^depends_on:[[:space:]]*//p' <<< "$content" | head -n 1 | tr -d '\r')"
  [[ "$id" =~ ^[a-z0-9][a-z0-9-]*$ ]] || fail "некорректный id в ${path}"
  [[ -n "$title" ]] || fail "не задан title в ${path}"
  awk '/^---[[:space:]]*$/ { count++; next } END { exit !(count >= 2) }' <<< "$content" ||
    fail "в ${path} нет корректной пары разделителей YAML-заголовка"

  body_file="$work_dir/task-body.md"
  awk '
    /^---[[:space:]]*$/ { count++; next }
    count >= 2 { print }
  ' <<< "$content" > "$body_file"
  printf '\n\n<!-- task-id:%s -->\n' "$id" >> "$body_file"

  if [[ -n "$dependency" ]]; then
    [[ "$dependency" =~ ^[a-z0-9][a-z0-9-]*$ ]] || fail "некорректный depends_on в ${path}"
    issue_number="$(find_issue_number "$dependency")"
    [[ -n "$issue_number" ]] || fail "сначала должна существовать зависимая задача '$dependency'"
    printf '\nЗависит от: #%s\n' "$issue_number" >> "$body_file"
  fi

  issue_number="$(find_issue_number "$id")"
  if [[ -z "$issue_number" ]]; then
    url="$(gh issue create --repo "$repo" --title "$title" --body-file "$body_file")"
    issue_number="${url##*/}"
    [[ "$issue_number" =~ ^[0-9]+$ ]] || fail "GitHub вернул неожиданный адрес Issue: ${url}"
    printf 'Создан Issue #%s: %s\n' "$issue_number" "$title"
    return
  fi

  gh issue edit "$issue_number" --repo "$repo" --title "$title" --body-file "$body_file"
  state="$(gh issue view "$issue_number" --repo "$repo" --json state --jq '.state')"
  if [[ "$state" == "CLOSED" ]]; then
    gh issue reopen "$issue_number" --repo "$repo"
    printf 'Обновлён и снова открыт Issue #%s: %s\n' "$issue_number" "$title"
  else
    printf 'Обновлён Issue #%s: %s\n' "$issue_number" "$title"
  fi
}

apply_commit_directives() {
  local commit="$1"
  local message task_id status comment issue_number current_state
  local status_count comment_count

  message="$(git show -s --format=%B "$commit")"
  task_id="$(sed -n 's/^Issue-Task-ID:[[:space:]]*//p' <<< "$message" | tail -n 1 | tr -d '\r')"
  status="$(sed -n 's/^Issue-Status:[[:space:]]*//p' <<< "$message" | tail -n 1 | tr -d '\r')"
  comment="$(sed -n 's/^Issue-Comment:[[:space:]]*//p' <<< "$message" | tail -n 1 | tr -d '\r')"
  status_count="$(grep -c '^Issue-Status:' <<< "$message" || true)"
  comment_count="$(grep -c '^Issue-Comment:' <<< "$message" || true)"

  [[ "$status_count" -le 1 ]] || fail "в коммите ${commit} указано несколько Issue-Status"
  [[ "$comment_count" -le 1 ]] || fail "в коммите ${commit} указано несколько Issue-Comment"
  if [[ -z "$task_id" && ( -n "$status" || -n "$comment" || "$status_count" -gt 0 || "$comment_count" -gt 0 ) ]]; then
    fail "директивам статуса или комментария нужен Issue-Task-ID в коммите ${commit}"
  fi
  [[ -n "$task_id" ]] || return 0
  [[ "$task_id" =~ ^[a-z0-9][a-z0-9-]*$ ]] || fail "некорректный Issue-Task-ID в коммите ${commit}"
  if [[ "$status_count" -gt 0 ]]; then
    [[ "$status" == "open" || "$status" == "closed" ]] ||
      fail "Issue-Status должен быть open или closed в коммите ${commit}"
  fi
  if [[ "$comment_count" -gt 0 ]]; then
    [[ -n "$comment" ]] || fail "Issue-Comment не должен быть пустым в коммите ${commit}"
  fi

  issue_number="$(find_issue_number "$task_id")"
  [[ -n "$issue_number" ]] || fail "Issue для идентификатора '$task_id' не найден"
  if [[ "$status_count" -gt 0 ]]; then
    current_state="$(gh issue view "$issue_number" --repo "$repo" --json state --jq '.state')"
    if [[ "$status" == "closed" && "$current_state" != "CLOSED" ]]; then
      gh issue close "$issue_number" --repo "$repo"
      printf 'Закрыт Issue #%s по директиве коммита\n' "$issue_number"
    elif [[ "$status" == "open" && "$current_state" != "OPEN" ]]; then
      gh issue reopen "$issue_number" --repo "$repo"
      printf 'Открыт Issue #%s по директиве коммита\n' "$issue_number"
    fi
  fi
  if [[ "$comment_count" -gt 0 ]]; then
    gh issue comment "$issue_number" --repo "$repo" --body "$comment"
    printf 'Добавлен комментарий к Issue #%s\n' "$issue_number"
  fi
}

process_changed_files() {
  local commit="$1"
  local diff_file="$work_dir/changed-files.txt"
  local change path blob

  if git rev-parse "${commit}^1" >/dev/null 2>&1; then
    git diff --name-status --no-renames "${commit}^1" "$commit" -- .tasks > "$diff_file"
  else
    git diff-tree --root --no-commit-id --name-status -r --no-renames "$commit" -- .tasks > "$diff_file"
  fi

  while IFS=$'\t' read -r change path; do
    [[ "$path" == .tasks/*.md ]] || continue
    if [[ "$change" == "D" ]]; then
      unset 'seen_blobs[$path]'
      continue
    fi
    [[ "$change" == "A" || "$change" == "M" || "$change" == "T" ]] || continue
    blob="$(git rev-parse "${commit}:${path}")"
    [[ "${seen_blobs[$path]-}" == "$blob" ]] && continue
    seen_blobs["$path"]="$blob"
    read_task_file "$commit" "$path"
  done < <(sort -k2,2 "$diff_file")
}

if [[ "$before" =~ ^0+$ ]]; then
  while IFS= read -r path; do
    [[ "$path" == .tasks/*.md ]] || continue
    seen_blobs["$path"]="$(git rev-parse "${after}:${path}")"
    read_task_file "$after" "$path"
  done < <(git ls-tree -r --name-only "$after" -- .tasks | sort)
  apply_commit_directives "$after"
else
  while IFS= read -r commit; do
    process_changed_files "$commit"
    apply_commit_directives "$commit"
  done < <(git rev-list --reverse --topo-order "${before}..${after}")
fi
