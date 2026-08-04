#!/usr/bin/env bash
# Create or repair the plan-build-verify-github label taxonomy in a GitHub repo.
#
# Idempotent: uses `gh label create --force`, which creates missing labels and
# updates color and description on existing ones. Never deletes labels and never
# touches labels outside the kind:, status:, phase:, and needs: namespaces.
#
# --dry-run queries the repo's current labels and reports, per label, whether it
# already exists or would be created. Writing labels is a side effect worth
# seeing before it happens.
#
# Usage:
#   ensure_labels.sh [--repo owner/name] [--dry-run]

set -euo pipefail

REPO=""
DRY_RUN=0

while [ $# -gt 0 ]; do
  case "$1" in
    --repo)
      REPO="${2:-}"
      [ -n "$REPO" ] || { echo "error: --repo requires a value" >&2; exit 2; }
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

command -v gh >/dev/null 2>&1 || { echo "error: gh CLI not found" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "error: gh is not authenticated. Run: gh auth login" >&2; exit 1; }

REPO_ARGS=()
if [ -n "$REPO" ]; then
  REPO_ARGS=(--repo "$REPO")
else
  REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)" || {
    echo "error: could not resolve the target repo. Pass --repo owner/name" >&2
    exit 1
  }
fi

# name|color|description
LABELS=(
  "kind:spec|0E8A16|Spec issue. The issue body is the spec."
  "kind:epic|5319E7|Spec decomposed into sub-issues."
  "kind:sub-spec|C2E0C6|Child spec produced by decomposing an epic."
  "status:draft|FBCA04|Spec is being written or revised. Do not build."
  "status:approved|0E8A16|Spec approved by the maintainer. Ready to build."
  "status:implemented|1D76DB|Built and reported. Ready to verify."
  "status:verified|0052CC|Acceptance evidence accepted."
  "status:blocked|B60205|Cannot proceed. See the issue body."
  "phase:plan|D4C5F9|Currently in the Plan phase."
  "phase:build|BFD4F2|Currently in the Build phase."
  "phase:verify|C5DEF5|Currently in the Verify phase."
  "needs:acceptance-criteria|E99695|No usable Acceptance criteria section."
  "needs:decomposition|E99695|Scope is too large for a single spec."
  "needs:triage|E99695|Not yet groomed into the roadmap model."
)

echo "Repo: $REPO"
[ "$DRY_RUN" -eq 1 ] && echo "Mode: dry run (no changes)"

EXISTING=""
EXISTING_KNOWN=0
if [ "$DRY_RUN" -eq 1 ]; then
  if EXISTING="$(gh label list --limit 500 --json name -q '.[].name' "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" 2>/dev/null)"; then
    EXISTING_KNOWN=1
  else
    echo "  warning: could not read existing labels from $REPO. Reporting all as unknown." >&2
  fi
fi

created=0
failed=0
would_create=0
already=0

for entry in "${LABELS[@]}"; do
  name="${entry%%|*}"
  rest="${entry#*|}"
  color="${rest%%|*}"
  desc="${rest#*|}"

  if [ "$DRY_RUN" -eq 1 ]; then
    if [ "$EXISTING_KNOWN" -eq 0 ]; then
      printf '  unknown       %-28s #%s\n' "$name" "$color"
    elif printf '%s\n' "$EXISTING" | grep -qxF "$name"; then
      printf '  exists        %-28s #%s  (color and description updated in place)\n' "$name" "$color"
      already=$((already + 1))
    else
      printf '  would create  %-28s #%s\n' "$name" "$color"
      would_create=$((would_create + 1))
    fi
    continue
  fi

  if gh label create "$name" --color "$color" --description "$desc" --force "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" >/dev/null 2>&1; then
    printf '  ok            %-28s #%s\n' "$name" "$color"
    created=$((created + 1))
  else
    printf '  FAILED        %-28s\n' "$name"
    failed=$((failed + 1))
  fi
done

if [ "$DRY_RUN" -eq 1 ]; then
  if [ "$EXISTING_KNOWN" -eq 1 ]; then
    echo "Dry run: $would_create label(s) would be created, $already already exist."
  else
    echo "Dry run: ${#LABELS[@]} label(s) would be ensured. Current state unknown."
  fi
  exit 0
fi

echo "Ensured $created of ${#LABELS[@]} labels."

if [ "$failed" -gt 0 ]; then
  echo "error: $failed label(s) failed. Check write access to $REPO." >&2
  exit 1
fi

echo
echo "Existing repo labels for area, component, and priority are unchanged."
echo "Review them with: gh label list --limit 200 ${REPO:+--repo $REPO}"
