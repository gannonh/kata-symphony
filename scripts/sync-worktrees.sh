#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -z "${MAIN_DIR:-}" ]; then
  MAIN_DIR="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || true
  if [ -z "$MAIN_DIR" ]; then
    echo "FATAL: could not detect git repo root from $SCRIPT_DIR" >&2
    exit 1
  fi
fi
WT_DIR="${WT_DIR:-$(dirname "$MAIN_DIR")/$(basename "$MAIN_DIR").worktrees}"

# Discover all worktree directories automatically (bash 3.2 compatible)
WORKTREES=()
while IFS= read -r dir; do
  WORKTREES+=("$(basename "$dir")")
done < <(find "$WT_DIR" -mindepth 1 -maxdepth 1 -type d -name 'wt-*' | sort)

errors=0

die() { echo "FATAL: $*" >&2; exit 1; }
warn() { echo "ERROR: $*" >&2; errors=$((errors + 1)); }

# -- Step 1: Move main checkout to exactly origin/main --------------------
echo "==> Switching $MAIN_DIR to main"
if ! git -C "$MAIN_DIR" switch main 2>&1; then
  die "switch to main failed"
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
for wt in "${WORKTREES[@]}"; do
  wt_path="$WT_DIR/$wt"
  branch="${wt}-standby"

  if [ ! -d "$wt_path" ]; then
    warn "$wt: directory not found"
    continue
  fi

  if ! git -C "$wt_path" diff --quiet || ! git -C "$wt_path" diff --cached --quiet; then
    warn "$wt: has uncommitted changes - skipping"
    continue
  fi

  if ! git -C "$wt_path" show-ref --verify --quiet "refs/heads/$branch"; then
    warn "$wt: branch '$branch' not found - skipping"
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
