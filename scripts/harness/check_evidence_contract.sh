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

BASE_REF="${1:-}"
if [[ -z "$BASE_REF" ]]; then
  if [[ "$STAGED_MODE" -eq 1 ]]; then
    changed_files="$(git diff --cached --name-only)"
    if [[ -z "${changed_files:-}" ]]; then
      echo "No staged files; evidence contract check passed."
      exit 0
    fi
  fi

  if [[ "$STAGED_MODE" -eq 0 ]]; then
    BASE_REF="$(harness_resolve_base_ref || true)"
  fi
fi

if [[ "$STAGED_MODE" -eq 0 && -z "$BASE_REF" ]]; then
  echo "Could not determine base ref; skipping evidence contract check."
  exit 0
fi

if [[ "$STAGED_MODE" -eq 0 ]]; then
  changed_files="$(harness_collect_changed_files "$STAGED_MODE" "$BASE_REF")"
fi

if [[ -z "${changed_files:-}" ]]; then
  echo "No changed files; evidence contract check passed."
  exit 0
fi

is_non_doc_change() {
  case "$1" in
    docs/*|AGENTS.md|ARCHITECTURE.md|PLANS.md|QUALITY_SCORE.md|RELIABILITY.md|SECURITY.md|README.md)
      return 1
      ;;
    *)
      return 0
      ;;
  esac
}

is_architecture_sensitive() {
  case "$1" in
    src/config/*|src/execution/*|src/orchestrator/*|scripts/harness/*|SPEC.md|WORKFLOW.md|ARCHITECTURE.md|SECURITY.md|RELIABILITY.md)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

non_doc_changes=0
architecture_sensitive=0
while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  if is_non_doc_change "$file"; then
    non_doc_changes=$((non_doc_changes + 1))
  fi
  if is_architecture_sensitive "$file"; then
    architecture_sensitive=1
  fi
done <<< "$changed_files"

if [[ "$architecture_sensitive" -eq 0 && "$non_doc_changes" -lt 2 ]]; then
  echo "Evidence contract not required for this change set."
  exit 0
fi

evidence_path=""
if ! evidence_path="$(harness_find_matching_evidence_path "$changed_files" 2>/dev/null)"; then
  echo "Qualifying changes require a change-evidence JSON artifact that matches the current diff."
  exit 1
fi

harness_assert_evidence_matches_changed_files "$evidence_path" "$changed_files"

if [[ -z "${evidence_path:-}" || ! -f "${evidence_path}" ]]; then
  echo "Qualifying changes require a change-evidence JSON artifact that matches the current diff."
  exit 1
fi

validation_output="$(node - "${evidence_path}" "${architecture_sensitive}" <<'EOF'
const fs = require('node:fs')

const file = process.argv[2]
const architectureSensitive = process.argv[3] === '1'
const parsed = JSON.parse(fs.readFileSync(file, 'utf8'))
const errors = []

const requiredString = (name) => {
  if (typeof parsed[name] !== 'string' || parsed[name].trim().length === 0) {
    errors.push(`Missing non-empty string field: ${name}`)
  }
}

const requiredArray = (name) => {
  if (!Array.isArray(parsed[name]) || parsed[name].length === 0) {
    errors.push(`Missing non-empty array field: ${name}`)
    return []
  }
  return parsed[name]
}

requiredString('topic')
requiredString('summary')
requiredArray('changedFiles')
requiredArray('contextLoaded')
const decisionArtifacts = Array.isArray(parsed.decisionArtifacts) ? parsed.decisionArtifacts : []
if (architectureSensitive && decisionArtifacts.length === 0) {
  errors.push('Architecture-sensitive changes require at least one decision artifact.')
}
if (!Array.isArray(parsed.canonicalDocsUpdated)) {
  errors.push('canonicalDocsUpdated must be an array.')
}
const waivers = Array.isArray(parsed.waivers) ? parsed.waivers : null
if (waivers === null) {
  errors.push('waivers must be an array.')
}
const verification = requiredArray('verification')
const verificationArtifacts = Array.isArray(parsed.verificationArtifacts) ? parsed.verificationArtifacts : null
if (verificationArtifacts === null) {
  errors.push('verificationArtifacts must be an array.')
}
requiredArray('impactedAreas')

if (Array.isArray(parsed.canonicalDocsUpdated) && parsed.canonicalDocsUpdated.length === 0) {
  if (!Array.isArray(parsed.waivers) || parsed.waivers.length === 0) {
    errors.push('No canonical docs updated; add an explicit waiver with rationale.')
  }
}

if (Array.isArray(parsed.waivers)) {
  for (const waiver of parsed.waivers) {
    if (!waiver || typeof waiver.doc !== 'string' || waiver.doc.trim().length === 0) {
      errors.push('Waivers must include a non-empty doc field.')
    }
    if (!waiver || typeof waiver.reason !== 'string' || waiver.reason.trim().length === 0) {
      errors.push('Waivers must include a non-empty reason field.')
    }
  }
}

for (const entry of verification) {
  if (!entry || typeof entry.command !== 'string' || entry.command.trim().length === 0 || entry.command === 'fill-in-command') {
    errors.push('Verification entries must include concrete command evidence.')
  }
  if (!entry || typeof entry.result !== 'string' || entry.result.trim().length === 0) {
    errors.push('Verification entries must include a result.')
  }
}

if (errors.length > 0) {
  process.stdout.write(errors.join('\n'))
  process.exit(1)
}
EOF
)" || {
  printf '%s\n' "$validation_output"
  exit 1
}

echo "Evidence contract check passed."
