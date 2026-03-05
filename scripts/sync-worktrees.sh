#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
die() { echo "FATAL: $*" >&2; exit 1; }
REPO_DIR="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || true
[ -n "$REPO_DIR" ] || die "could not detect git repo root from $SCRIPT_DIR"

# Discover main and standby worktrees from git metadata.
MAIN_DIR="${MAIN_DIR:-}"
STANDBY_PATHS=()
STANDBY_BRANCHES=()
record_worktree=""
record_branch=""
while IFS= read -r line || [ -n "$line" ]; do
  if [ -z "$line" ]; then
    if [ -n "$record_worktree" ]; then
      if [ "$record_branch" = "main" ] && [ -z "$MAIN_DIR" ]; then
        MAIN_DIR="$record_worktree"
      fi
      case "$record_branch" in
        wt-*-standby)
          STANDBY_PATHS+=("$record_worktree")
          STANDBY_BRANCHES+=("$record_branch")
          ;;
      esac
    fi
    record_worktree=""
    record_branch=""
    continue
  fi

  case "$line" in
    worktree\ *) record_worktree="${line#worktree }" ;;
    branch\ refs/heads/*) record_branch="${line#branch refs/heads/}" ;;
  esac
done < <(git -C "$REPO_DIR" worktree list --porcelain; echo "")
[ -n "$MAIN_DIR" ] || die "could not detect worktree path for branch 'main'"
[ "${#STANDBY_PATHS[@]}" -gt 0 ] || die "no standby worktrees found (expected branches named wt-*-standby)"

errors=0
warn() { echo "ERROR: $*" >&2; errors=$((errors + 1)); }
is_dirty() {
  [ -n "$(git -C "$1" status --porcelain --untracked-files=normal)" ]
}

# -- Step 1: Move main checkout to exactly origin/main --------------------
echo "==> Switching $MAIN_DIR to main"
if ! git -C "$MAIN_DIR" switch main 2>&1; then
  die "switch to main failed"
fi

if is_dirty "$MAIN_DIR"; then
  die "main worktree has local changes/untracked files; clean it before sync"
fi

echo "==> Fetching origin/main"
if ! git -C "$MAIN_DIR" fetch origin main 2>&1; then
  die "fetch failed"
fi

target_sha=$(git -C "$MAIN_DIR" rev-parse origin/main)
echo "==> Resetting main checkout to ${target_sha:0:7}"
if ! git -C "$MAIN_DIR" reset --hard "$target_sha" 2>&1; then
  die "main reset failed"
fi

main_sha=$(git -C "$MAIN_DIR" rev-parse HEAD)
if [ "$main_sha" != "$target_sha" ]; then
  die "main expected ${target_sha:0:7} but got ${main_sha:0:7}"
fi
echo "    main is at ${target_sha:0:7}"

# -- Step 2: Move each standby worktree to exactly target_sha -------------
for i in "${!STANDBY_PATHS[@]}"; do
  wt_path="${STANDBY_PATHS[$i]}"
  branch="${STANDBY_BRANCHES[$i]}"
  wt="$(basename "$wt_path")"

  if [ ! -d "$wt_path" ]; then
    warn "$wt: directory not found"
    continue
  fi

  if is_dirty "$wt_path"; then
    warn "$wt: has local changes/untracked files - skipping"
    continue
  fi

  current=$(git -C "$wt_path" branch --show-current)
  if [ "$current" != "$branch" ]; then
    echo "==> Switching $wt to $branch"
    if ! git -C "$wt_path" switch "$branch" 2>&1; then
      warn "$wt: switch failed"
      continue
    fi
  fi

  echo "==> Resetting $wt to ${target_sha:0:7}"
  if ! git -C "$wt_path" reset --hard "$target_sha" 2>&1; then
    warn "$wt: reset failed"
    continue
  fi

  if ! git -C "$wt_path" branch --set-upstream-to=origin/main "$branch" >/dev/null 2>&1; then
    warn "$wt: failed to set upstream to origin/main"
  fi

  wt_sha=$(git -C "$wt_path" rev-parse HEAD)
  if [ "$wt_sha" != "$target_sha" ]; then
    warn "$wt: expected ${target_sha:0:7} but got ${wt_sha:0:7}"
  else
    echo "    $wt now at ${target_sha:0:7}"
  fi
done

# -- Summary --------------------------------------------------------------
echo ""
if [ "$errors" -gt 0 ]; then
  echo "FAILED: $errors error(s) above. Fix them before starting work."
  exit 1
else
  echo "All worktrees switched to standby and synced to main (${target_sha:0:7})."
fi
