#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${REPO_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
cd "$ROOT_DIR"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

STAGED_MODE=0
if [[ "${1:-}" == "--staged" ]]; then
  STAGED_MODE=1
  shift
fi

context_map_path="${ROOT_DIR}/docs/harness/context-map.yaml"
if [[ -f "$context_map_path" ]]; then
  canonical_docs_str="$(awk '
    /owned_by:/ { in_owned=1; next }
    in_owned && /^[[:space:]]*- / {
      val=$0
      gsub(/^[[:space:]]*- /, "", val)
      gsub(/^"/, "", val); gsub(/"$/, "", val)
      gsub(/^\047/, "", val); gsub(/\047$/, "", val)
      print val
      next
    }
    in_owned { in_owned=0 }
  ' "$context_map_path" | sort -u)"
else
  canonical_docs_str=""
fi

BASE_REF="${1:-}"
if [[ -z "$BASE_REF" ]]; then
  if [[ "$STAGED_MODE" -eq 1 ]]; then
    changed_files="$(git diff --cached --name-only)"
    if [[ -z "${changed_files:-}" ]]; then
      echo "No staged files; doc relevance check passed."
      exit 0
    fi
  fi

  if [[ "$STAGED_MODE" -eq 0 ]]; then
    BASE_REF="$(harness_resolve_base_ref || true)"
  fi
fi

if [[ "$STAGED_MODE" -eq 0 && -z "$BASE_REF" ]]; then
  echo "Could not determine base ref; skipping doc relevance check."
  exit 0
fi

if [[ "$STAGED_MODE" -eq 0 ]]; then
  changed_files="$(harness_collect_changed_files "$STAGED_MODE" "$BASE_REF")"
fi

if [[ -z "${changed_files:-}" ]]; then
  echo "No changed files; doc relevance check passed."
  exit 0
fi

evidence_path=""
match_rc=0
match_output="$(harness_find_matching_evidence_path "$changed_files" 2>&1)" || match_rc=$?

if [[ "$match_rc" -eq 2 ]]; then
  printf '%s\n' "$match_output" >&2
  exit 1
elif [[ "$match_rc" -eq 0 ]]; then
  evidence_path="$match_output"
  harness_assert_evidence_matches_changed_files "$evidence_path" "$changed_files"
fi

allowed_docs=""
evidence_declared=0
if [[ -n "${evidence_path:-}" && -f "${evidence_path}" ]]; then
  evidence_declared=1
  allowed_docs="$(node - "${evidence_path}" <<'EOF'
const fs = require('node:fs')
const file = process.argv[2]
const parsed = JSON.parse(fs.readFileSync(file, 'utf8'))
const combined = [
  ...(Array.isArray(parsed.canonicalDocsUpdated) ? parsed.canonicalDocsUpdated : []),
  ...((Array.isArray(parsed.waivers) ? parsed.waivers : []).flatMap((entry) =>
    entry && typeof entry.doc === 'string' ? [entry.doc] : [],
  )),
]
const unique = [...new Set(combined)].sort((a, b) => a.localeCompare(b))
process.stdout.write(`${unique.join('\n')}\n`)
EOF
)"
fi

contains_line() {
  local needle="$1"
  while IFS= read -r item; do
    [[ -z "$item" ]] && continue
    if [[ "$item" == "$needle" ]]; then
      return 0
    fi
  done <<< "$2"

  return 1
}

is_metadata_only_diff() {
  local target="$1"
  local before_content
  local after_content

  if [[ "$STAGED_MODE" -eq 1 ]]; then
    before_content="$(git show "HEAD:${target}" 2>/dev/null || true)"
    after_content="$(git show ":${target}" 2>/dev/null || true)"
  else
    before_content="$(git show "${BASE_REF}:${target}" 2>/dev/null || true)"
    after_content="$(git show "HEAD:${target}" 2>/dev/null || cat "$target")"
  fi

  local normalized_before
  local normalized_after
  normalized_before="$(printf '%s' "$before_content" | sed -E '/^Last reviewed: /d; s/[[:space:]]+$//')"
  normalized_after="$(printf '%s' "$after_content" | sed -E '/^Last reviewed: /d; s/[[:space:]]+$//')"

  [[ "$normalized_before" == "$normalized_after" ]]
}

is_canonical_doc() {
  local target="$1"
  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    if [[ "$entry" == */ ]]; then
      # Directory prefix match
      if [[ "$target" == "$entry"* ]]; then
        return 0
      fi
    else
      # Exact match
      if [[ "$target" == "$entry" ]]; then
        return 0
      fi
    fi
  done <<< "$canonical_docs_str"
  return 1
}

failed=0
while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  if ! is_canonical_doc "$file"; then
    continue
  fi

  if is_metadata_only_diff "$file"; then
    echo "Canonical doc change is metadata-only: $file"
    failed=1
    continue
  fi

  if [[ "$evidence_declared" -eq 1 ]] && ! contains_line "$file" "$allowed_docs"; then
    echo "Canonical doc changed without declared update or waiver linkage: $file"
    failed=1
  fi
done <<< "$changed_files"

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

echo "Doc relevance check passed."
