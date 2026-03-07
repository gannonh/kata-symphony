#!/usr/bin/env bash
set -euo pipefail

harness_resolve_base_ref() {
  if [[ -n "${HARNESS_BASE_REF:-}" ]]; then
    if git rev-parse --verify "${HARNESS_BASE_REF}^{commit}" >/dev/null 2>&1; then
      printf '%s\n' "${HARNESS_BASE_REF}"
      return 0
    fi
  fi

  local candidate
  local merge_base
  local candidates=(
    "origin/main"
    "main"
    "origin/master"
    "master"
  )

  for candidate in "${candidates[@]}"; do
    if git rev-parse --verify "$candidate" >/dev/null 2>&1; then
      merge_base="$(git merge-base HEAD "$candidate" 2>/dev/null || true)"
      if [[ -n "$merge_base" ]]; then
        printf '%s\n' "$merge_base"
        return 0
      fi
    fi
  done

  local upstream
  upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
  if [[ -n "$upstream" ]]; then
    merge_base="$(git merge-base HEAD "$upstream" 2>/dev/null || true)"
    if [[ -n "$merge_base" ]]; then
      printf '%s\n' "$merge_base"
      return 0
    fi
  fi

  if git rev-parse --verify HEAD~1 >/dev/null 2>&1; then
    git rev-parse --verify HEAD~1
    return 0
  fi

  return 1
}

harness_collect_changed_files() {
  local staged_mode="$1"
  local base_ref="${2:-}"

  if [[ "$staged_mode" -eq 1 ]]; then
    git diff --cached --name-only | sed '/^$/d' | sort -u
    return 0
  fi

  if [[ -z "$base_ref" ]]; then
    return 0
  fi

  git diff --name-only "${base_ref}"...HEAD | sed '/^$/d' | sort -u
}

harness_find_matching_evidence_path() {
  local changed_files="$1"

  if [[ -n "${HARNESS_EVIDENCE_PATH:-}" ]]; then
    printf '%s\n' "${HARNESS_EVIDENCE_PATH}"
    return 0
  fi

  if [[ ! -d docs/generated/change-evidence ]]; then
    return 1
  fi

  local candidate
  local matches=()
  while IFS= read -r candidate; do
    [[ -z "$candidate" ]] && continue
    if HARNESS_EXPECTED_CHANGED_FILES="$changed_files" node - "$candidate" <<'EOF' >/dev/null 2>&1
const fs = require('node:fs')

const candidatePath = process.argv[2]
const expected = (process.env.HARNESS_EXPECTED_CHANGED_FILES || '')
  .split('\n')
  .map((item) => item.trim())
  .filter((item) => item.length > 0)
  .sort()
const parsed = JSON.parse(fs.readFileSync(candidatePath, 'utf8'))
const actual = Array.isArray(parsed.changedFiles)
  ? [...new Set(parsed.changedFiles.filter((item) => typeof item === 'string' && item.trim().length > 0))].sort()
  : []

if (expected.length !== actual.length) {
  process.exit(1)
}

for (let index = 0; index < expected.length; index += 1) {
  if (expected[index] !== actual[index]) {
    process.exit(1)
  }
}
EOF
    then
      matches+=("$candidate")
    fi
  done < <(find docs/generated/change-evidence -maxdepth 1 -type f -name '*.json' | sort)

  if [[ "${#matches[@]}" -eq 1 ]]; then
    printf '%s\n' "${matches[0]}"
    return 0
  fi

  if [[ "${#matches[@]}" -gt 1 ]]; then
    echo "Multiple evidence artifacts match the current change set. Set HARNESS_EVIDENCE_PATH explicitly." >&2
    return 2
  fi

  return 1
}

harness_assert_evidence_matches_changed_files() {
  local evidence_path="$1"
  local changed_files="$2"

  HARNESS_EXPECTED_CHANGED_FILES="$changed_files" node - "$evidence_path" <<'EOF'
const fs = require('node:fs')

const evidencePath = process.argv[2]
const expected = (process.env.HARNESS_EXPECTED_CHANGED_FILES || '')
  .split('\n')
  .map((item) => item.trim())
  .filter((item) => item.length > 0)
  .sort()
const parsed = JSON.parse(fs.readFileSync(evidencePath, 'utf8'))
const actual = Array.isArray(parsed.changedFiles)
  ? [...new Set(parsed.changedFiles.filter((item) => typeof item === 'string' && item.trim().length > 0))].sort()
  : []

if (expected.length !== actual.length || expected.some((item, index) => item !== actual[index])) {
  process.stderr.write('Evidence artifact changedFiles do not match the current diff.\n')
  process.exit(1)
}
EOF
}
