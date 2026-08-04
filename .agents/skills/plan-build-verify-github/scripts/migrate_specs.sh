#!/usr/bin/env bash
# Migrate file-based specs in docs/specs/ to GitHub Issues.
#
# For each active spec file: creates a GitHub Issue whose body is the spec,
# then archives the source file under docs/specs/archive/ with a pointer to
# the issue. Completed specs are archived WITHOUT creating an issue, so the
# roadmap only contains work that is still live.
#
# The script is idempotent: files already carrying a `github_issue:` frontmatter
# key, and anything already under the archive directory, are skipped.
#
# Usage:
#   migrate_specs.sh [--dry-run] [--repo owner/name] [--specs-dir docs/specs]
#                    [--default-status draft|approved|implemented|blocked|completed]
#                    [--include-completed]
#
# Options:
#   --dry-run             Print planned actions. Touches nothing.
#   --repo                Target repo. Defaults to the current git remote.
#   --specs-dir           Spec directory. Defaults to docs/specs.
#   --default-status      Status to assume when a file declares none. Without
#                         this, undeclared files are skipped rather than guessed.
#   --include-completed   Also create issues for completed specs. Off by default.

set -euo pipefail

SPECS_DIR="docs/specs"
REPO=""
DRY_RUN=0
INCLUDE_COMPLETED=0
DEFAULT_STATUS=""

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --include-completed) INCLUDE_COMPLETED=1; shift ;;
    --repo)
      REPO="${2:-}"; [ -n "$REPO" ] || { echo "error: --repo requires a value" >&2; exit 2; }; shift 2 ;;
    --specs-dir)
      SPECS_DIR="${2:-}"; [ -n "$SPECS_DIR" ] || { echo "error: --specs-dir requires a value" >&2; exit 2; }; shift 2 ;;
    --default-status)
      DEFAULT_STATUS="${2:-}"; [ -n "$DEFAULT_STATUS" ] || { echo "error: --default-status requires a value" >&2; exit 2; }; shift 2 ;;
    -h|--help)
      sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)
      echo "error: unknown argument: $1" >&2; exit 2 ;;
  esac
done

# ---------------------------------------------------------------- preflight --

command -v gh >/dev/null 2>&1 || { echo "error: gh CLI not found" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "error: git not found" >&2; exit 1; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { echo "error: not inside a git repository" >&2; exit 1; }

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

[ -d "$SPECS_DIR" ] || { echo "error: $SPECS_DIR does not exist" >&2; exit 1; }

if [ "$DRY_RUN" -eq 0 ]; then
  gh auth status >/dev/null 2>&1 || { echo "error: gh is not authenticated. Run: gh auth login" >&2; exit 1; }
fi

REPO_ARGS=()
if [ -n "$REPO" ]; then
  REPO_ARGS=(--repo "$REPO")
elif [ "$DRY_RUN" -eq 0 ]; then
  REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)" || {
    echo "error: could not resolve the target repo. Pass --repo owner/name" >&2; exit 1; }
fi

if [ "$DRY_RUN" -eq 0 ] && [ -n "$(git status --porcelain -- "$SPECS_DIR")" ]; then
  echo "error: $SPECS_DIR has uncommitted changes. Commit or stash them first." >&2
  exit 1
fi

ARCHIVE_DIR="$SPECS_DIR/archive"
TODAY="$(date -u +%Y-%m-%d)"
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ------------------------------------------------------------------ helpers --

# fm_get <file> <key> -> frontmatter value, empty if absent
fm_get() {
  awk -v key="$2" '
    NR == 1 && $0 != "---" { exit }
    NR == 1 { infm = 1; next }
    infm && $0 == "---" { exit }
    infm {
      if (index($0, key ":") == 1) {
        v = substr($0, length(key) + 2)
        sub(/^[ \t]+/, "", v); sub(/[ \t\r]+$/, "", v)
        gsub(/^"|"$/, "", v); gsub(/^\047|\047$/, "", v)
        print v; exit
      }
    }
  ' "$1"
}

has_frontmatter() { [ "$(head -n 1 "$1")" = "---" ]; }

strip_frontmatter() {
  awk '
    NR == 1 && $0 == "---" { infm = 1; next }
    infm && $0 == "---" { infm = 0; next }
    !infm { print }
  ' "$1"
}

# body_status <file> -> value under a "## Status" heading, empty if absent
body_status() {
  strip_frontmatter "$1" | awk '
    /^##[ \t]+Status[ \t]*$/ { instatus = 1; next }
    instatus && /^#/ { exit }
    instatus && NF {
      sub(/^[ \t]+/, ""); sub(/[ \t\r]+$/, "")
      print; exit
    }
  '
}

first_h1() {
  strip_frontmatter "$1" | awk '/^#[ \t]+/ { sub(/^#[ \t]+/, ""); sub(/[ \t\r]+$/, ""); print; exit }'
}

drop_first_h1() {
  strip_frontmatter "$1" | awk '
    !dropped && /^#[ \t]+/ { dropped = 1; skipblank = 1; next }
    skipblank && !NF { skipblank = 0; next }
    { skipblank = 0; print }
  '
}

title_from_filename() {
  basename "$1" .md \
    | sed -E 's/^[0-9]{4}-[0-9]{2}-[0-9]{2}-//' \
    | sed -E 's/-(design|spec|plan)$//' \
    | tr '-_' '  ' \
    | awk '{ $1 = toupper(substr($1, 1, 1)) substr($1, 2); print }'
}

# classify <raw status> -> a status label, COMPLETED, or UNKNOWN
classify() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -d '`*_')" in
    draft|drafting|proposed|proposal|idea|new|exploring)
      echo "status:draft" ;;
    approved|ready|accepted|"in progress"|in-progress|inprogress|wip|building|active)
      echo "status:approved" ;;
    implemented|built|"needs verification"|needs-verification|"awaiting verification")
      echo "status:implemented" ;;
    blocked|"on hold"|on-hold|paused|waiting)
      echo "status:blocked" ;;
    verified|complete|completed|done|shipped|released|delivered|closed|merged|superseded|archived|obsolete|deprecated|cancelled|canceled|abandoned|wontfix|"wont do"|"won't do")
      echo "COMPLETED" ;;
    *)
      echo "UNKNOWN" ;;
  esac
}

# archive_file <src> <rel> <issue-number-or-empty> <raw-status>
archive_file() {
  src="$1"; rel="$2"; issue="$3"; raw_status="$4"
  dest="$ARCHIVE_DIR/$rel"
  tmp="$(mktemp -t pbvg-archive)"
  fm_title="$(first_h1 "$src")"
  [ -n "$fm_title" ] || fm_title="$(title_from_filename "$src")"

  mkdir -p "$(dirname "$dest")"

  if has_frontmatter "$src"; then
    awk -v issue="$issue" -v now="$NOW" '
      NR == 1 { print; infm = 1; next }
      infm && $0 == "---" {
        if (issue != "") {
          print "github_issue: " issue
          print "migrated: true"
        } else {
          print "migrated: false"
        }
        print "archived_at: " now
        print "---"
        infm = 0
        next
      }
      { print }
    ' "$src" > "$tmp"
  else
    {
      printf -- '---\n'
      printf 'type: Spec\n'
      printf 'title: %s\n' "$fm_title"
      printf 'status: %s\n' "${raw_status:-Unknown}"
      if [ -n "$issue" ]; then
        printf 'github_issue: %s\n' "$issue"
        printf 'migrated: true\n'
      else
        printf 'migrated: false\n'
      fi
      printf 'archived_at: %s\n' "$NOW"
      printf -- '---\n\n'
      cat "$src"
    } > "$tmp"
  fi

  fm_end="$(grep -n '^---$' "$tmp" | sed -n '2p' | cut -d: -f1)"

  {
    head -n "$fm_end" "$tmp"
    printf '\n'
    if [ -n "$issue" ]; then
      printf '> **Migrated to #%s.** The GitHub Issue is the canonical spec. This file is history and is not maintained.\n' "$issue"
    else
      printf '> **Completed before migration** (status: %s). Retained as history. Not tracked in GitHub Issues.\n' "${raw_status:-unknown}"
    fi
    tail -n +"$((fm_end + 1))" "$tmp"
  } > "$dest"

  rm -f "$tmp"

  if git ls-files --error-unmatch "$src" >/dev/null 2>&1; then
    git rm -q "$src"
    git add "$dest"
  else
    rm -f "$src"
  fi
}

write_index() {
  cat > "$SPECS_DIR/index.md" <<'INDEX'
# Specs

Specs for this project are GitHub Issues. This directory holds no spec documents.

## Read the roadmap

```bash
gh issue list --label kind:spec --state open            # all active specs
gh issue list --label status:approved --state open      # approved, ready to build
gh issue list --label status:implemented --state open   # built, awaiting verification
gh issue view <N>                                       # read a spec
gh sub-issue list <N>                                   # read an epic's phases
```

## Status model

| Label                | Meaning                                     |
| -------------------- | ------------------------------------------- |
| `status:draft`       | Being written or revised. Do not build.     |
| `status:approved`    | Approved by the maintainer. Ready to build. |
| `status:implemented` | Built and reported. Ready to verify.        |
| `status:verified`    | Acceptance evidence accepted.               |
| `status:blocked`     | Cannot proceed. See the issue body.         |

## Writing and executing specs

Use the `plan-build-verify-github` skill. It publishes specs as issues, runs Build
against approved issues, and posts acceptance evidence back to the issue.

## Archive

Pre-migration spec files are preserved under [`archive/`](./archive/) with links to
their issues. Completed specs were archived without an issue. Both are history and
are not maintained.
INDEX
}

append_log() {
  log="$SPECS_DIR/log.md"
  entry="$(mktemp -t pbvg-log)"

  {
    printf '## %s\n\n' "$TODAY"
    printf 'Migrated file-based specs to GitHub Issues. The issue is now the canonical spec.\n\n'
    if [ "${#MIGRATED[@]}" -gt 0 ]; then
      printf 'Migrated:\n\n'
      printf -- '- %s\n' "${MIGRATED[@]}"
      printf '\n'
    fi
    if [ "${#SKIPPED_COMPLETED[@]}" -gt 0 ]; then
      printf 'Archived without an issue (already complete):\n\n'
      printf -- '- %s\n' "${SKIPPED_COMPLETED[@]}"
      printf '\n'
    fi
    if [ "${#SKIPPED_UNKNOWN[@]}" -gt 0 ]; then
      printf 'Left in place (status unclear, needs a decision):\n\n'
      printf -- '- %s\n' "${SKIPPED_UNKNOWN[@]}"
      printf '\n'
    fi
  } > "$entry"

  if [ -f "$log" ]; then
    merged="$(mktemp -t pbvg-logmerge)"
    if head -n 1 "$log" | grep -q '^# '; then
      { head -n 1 "$log"; printf '\n'; cat "$entry"; tail -n +2 "$log" | sed '/./,$!d'; } > "$merged"
    else
      { cat "$entry"; cat "$log"; } > "$merged"
    fi
    mv "$merged" "$log"
  else
    { printf '# Specs Log\n\n'; cat "$entry"; } > "$log"
  fi

  rm -f "$entry"
}

# ------------------------------------------------------------------- labels --

if [ "$DRY_RUN" -eq 0 ] && [ -f "$SCRIPT_DIR/ensure_labels.sh" ]; then
  echo "Ensuring labels..."
  bash "$SCRIPT_DIR/ensure_labels.sh" ${REPO:+--repo "$REPO"} || {
    echo "error: label setup failed" >&2; exit 1; }
  echo
fi

# --------------------------------------------------------------- collection --

FILES=()
while IFS= read -r f; do
  FILES+=("$f")
done < <(
  find "$SPECS_DIR" -type f -name '*.md' \
    ! -path "$ARCHIVE_DIR/*" \
    ! -name 'index.md' \
    ! -name 'log.md' \
  | LC_ALL=C sort
)

if [ "${#FILES[@]}" -eq 0 ]; then
  echo "No spec files found under $SPECS_DIR. Nothing to migrate."
  exit 0
fi

echo "Repo:        ${REPO:-<dry run, unresolved>}"
echo "Specs dir:   $SPECS_DIR"
echo "Archive dir: $ARCHIVE_DIR"
echo "Candidates:  ${#FILES[@]}"
[ "$DRY_RUN" -eq 1 ] && echo "Mode:        dry run (no changes)"
echo

MIGRATED=()
SKIPPED_COMPLETED=()
SKIPPED_UNKNOWN=()
SKIPPED_ALREADY=()
FAILED=()

# ------------------------------------------------------------------ migrate --

for f in "${FILES[@]}"; do
  rel="${f#"$SPECS_DIR"/}"

  existing_issue="$(fm_get "$f" "github_issue")"
  if [ -n "$existing_issue" ]; then
    echo "skip  $rel  (already migrated to #$existing_issue)"
    SKIPPED_ALREADY+=("$rel -> #$existing_issue")
    continue
  fi

  raw_status="$(fm_get "$f" "status")"
  [ -n "$raw_status" ] || raw_status="$(body_status "$f")"
  [ -n "$raw_status" ] || raw_status="$DEFAULT_STATUS"

  state="$(classify "$raw_status")"

  if [ "$state" = "UNKNOWN" ]; then
    if [ -z "$raw_status" ]; then
      echo "skip  $rel  (no status declared; pass --default-status to migrate these)"
      SKIPPED_UNKNOWN+=("$rel (no status)")
    else
      echo "skip  $rel  (unrecognized status: '$raw_status')"
      SKIPPED_UNKNOWN+=("$rel (status: $raw_status)")
    fi
    continue
  fi

  title="$(fm_get "$f" "title")"
  [ -n "$title" ] || title="$(first_h1 "$f")"
  [ -n "$title" ] || title="$(title_from_filename "$f")"

  if [ "$state" = "COMPLETED" ] && [ "$INCLUDE_COMPLETED" -eq 0 ]; then
    echo "done  $rel  (status: $raw_status) -> archived, no issue created"
    SKIPPED_COMPLETED+=("$rel (status: $raw_status)")
    [ "$DRY_RUN" -eq 0 ] && archive_file "$f" "$rel" "" "$raw_status"
    continue
  fi

  [ "$state" = "COMPLETED" ] && state="status:verified"

  labels="kind:spec,$state"
  if ! strip_frontmatter "$f" | grep -qiE '^##[ \t]+Acceptance criteria[ \t]*$'; then
    labels="$labels,needs:acceptance-criteria"
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    printf 'plan  %s\n      -> issue "%s"\n      -> labels %s\n      -> archive %s/%s\n' \
      "$rel" "$title" "$labels" "$ARCHIVE_DIR" "$rel"
    MIGRATED+=("$rel -> (dry run)")
    continue
  fi

  body_file="$(mktemp -t pbvg-migrate)"
  {
    if ! drop_first_h1 "$f" | grep -qE '^##[ \t]+Status[ \t]*$'; then
      status_word="${state#status:}"
      printf '## Status\n\n%s%s\n\n' \
        "$(printf '%s' "${status_word:0:1}" | tr '[:lower:]' '[:upper:]')" "${status_word:1}"
    fi
    printf '_Migrated from `%s/%s` on %s. This issue is now the canonical spec._\n\n' \
      "$SPECS_DIR" "$rel" "$TODAY"
    drop_first_h1 "$f"
  } > "$body_file"

  if ! issue_out="$(gh issue create \
      --title "$title" \
      --body-file "$body_file" \
      --label "$labels" \
      "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" 2>&1)"; then
    echo "FAIL  $rel  (gh issue create failed: $issue_out)" >&2
    FAILED+=("$rel")
    rm -f "$body_file"
    continue
  fi
  rm -f "$body_file"

  issue_url="$(printf '%s\n' "$issue_out" | grep -oE 'https://[^ ]+/issues/[0-9]+' | tail -n 1)"
  issue_num="${issue_url##*/}"

  if [ -z "$issue_num" ]; then
    echo "FAIL  $rel  (issue may have been created but its number could not be parsed; resolve by hand)" >&2
    FAILED+=("$rel")
    continue
  fi

  echo "ok    $rel  -> #$issue_num  [$labels]"
  MIGRATED+=("$rel -> #$issue_num")
  archive_file "$f" "$rel" "$issue_num" "$raw_status"
done

# ------------------------------------------------------------------ summary --

echo
if [ "$DRY_RUN" -eq 1 ]; then
  echo "Dry run summary"
  echo "  would migrate:            ${#MIGRATED[@]}"
  echo "  would archive, no issue:  ${#SKIPPED_COMPLETED[@]} (completed)"
  echo "  skipped, status unclear:  ${#SKIPPED_UNKNOWN[@]}"
  echo "  skipped, already done:    ${#SKIPPED_ALREADY[@]}"
  if [ "${#SKIPPED_UNKNOWN[@]}" -gt 0 ]; then
    echo
    echo "Status could not be determined for:"
    printf '  %s\n' "${SKIPPED_UNKNOWN[@]}"
    echo "Set a status in each file, or re-run with --default-status <status>."
  fi
  echo
  echo "Re-run without --dry-run to apply."
  exit 0
fi

if [ "${#MIGRATED[@]}" -gt 0 ] || [ "${#SKIPPED_COMPLETED[@]}" -gt 0 ]; then
  write_index
  append_log
  git add "$SPECS_DIR/index.md" "$SPECS_DIR/log.md" 2>/dev/null || true
fi

echo "Summary"
echo "  migrated to issues:       ${#MIGRATED[@]}"
echo "  archived, no issue:       ${#SKIPPED_COMPLETED[@]} (completed)"
echo "  skipped, status unclear:  ${#SKIPPED_UNKNOWN[@]}"
echo "  skipped, already done:    ${#SKIPPED_ALREADY[@]}"
echo "  failed:                   ${#FAILED[@]}"

if [ "${#SKIPPED_UNKNOWN[@]}" -gt 0 ]; then
  echo
  echo "These files were left in place because their status could not be determined:"
  printf '  %s\n' "${SKIPPED_UNKNOWN[@]}"
  echo "Set a status in each file, or re-run with --default-status <status>."
fi

if [ "${#FAILED[@]}" -gt 0 ]; then
  echo
  echo "These files failed to migrate and were left in place:"
  printf '  %s\n' "${FAILED[@]}"
  exit 1
fi

echo
echo "Changes are staged, not committed. Review with: git status && git diff --cached"
