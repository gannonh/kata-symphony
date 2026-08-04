#!/usr/bin/env bash
# Migrate file-based specs in docs/specs/ to GitHub Issues.
#
# Runs in three stages: assess every candidate file, report the plan, then apply
# it. For each live spec the script creates a GitHub Issue whose body is the
# spec, archives the source file under docs/specs/archive/, and rewrites the
# Markdown links that pointed at the old path. Completed specs are archived
# without an issue so the roadmap only contains work that is still live.
#
# Re-runs are safe: files carrying a `github_issue:` frontmatter key are
# skipped, anything already under the archive is ignored, and issues are
# matched back to their source file by a key embedded in the issue body.
#
# Usage:
#   migrate_specs.sh --assess
#   migrate_specs.sh --dry-run
#   migrate_specs.sh [options]
#
# Options:
#   --assess                 Report status, source, confidence, title, and the
#                            planned action per file. Writes nothing, contacts
#                            GitHub read-only.
#   --dry-run                Print the full plan, including label and link
#                            changes. Writes nothing.
#   --repo owner/name        Target repo. Defaults to the current git remote.
#   --specs-dir <path>       Spec directory. Defaults to docs/specs.
#   --docs-root <path>       OKF docs root for root-relative links. Default docs.
#   --status-map <file>      Per-file status overrides: `<relpath> <status>` per
#                            line, `#` comments allowed. Highest precedence.
#   --default-status <s>     Status for files that declare none. Without it,
#                            undeclared files are skipped rather than guessed.
#   --implemented-action <a> What to do with Implemented specs:
#                            migrate (default) | archive | blocked | skip
#   --include-completed      Create issues for completed specs too. Off by default.
#   --allow-conflicts        Proceed when a file has conflicting status evidence.
#                            Without it, conflicts stop the run before any write.
#   --replace-index          Replace docs/specs/index.md wholesale. Without it,
#                            existing index content is preserved below the new
#                            GitHub pointer.
#   --ensure-labels          Always run label setup. By default labels are only
#                            created when at least one issue will be created.
#   --no-rewrite-links       Skip Markdown link rewriting after archiving.
#   --no-verify-labels       Skip the read-only pre-migration issue/label report.

set -euo pipefail

SPECS_DIR="docs/specs"
DOCS_ROOT="docs"
REPO=""
DRY_RUN=0
ASSESS=0
INCLUDE_COMPLETED=0
DEFAULT_STATUS=""
STATUS_MAP_FILE=""
IMPLEMENTED_ACTION="migrate"
ALLOW_CONFLICTS=0
REPLACE_INDEX=0
FORCE_LABELS=0
REWRITE_LINKS=1
VERIFY_LABELS=1

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --assess) ASSESS=1; shift ;;
    --include-completed) INCLUDE_COMPLETED=1; shift ;;
    --allow-conflicts) ALLOW_CONFLICTS=1; shift ;;
    --replace-index) REPLACE_INDEX=1; shift ;;
    --ensure-labels) FORCE_LABELS=1; shift ;;
    --no-rewrite-links) REWRITE_LINKS=0; shift ;;
    --no-verify-labels) VERIFY_LABELS=0; shift ;;
    --repo)
      REPO="${2:-}"; [ -n "$REPO" ] || { echo "error: --repo requires a value" >&2; exit 2; }; shift 2 ;;
    --specs-dir)
      SPECS_DIR="${2:-}"; [ -n "$SPECS_DIR" ] || { echo "error: --specs-dir requires a value" >&2; exit 2; }; shift 2 ;;
    --docs-root)
      DOCS_ROOT="${2:-}"; [ -n "$DOCS_ROOT" ] || { echo "error: --docs-root requires a value" >&2; exit 2; }; shift 2 ;;
    --status-map)
      STATUS_MAP_FILE="${2:-}"; [ -n "$STATUS_MAP_FILE" ] || { echo "error: --status-map requires a value" >&2; exit 2; }; shift 2 ;;
    --default-status)
      DEFAULT_STATUS="${2:-}"; [ -n "$DEFAULT_STATUS" ] || { echo "error: --default-status requires a value" >&2; exit 2; }; shift 2 ;;
    --implemented-action)
      IMPLEMENTED_ACTION="${2:-}"; [ -n "$IMPLEMENTED_ACTION" ] || { echo "error: --implemented-action requires a value" >&2; exit 2; }; shift 2 ;;
    -h|--help)
      sed -n '2,45p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)
      echo "error: unknown argument: $1" >&2; exit 2 ;;
  esac
done

case "$IMPLEMENTED_ACTION" in
  migrate|archive|blocked|skip) ;;
  *) echo "error: --implemented-action must be one of: migrate archive blocked skip" >&2; exit 2 ;;
esac

# --assess implies no writes at all.
[ "$ASSESS" -eq 1 ] && DRY_RUN=1

WRITES=1
[ "$DRY_RUN" -eq 1 ] && WRITES=0

# ---------------------------------------------------------------- preflight --

command -v gh >/dev/null 2>&1 || { echo "error: gh CLI not found" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "error: git not found" >&2; exit 1; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { echo "error: not inside a git repository" >&2; exit 1; }

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

[ -d "$SPECS_DIR" ] || { echo "error: $SPECS_DIR does not exist" >&2; exit 1; }

GH_READY=0
if gh auth status >/dev/null 2>&1; then
  GH_READY=1
elif [ "$WRITES" -eq 1 ]; then
  echo "error: gh is not authenticated. Run: gh auth login" >&2
  exit 1
fi

REPO_ARGS=()
if [ -n "$REPO" ]; then
  REPO_ARGS=(--repo "$REPO")
elif [ "$GH_READY" -eq 1 ]; then
  REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
  if [ -z "$REPO" ] && [ "$WRITES" -eq 1 ]; then
    echo "error: could not resolve the target repo. Pass --repo owner/name" >&2
    exit 1
  fi
  [ -n "$REPO" ] && REPO_ARGS=(--repo "$REPO")
fi

if [ "$WRITES" -eq 1 ] && [ -n "$(git status --porcelain -- "$SPECS_DIR")" ]; then
  echo "error: $SPECS_DIR has uncommitted changes. Commit or stash them first." >&2
  exit 1
fi

if [ -n "$STATUS_MAP_FILE" ] && [ ! -f "$STATUS_MAP_FILE" ]; then
  echo "error: --status-map file not found: $STATUS_MAP_FILE" >&2
  exit 1
fi

ARCHIVE_DIR="$SPECS_DIR/archive"
TODAY="$(date -u +%Y-%m-%d)"
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

WORK_DIR="$(mktemp -d -t pbvg-migrate)"
trap 'rm -rf "$WORK_DIR"' EXIT

RECORDS="$WORK_DIR/records.tsv"
PATH_MAP="$WORK_DIR/pathmap.tsv"
ISSUE_KEYS="$WORK_DIR/issue-keys.tsv"

# Records are separated by US (0x1f), not tab. Tab is an IFS whitespace
# character, so `read` would collapse consecutive tabs and silently shift every
# field after an empty one.
SEP="$(printf '\037')"
: > "$RECORDS"
: > "$PATH_MAP"
: > "$ISSUE_KEYS"

# ------------------------------------------------------------------ helpers --
#
# Every reader below consumes its input file directly and never exits early from
# inside a pipeline. Early `exit` in a piped awk sends SIGPIPE upstream, which
# under `set -o pipefail` aborts the whole script. Match state is recorded and
# printed at END instead.

# fm_get <file> <key> -> frontmatter value, empty when absent
fm_get() {
  awk -v key="$2" '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
    NR == 1 && $0 != "---" { done = 1 }
    done { next }
    NR == 1 { infm = 1; next }
    infm && $0 == "---" { done = 1; next }
    infm && !found {
      if (index($0, key ":") == 1) {
        v = trim(substr($0, length(key) + 2))
        gsub(/^\042|\042$/, "", v)
        gsub(/^\047|\047$/, "", v)
        val = trim(v); found = 1
      }
    }
    END { if (found) print val }
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

# has_h2 <file> <heading regex> -> "1" or "0". Counts, never exits early.
has_h2() {
  awk -v want="$2" '
    NR == 1 && $0 == "---" { infm = 1; next }
    infm && $0 == "---" { infm = 0; next }
    infm { next }
    tolower($0) ~ want { hit = 1 }
    END { print hit ? 1 : 0 }
  ' "$1"
}

# first_h1 <file> -> text of the first level-one heading in the body
first_h1() {
  awk '
    NR == 1 && $0 == "---" { infm = 1; next }
    infm && $0 == "---" { infm = 0; next }
    infm { next }
    !found && /^#[ \t]+/ {
      line = $0
      sub(/^#[ \t]+/, "", line); sub(/[ \t\r]+$/, "", line)
      val = line; found = 1
    }
    END { if (found) print val }
  ' "$1"
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
    | sed -E 's/-(design|spec|plan|report|notes)$//' \
    | tr '-_' '  ' \
    | awk '{ $1 = toupper(substr($1, 1, 1)) substr($1, 2); print }'
}

# status_candidates <file> -> "<source>\t<value>" lines, most specific first.
#
# Recognized shapes:
#   frontmatter   status: Approved
#   section       ## Status  ->  Approved
#   phase         ## Status  ->  - **Plan**: Approved / - **Build**: Implemented
#   inline        **Status**: Implemented   (anywhere in the body)
status_candidates() {
  awk '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
    function clean(s) {
      s = trim(s)
      gsub(/^\042|\042$/, "", s)
      gsub(/^\047|\047$/, "", s)
      gsub(/[`*_]/, "", s)
      sub(/\.$/, "", s)
      return trim(s)
    }
    NR == 1 && $0 == "---" { infm = 1; next }
    infm && $0 == "---" { infm = 0; next }
    infm {
      if (!fm && index($0, "status:") == 1) fm = clean(substr($0, 8))
      next
    }
    /^##+[ \t]+Status[ \t]*:?[ \t]*$/ { instatus = 1; next }
    instatus && /^#/ { instatus = 0 }
    {
      line = $0
      sub(/^[ \t]*[-*+][ \t]+/, "", line)
      line = trim(line)
      bare = line
      gsub(/[`*_]/, "", bare)
      if (match(bare, /^(Plan|Build|Verify|Design|Spec|Implementation|Verification)[ \t]*:[ \t]*/)) {
        key = bare; sub(/[ \t]*:.*$/, "", key)
        val = clean(substr(bare, RLENGTH + 1))
        if (val != "" && instatus) {
          if (key == "Plan" || key == "Design" || key == "Spec") { if (!p_plan) p_plan = val }
          else if (key == "Build" || key == "Implementation") { if (!p_build) p_build = val }
          else { if (!p_verify) p_verify = val }
        }
        next
      }
      if (match(bare, /^Status[ \t]*:[ \t]*/)) {
        val = clean(substr(bare, RLENGTH + 1))
        if (val != "" && !inline) inline = val
        next
      }
      if (instatus && !section && NF) section = clean(line)
    }
    END {
      if (fm != "") print "frontmatter\t" fm
      if (p_verify != "") print "phase\tVerify: " p_verify
      else if (p_build != "") print "phase\tBuild: " p_build
      else if (p_plan != "") print "phase\tPlan: " p_plan
      if (section != "") print "section\t" section
      if (inline != "") print "inline\t" inline
    }
  ' "$1"
}

# classify <raw status> -> a status label, COMPLETED, or UNKNOWN.
# Accepts bare values and `Phase: Value` forms.
classify() {
  v="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -d '`*_' | sed -E 's/^[ \t]+//; s/[ \t]+$//')"
  case "$v" in
    plan:*|build:*|verify:*|design:*|spec:*|implementation:*|verification:*)
      v="$(printf '%s' "$v" | sed -E 's/^[a-z]+:[ \t]*//')" ;;
  esac
  case "$v" in
    draft|drafting|proposed|proposal|idea|new|exploring|planned|planning)
      echo "status:draft" ;;
    approved|ready|accepted|"in progress"|in-progress|inprogress|wip|building|active|"in review")
      echo "status:approved" ;;
    implemented|built|complete\ pending\ verification|"needs verification"|needs-verification|"awaiting verification"|"pending verification")
      echo "status:implemented" ;;
    blocked|"on hold"|on-hold|paused|waiting|stalled)
      echo "status:blocked" ;;
    verified|complete|completed|done|shipped|released|delivered|closed|merged|superseded|archived|obsolete|deprecated|cancelled|canceled|abandoned|wontfix|"wont do"|"won't do"|"not planned")
      echo "COMPLETED" ;;
    *)
      echo "UNKNOWN" ;;
  esac
}

confidence_for() {
  case "$1" in
    override) echo "high" ;;
    frontmatter) echo "high" ;;
    phase) echo "high" ;;
    section) echo "high" ;;
    inline) echo "medium" ;;
    default) echo "low" ;;
    *) echo "none" ;;
  esac
}

status_map_lookup() {
  [ -n "$STATUS_MAP_FILE" ] || return 0
  awk -v want="$1" '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
    { sub(/#.*$/, "") }
    { line = trim($0) }
    line == "" { next }
    !found {
      key = line
      sub(/[ \t=:].*$/, "", key)
      if (key == want) {
        val = trim(substr(line, length(key) + 1))
        sub(/^[ \t=:]+/, "", val)
        found = 1; out = val
      }
    }
    END { if (found) print out }
  ' "$STATUS_MAP_FILE"
}

source_key() { printf 'pbvg-source:%s' "$1"; }

# ---------------------------------------------------------------- archiving --

# archive_file <src> <rel> <issue-number-or-empty> <source-status>
#
# Canonical archive status is normalized: `Migrated` when the spec became an
# issue, `Completed` when it was archived as finished work. The status the file
# carried before the migration is preserved under `source_status`.
archive_file() {
  src="$1"; rel="$2"; issue="$3"; src_status="$4"
  dest="$ARCHIVE_DIR/$rel"
  tmp="$WORK_DIR/archive.tmp"
  fm_title="$(fm_get "$src" "title")"
  [ -n "$fm_title" ] || fm_title="$(first_h1 "$src")"
  [ -n "$fm_title" ] || fm_title="$(title_from_filename "$src")"
  fm_type="$(fm_get "$src" "type")"
  [ -n "$fm_type" ] || fm_type="Spec"

  if [ -n "$issue" ]; then
    canonical_status="Migrated"
  else
    canonical_status="Completed"
  fi

  mkdir -p "$(dirname "$dest")"

  if has_frontmatter "$src"; then
    awk -v issue="$issue" -v now="$NOW" -v status="$canonical_status" \
        -v src_status="$src_status" -v ftype="$fm_type" -v ftitle="$fm_title" '
      NR == 1 { print; infm = 1; next }
      infm && $0 == "---" {
        if (!seen_type) print "type: " ftype
        if (!seen_title) print "title: " ftitle
        print "status: " status
        if (src_status != "") print "source_status: " src_status
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
      infm && index($0, "status:") == 1 { next }
      infm && index($0, "source_status:") == 1 { next }
      infm && index($0, "migrated:") == 1 { next }
      infm && index($0, "github_issue:") == 1 { next }
      infm && index($0, "archived_at:") == 1 { next }
      infm && index($0, "type:") == 1 { seen_type = 1; print; next }
      infm && index($0, "title:") == 1 { seen_title = 1; print; next }
      { print }
    ' "$src" > "$tmp"
  else
    {
      printf -- '---\n'
      printf 'type: %s\n' "$fm_type"
      printf 'title: %s\n' "$fm_title"
      printf 'status: %s\n' "$canonical_status"
      [ -n "$src_status" ] && printf 'source_status: %s\n' "$src_status"
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
      printf '> **Completed before migration** (source status: %s). Retained as history. Not tracked in GitHub Issues.\n' "${src_status:-unknown}"
    fi
    tail -n +"$((fm_end + 1))" "$tmp"
  } > "$dest"

  rm -f "$tmp"

  printf '%s\t%s\n' "$src" "$dest" >> "$PATH_MAP"

  if git ls-files --error-unmatch "$src" >/dev/null 2>&1; then
    git rm -q "$src"
    git add "$dest"
  else
    rm -f "$src"
  fi
}

# -------------------------------------------------------------------- index --

write_index() {
  index="$SPECS_DIR/index.md"
  preserved=""

  if [ -f "$index" ] && [ "$REPLACE_INDEX" -eq 0 ]; then
    preserved="$WORK_DIR/preserved-index.md"
    # Drop the old H1 and any leading or trailing blank lines; keep the rest.
    awk '
      !dropped && /^#[ \t]+/ { dropped = 1; skipblank = 1; next }
      skipblank && !NF { next }
      { skipblank = 0 }
      !NF { pending++; next }
      { while (pending-- > 0) print ""; pending = 0; print }
    ' "$index" > "$preserved"
    [ -s "$preserved" ] || preserved=""
  fi

  {
    cat <<'INDEX'
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

    if [ -n "$preserved" ]; then
      printf '\n## Roadmap context carried over from the previous index\n\n'
      printf 'The content below predates the migration to GitHub Issues. Links to issues\n'
      printf 'remain valid; links to spec files now point into `archive/`. Reconcile this\n'
      printf 'section against the GitHub roadmap, then delete it.\n\n'
      cat "$preserved"
      printf '\n'
    fi
  } > "$index.new"

  mv "$index.new" "$index"
}

# ---------------------------------------------------------------------- log --

append_log() {
  log="$SPECS_DIR/log.md"
  entry="$WORK_DIR/log-entry.md"

  {
    printf 'Migrated file-based specs to GitHub Issues. The issue is now the canonical spec.\n\n'
    if [ "${#MIGRATED[@]}" -gt 0 ]; then
      printf 'Migrated:\n\n'
      printf -- '- %s\n' "${MIGRATED[@]}"
      printf '\n'
    fi
    if [ "${#ARCHIVED[@]}" -gt 0 ]; then
      printf 'Archived without an issue:\n\n'
      printf -- '- %s\n' "${ARCHIVED[@]}"
      printf '\n'
    fi
    if [ "${#SKIPPED_UNKNOWN[@]}" -gt 0 ]; then
      printf 'Left in place (status unclear, needs a decision):\n\n'
      printf -- '- %s\n' "${SKIPPED_UNKNOWN[@]}"
      printf '\n'
    fi
    if [ "${#SKIPPED_POLICY[@]}" -gt 0 ]; then
      printf 'Left in place (excluded by policy):\n\n'
      printf -- '- %s\n' "${SKIPPED_POLICY[@]}"
      printf '\n'
    fi
    if [ "${#FAILED[@]}" -gt 0 ]; then
      printf 'Failed and left in place:\n\n'
      printf -- '- %s\n' "${FAILED[@]}"
      printf '\n'
    fi
  } > "$entry"

  merged="$WORK_DIR/log-merged.md"

  if [ ! -f "$log" ]; then
    { printf '# Specs Log\n\n'; printf '## %s\n\n' "$TODAY"; cat "$entry"; } > "$log"
    return
  fi

  if grep -q "^## $TODAY\$" "$log"; then
    # Same-day heading already exists. Insert the entry under it and keep
    # newest-first ordering intact.
    awk -v today="## $TODAY" -v entry="$entry" '
      $0 == today && !inserted {
        print
        print ""
        while ((getline line < entry) > 0) print line
        close(entry)
        inserted = 1
        skipblank = 1
        next
      }
      skipblank && !NF { next }
      { skipblank = 0; print }
    ' "$log" > "$merged"
  elif head -n 1 "$log" | grep -q '^# '; then
    {
      head -n 1 "$log"
      printf '\n## %s\n\n' "$TODAY"
      cat "$entry"
      tail -n +2 "$log" | sed '/./,$!d'
    } > "$merged"
  else
    { printf '## %s\n\n' "$TODAY"; cat "$entry"; cat "$log"; } > "$merged"
  fi

  mv "$merged" "$log"
}

# ------------------------------------------------------- existing issue map --

# Build a source-key -> issue-number map from the repo's existing issues so a
# rerun after a partial failure reuses issues instead of duplicating them.
refresh_issue_keys() {
  [ "$GH_READY" -eq 1 ] || return 0
  gh issue list --state all --limit 500 --json number,body "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" \
    -q '.[] | "\(.number)\t\(.body | gsub("\n"; " "))"' 2>/dev/null > "$ISSUE_KEYS" || : > "$ISSUE_KEYS"
}

find_issue_by_key() {
  [ -s "$ISSUE_KEYS" ] || return 0
  awk -F'\t' -v key="$1" '
    !found && index($2, key) { found = 1; num = $1 }
    END { if (found) print num }
  ' "$ISSUE_KEYS"
}

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

echo "Repo:        ${REPO:-<unresolved>}"
echo "Specs dir:   $SPECS_DIR"
echo "Archive dir: $ARCHIVE_DIR"
echo "Candidates:  ${#FILES[@]}"
[ "$ASSESS" -eq 1 ] && echo "Mode:        assess (read-only)"
[ "$ASSESS" -eq 0 ] && [ "$DRY_RUN" -eq 1 ] && echo "Mode:        dry run (no changes)"
echo "Policy:      implemented -> $IMPLEMENTED_ACTION; completed -> $([ "$INCLUDE_COMPLETED" -eq 1 ] && echo migrate || echo archive)"
echo

refresh_issue_keys

# ------------------------------------------------------------------- assess --

CONFLICTS=()

for f in "${FILES[@]}"; do
  rel="${f#"$SPECS_DIR"/}"

  existing_issue="$(fm_get "$f" "github_issue")"
  if [ -n "$existing_issue" ]; then
    printf '%s\037%s\037%s\037%s\037%s\037%s\037%s\037%s\n' \
      "$rel" "" "existing" "high" "" "already" "" "#$existing_issue" >> "$RECORDS"
    continue
  fi

  raw=""
  src=""
  conflict=""

  override="$(status_map_lookup "$rel")"
  if [ -n "$override" ]; then
    raw="$override"
    src="override"
  else
    seen_states=""
    while IFS="$(printf '\t')" read -r cand_src cand_val; do
      [ -n "$cand_val" ] || continue
      cand_state="$(classify "$cand_val")"
      if [ -z "$raw" ]; then
        raw="$cand_val"
        src="$cand_src"
      fi
      if [ "$cand_state" != "UNKNOWN" ]; then
        case " $seen_states " in
          *" $cand_state "*) ;;
          *) seen_states="$seen_states $cand_state" ;;
        esac
      fi
    done < <(status_candidates "$f")

    set -- $seen_states
    if [ "$#" -gt 1 ]; then
      conflict="$(printf '%s' "$seen_states" | sed 's/^ //; s/ /, /g')"
      CONFLICTS+=("$rel: $conflict")
    fi

    # Prefer a candidate that actually classifies over the first one found.
    if [ "$(classify "$raw")" = "UNKNOWN" ]; then
      while IFS="$(printf '\t')" read -r cand_src cand_val; do
        [ -n "$cand_val" ] || continue
        if [ "$(classify "$cand_val")" != "UNKNOWN" ]; then
          raw="$cand_val"
          src="$cand_src"
          break
        fi
      done < <(status_candidates "$f")
    fi
  fi

  if [ -z "$raw" ] && [ -n "$DEFAULT_STATUS" ]; then
    raw="$DEFAULT_STATUS"
    src="default"
  fi

  state="$(classify "$raw")"
  conf="$(confidence_for "$src")"
  [ -n "$raw" ] || { src="none"; conf="none"; }

  title="$(fm_get "$f" "title")"
  [ -n "$title" ] || title="$(first_h1 "$f")"
  [ -n "$title" ] || title="$(title_from_filename "$f")"
  title="$(printf '%s' "$title" | tr '\t' ' ')"

  case "$state" in
    UNKNOWN)
      action="skip" ;;
    COMPLETED)
      if [ "$INCLUDE_COMPLETED" -eq 1 ]; then action="migrate"; else action="archive"; fi ;;
    status:implemented)
      case "$IMPLEMENTED_ACTION" in
        migrate) action="migrate" ;;
        archive) action="archive" ;;
        blocked) action="migrate-blocked" ;;
        skip)    action="policy-skip" ;;
      esac ;;
    *)
      action="migrate" ;;
  esac

  printf '%s\037%s\037%s\037%s\037%s\037%s\037%s\037%s\n' \
    "$rel" "$raw" "$src" "$conf" "$state" "$action" "$conflict" "$title" >> "$RECORDS"
done

# ------------------------------------------------------------ assess report --

WILL_CREATE_ISSUES=0
while IFS="$SEP" read -r rel raw src conf state action conflict title; do
  case "$action" in
    migrate|migrate-blocked) WILL_CREATE_ISSUES=$((WILL_CREATE_ISSUES + 1)) ;;
  esac
done < "$RECORDS"

echo "Assessment"
printf '  %-44s %-22s %-12s %-7s %s\n' "FILE" "STATUS" "SOURCE" "CONF" "ACTION"
while IFS="$SEP" read -r rel raw src conf state action conflict title; do
  printf '  %-44s %-22s %-12s %-7s %s\n' \
    "$(printf '%.44s' "$rel")" \
    "$(printf '%.22s' "${raw:-<none>}")" \
    "$(printf '%.12s' "${src:-none}")" \
    "$conf" \
    "$action"
  printf '  %-44s %s\n' "" "title: $title"
  [ -n "$conflict" ] && printf '  %-44s conflict: %s\n' "" "$conflict"
done < "$RECORDS"
echo

if [ "${#CONFLICTS[@]}" -gt 0 ]; then
  echo "Conflicting status evidence (${#CONFLICTS[@]}):"
  printf '  %s\n' "${CONFLICTS[@]}"
  echo "Resolve each in the source file, or decide it explicitly with --status-map."
  echo
fi

# ---------------------------------------------- existing roadmap and labels --

if [ "$VERIFY_LABELS" -eq 1 ] && [ "$GH_READY" -eq 1 ] && [ -n "$REPO" ]; then
  echo "Existing GitHub roadmap"
  untagged="$(gh issue list --state open --limit 200 \
      --json number,title,labels "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" \
      -q '[.[] | select([.labels[].name] | index("kind:spec") | not)] | length' 2>/dev/null || echo "?")"
  spec_tagged="$(gh issue list --state open --limit 200 --label kind:spec \
      --json number "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" -q 'length' 2>/dev/null || echo "?")"
  [ -n "$untagged" ] || untagged="?"
  [ -n "$spec_tagged" ] || spec_tagged="?"
  echo "  open issues already labeled kind:spec: $spec_tagged"
  echo "  open issues without kind:spec:         $untagged"
  if [ "$untagged" != "0" ] && [ "$untagged" != "?" ]; then
    echo "  These will not appear in 'gh issue list --label kind:spec'. Review them and"
    echo "  decide a labeling plan with the maintainer. This script never relabels"
    echo "  issues it did not create."
    gh issue list --state open --limit 200 \
      --json number,title,labels "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" \
      -q '.[] | select([.labels[].name] | index("kind:spec") | not) | "    #\(.number) \(.title)  [\([.labels[].name] | join(","))]"' \
      2>/dev/null | head -n 30 || true
  fi
  echo
fi

# ------------------------------------------------------------------ labels --

label_plan() {
  if [ "$GH_READY" -eq 0 ]; then
    echo "Labels: state unknown (gh is not authenticated). Run 'gh auth login' to see the plan."
    return 0
  fi
  bash "$SCRIPT_DIR/ensure_labels.sh" --dry-run ${REPO:+--repo "$REPO"} || true
}

NEED_LABELS=0
if [ "$FORCE_LABELS" -eq 1 ] || [ "$WILL_CREATE_ISSUES" -gt 0 ]; then
  NEED_LABELS=1
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "Label plan"
  if [ "$NEED_LABELS" -eq 1 ]; then
    label_plan
  else
    echo "  No issues will be created, so label setup is skipped."
    echo "  Pass --ensure-labels to create the taxonomy anyway."
  fi
  echo
fi

# ------------------------------------------------------------ dry-run exit --

if [ "$DRY_RUN" -eq 1 ]; then
  echo "Planned changes"
  while IFS="$SEP" read -r rel raw src conf state action conflict title; do
    case "$action" in
      migrate)
        printf '  issue   %s\n            title:   %s\n            labels:  kind:spec,%s\n            archive: %s/%s\n' \
          "$rel" "$title" "$([ "$state" = "COMPLETED" ] && echo "status:verified" || echo "$state")" "$ARCHIVE_DIR" "$rel" ;;
      migrate-blocked)
        printf '  issue   %s\n            title:   %s\n            labels:  kind:spec,status:blocked\n            archive: %s/%s\n' \
          "$rel" "$title" "$ARCHIVE_DIR" "$rel" ;;
      archive)
        printf '  archive %s  (status: %s) -> %s/%s, no issue\n' "$rel" "${raw:-unknown}" "$ARCHIVE_DIR" "$rel" ;;
      policy-skip)
        printf '  skip    %s  (implemented, --implemented-action skip)\n' "$rel" ;;
      already)
        printf '  skip    %s  (already migrated to %s)\n' "$rel" "$title" ;;
      skip)
        printf '  skip    %s  (status unclear: %s)\n' "$rel" "${raw:-none declared}" ;;
    esac
  done < "$RECORDS"
  echo

  echo "Index:  $SPECS_DIR/index.md -> GitHub pointer$([ "$REPLACE_INDEX" -eq 1 ] && echo " (existing content REPLACED)" || echo " (existing content preserved below the pointer)")"
  echo "Log:    $SPECS_DIR/log.md   -> migration entry under ## $TODAY"
  if [ "$REWRITE_LINKS" -eq 1 ]; then
    echo "Links:  Markdown links to archived paths will be rewritten across tracked docs"
  else
    echo "Links:  rewriting disabled (--no-rewrite-links); cross-links will break"
  fi
  echo

  if [ "${#CONFLICTS[@]}" -gt 0 ] && [ "$ALLOW_CONFLICTS" -eq 0 ]; then
    echo "This run would stop before writing: ${#CONFLICTS[@]} file(s) have conflicting status evidence."
    echo "Resolve them, or pass --allow-conflicts to accept the highest-precedence value."
    echo
  fi

  [ "$ASSESS" -eq 1 ] && echo "Assessment complete. Re-run with --dry-run for the full plan, then without it to apply."
  [ "$ASSESS" -eq 0 ] && echo "Re-run without --dry-run to apply."
  exit 0
fi

# ---------------------------------------------------------- conflict gate --

if [ "${#CONFLICTS[@]}" -gt 0 ] && [ "$ALLOW_CONFLICTS" -eq 0 ]; then
  echo "error: ${#CONFLICTS[@]} file(s) have conflicting status evidence. Nothing was written." >&2
  printf '  %s\n' "${CONFLICTS[@]}" >&2
  echo "Resolve each source file, decide it with --status-map, or pass --allow-conflicts." >&2
  exit 1
fi

# ------------------------------------------------------------------ apply --

if [ "$NEED_LABELS" -eq 1 ]; then
  echo "Ensuring labels..."
  bash "$SCRIPT_DIR/ensure_labels.sh" ${REPO:+--repo "$REPO"} || {
    echo "error: label setup failed" >&2; exit 1; }
  echo
else
  echo "No issues to create; skipping label setup. Pass --ensure-labels to force it."
  echo
fi

MIGRATED=()
ARCHIVED=()
SKIPPED_UNKNOWN=()
SKIPPED_POLICY=()
SKIPPED_ALREADY=()
RECOVERED=()
FAILED=()

while IFS="$SEP" read -r rel raw src conf state action conflict title; do
  f="$SPECS_DIR/$rel"

  case "$action" in
    already)
      echo "skip  $rel  (already migrated to $title)"
      SKIPPED_ALREADY+=("$rel -> $title")
      continue ;;
    skip)
      if [ -z "$raw" ]; then
        echo "skip  $rel  (no status declared; use --status-map or --default-status)"
        SKIPPED_UNKNOWN+=("$rel (no status)")
      else
        echo "skip  $rel  (unrecognized status: '$raw')"
        SKIPPED_UNKNOWN+=("$rel (status: $raw)")
      fi
      continue ;;
    policy-skip)
      echo "skip  $rel  (implemented; --implemented-action skip)"
      SKIPPED_POLICY+=("$rel (implemented, skipped by policy)")
      continue ;;
    archive)
      echo "done  $rel  (source status: ${raw:-unknown}) -> archived, no issue"
      ARCHIVED+=("$rel (source status: ${raw:-unknown})")
      archive_file "$f" "$rel" "" "$raw"
      continue ;;
  esac

  # migrate / migrate-blocked
  if [ "$action" = "migrate-blocked" ]; then
    issue_state="status:blocked"
  elif [ "$state" = "COMPLETED" ]; then
    issue_state="status:verified"
  else
    issue_state="$state"
  fi

  labels="kind:spec,$issue_state"
  if [ "$(has_h2 "$f" '^##+[ \t]+acceptance criteria[ \t]*$')" = "0" ]; then
    labels="$labels,needs:acceptance-criteria"
  fi

  key="$(source_key "$SPECS_DIR/$rel")"

  issue_num="$(find_issue_by_key "$key")"
  if [ -n "$issue_num" ]; then
    echo "reuse $rel  -> #$issue_num  (existing issue carries this source key)"
    RECOVERED+=("$rel -> #$issue_num (reused)")
    MIGRATED+=("$rel -> #$issue_num")
    archive_file "$f" "$rel" "$issue_num" "$raw"
    continue
  fi

  body_file="$WORK_DIR/body.md"
  {
    if [ "$(has_h2 "$f" '^##+[ \t]+status[ \t]*:?[ \t]*$')" = "0" ]; then
      status_word="${issue_state#status:}"
      printf '## Status\n\n%s%s\n\n' \
        "$(printf '%s' "${status_word:0:1}" | tr '[:lower:]' '[:upper:]')" "${status_word:1}"
    fi
    printf '_Migrated from `%s/%s` on %s. This issue is now the canonical spec._\n' \
      "$SPECS_DIR" "$rel" "$TODAY"
    printf '_Source key: `%s`_\n\n' "$key"
    drop_first_h1 "$f"
  } > "$body_file"

  create_ok=1
  if ! issue_out="$(gh issue create \
      --title "$title" \
      --body-file "$body_file" \
      --label "$labels" \
      "${REPO_ARGS[@]+"${REPO_ARGS[@]}"}" 2>&1)"; then
    create_ok=0
  fi
  rm -f "$body_file"

  issue_num=""
  if [ "$create_ok" -eq 1 ]; then
    issue_url="$(printf '%s\n' "$issue_out" | grep -oE 'https://[^ ]+/issues/[0-9]+' | tail -n 1 || true)"
    issue_num="${issue_url##*/}"
  fi

  # Whether creation reported failure or its output could not be parsed, look
  # the issue up by its source key before deciding it failed. This prevents a
  # created-but-unrecorded issue from being duplicated on the next run.
  if [ -z "$issue_num" ]; then
    refresh_issue_keys
    issue_num="$(find_issue_by_key "$key")"
    if [ -n "$issue_num" ]; then
      echo "warn  $rel  (create output unusable; recovered #$issue_num by source key)" >&2
      RECOVERED+=("$rel -> #$issue_num (recovered after create failure)")
    fi
  fi

  if [ -z "$issue_num" ]; then
    if [ "$create_ok" -eq 0 ]; then
      echo "FAIL  $rel  (gh issue create failed: $issue_out)" >&2
      FAILED+=("$rel (gh issue create failed)")
    else
      echo "FAIL  $rel  (issue number could not be resolved; no issue carries the source key)" >&2
      FAILED+=("$rel (issue number unresolved)")
    fi
    continue
  fi

  echo "ok    $rel  -> #$issue_num  [$labels]"
  MIGRATED+=("$rel -> #$issue_num")
  archive_file "$f" "$rel" "$issue_num" "$raw"
done < "$RECORDS"

# ------------------------------------------------------------ link rewrite --

LINK_REPORT=""
if [ "$REWRITE_LINKS" -eq 1 ] && [ -s "$PATH_MAP" ]; then
  if command -v python3 >/dev/null 2>&1; then
    echo
    echo "Rewriting Markdown links to archived paths..."
    LINK_REPORT="$WORK_DIR/links.txt"
    if python3 "$SCRIPT_DIR/rewrite_spec_links.py" \
        --map "$PATH_MAP" --root "$REPO_ROOT" --docs-root "$DOCS_ROOT" \
        --scope "$SPECS_DIR" | tee "$LINK_REPORT"; then
      while IFS= read -r changed; do
        [ -n "$changed" ] && git add "$changed" 2>/dev/null || true
      done < <(sed -n 's/^rewrote  *\([^ ]*\) .*/\1/p' "$LINK_REPORT")
    else
      echo "error: link rewriting failed. Archived files moved but links were not updated." >&2
      echo "Path map: $PATH_MAP (copied to $REPO_ROOT/.pbvg-pathmap.tsv)" >&2
      cp "$PATH_MAP" "$REPO_ROOT/.pbvg-pathmap.tsv"
      exit 1
    fi
  else
    echo "error: python3 not found; cannot rewrite links after moving files." >&2
    echo "Re-run with --no-rewrite-links to accept broken links, or install python3." >&2
    exit 1
  fi
fi

# -------------------------------------------------------- index, log, done --

if [ "${#MIGRATED[@]}" -gt 0 ] || [ "${#ARCHIVED[@]}" -gt 0 ]; then
  write_index
  append_log
  git add "$SPECS_DIR/index.md" "$SPECS_DIR/log.md" 2>/dev/null || true
fi

echo
echo "Summary"
echo "  migrated to issues:       ${#MIGRATED[@]}"
echo "  archived, no issue:       ${#ARCHIVED[@]}"
echo "  reused or recovered:      ${#RECOVERED[@]}"
echo "  skipped, status unclear:  ${#SKIPPED_UNKNOWN[@]}"
echo "  skipped by policy:        ${#SKIPPED_POLICY[@]}"
echo "  skipped, already done:    ${#SKIPPED_ALREADY[@]}"
echo "  failed:                   ${#FAILED[@]}"

if [ "${#SKIPPED_UNKNOWN[@]}" -gt 0 ]; then
  echo
  echo "Left in place because their status could not be determined:"
  printf '  %s\n' "${SKIPPED_UNKNOWN[@]}"
  echo "Classify each with --status-map, or set a status in the file."
fi

if [ -n "$LINK_REPORT" ] && grep -q '^unresolved' "$LINK_REPORT"; then
  echo
  echo "Unresolved links (destinations that do not exist and were not in the move map):"
  grep '^unresolved' "$LINK_REPORT" | sed 's/^/  /'
  echo "These predate the migration. Fix or remove them separately."
fi

if [ "${#FAILED[@]}" -gt 0 ]; then
  echo
  echo "These files failed to migrate and were left in place:"
  printf '  %s\n' "${FAILED[@]}"
  echo "Re-run the script. Any issue that was created carries a source key and will be reused."
  exit 1
fi

echo
echo "Changes are staged, not committed. Review with: git status && git diff --cached"
