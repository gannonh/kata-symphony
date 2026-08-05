#!/usr/bin/env bash
set -euo pipefail

ORCA_VM_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ORCA_VM_STATE_FILE="${ORCA_VM_STATE_FILE:-$ORCA_VM_DIR/vercel-sandbox-state.json}"
ORCA_REPO_ROOT="$(cd -- "$ORCA_VM_DIR/../.." && pwd)"
ORCA_REPO_ENV_FILE="$ORCA_REPO_ROOT/.env"

dotenv_value() {
  local name="$1"
  node --env-file="$ORCA_REPO_ENV_FILE" -e 'process.stdout.write(process.env[process.argv[1]] ?? "")' "$name"
}

load_repo_credentials() {
  [ -f "$ORCA_REPO_ENV_FILE" ] || return 0

  if [ -z "${VERCEL_TOKEN:-${VERCEL_AUTH_TOKEN:-}}" ]; then
    VERCEL_TOKEN="$(dotenv_value VERCEL_TOKEN)"
    if [ -z "$VERCEL_TOKEN" ]; then
      VERCEL_AUTH_TOKEN="$(dotenv_value VERCEL_AUTH_TOKEN)"
      export VERCEL_AUTH_TOKEN
    else
      export VERCEL_TOKEN
    fi
  fi

  if [ -z "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]; then
    GH_TOKEN="$(dotenv_value GH_TOKEN)"
    if [ -z "$GH_TOKEN" ]; then
      GITHUB_TOKEN="$(dotenv_value GITHUB_TOKEN)"
      export GITHUB_TOKEN
    else
      export GH_TOKEN
    fi
  fi
}

load_repo_credentials

fail() {
  echo "vercel-sandbox: $*" >&2
  exit 1
}

state_value() {
  local key="$1"
  [ -f "$ORCA_VM_STATE_FILE" ] || return 0
  node -e '
    const fs = require("node:fs");
    const [file, key] = process.argv.slice(1);
    const data = JSON.parse(fs.readFileSync(file, "utf8"));
    const value = data[key];
    if (value !== undefined && value !== null && value !== "") process.stdout.write(String(value));
  ' "$ORCA_VM_STATE_FILE" "$key"
}

env_or_state() {
  local env_name="$1"
  local key="$2"
  local fallback="${3:-}"
  local value="${!env_name:-}"
  if [ -z "$value" ]; then
    value="$(state_value "$key")"
  fi
  if [ -z "$value" ]; then
    value="$fallback"
  fi
  printf '%s' "$value"
}

resolve_provider_values() {
  scope="$(env_or_state VERCEL_TEAM_ID scope)"
  project="$(env_or_state VERCEL_PROJECT_ID project)"
  vercel_token="${VERCEL_TOKEN:-${VERCEL_AUTH_TOKEN:-}}"
  [ -n "$vercel_token" ] || fail "VERCEL_TOKEN must be set in the ignored repository .env file or exported; it is never written to state"
  vercel_args=(--token "$vercel_token")
  [ -n "$scope" ] && vercel_args+=(--scope "$scope")
  [ -n "$project" ] && vercel_args+=(--project "$project")
}

resolve_git_token() {
  gh_token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
  if [ -z "$gh_token" ] && command -v gh >/dev/null 2>&1; then
    gh_token="$(gh auth token 2>/dev/null || true)"
  fi
  [ -n "$gh_token" ] || fail "GH_TOKEN (or GITHUB_TOKEN / gh auth token) must be set in the ignored repository .env file or exported to clone the repository"
}

state_merge() {
  local patch_json="$1"
  node -e '
    const fs = require("node:fs");
    const [file, patchText] = process.argv.slice(1);
    const previous = JSON.parse(fs.readFileSync(file, "utf8"));
    const next = { ...previous, ...JSON.parse(patchText) };
    fs.writeFileSync(file, `${JSON.stringify(next, null, 2)}\n`);
  ' "$ORCA_VM_STATE_FILE" "$patch_json"
}

sanitize_name() {
  local value
  value="$(printf '%s' "$1" | tr -cs '[:alnum:]-' '-' | sed -E 's/^-+//; s/-+$//')"
  [ -n "$value" ] || value="orca-sandbox"
  printf '%s' "${value:0:63}"
}

parse_snapshot_id() {
  printf '%s\n' "$1" | grep -Eo 'snap_[A-Za-z0-9]+' | tail -1 || true
}

parse_public_url() {
  printf '%s\n' "$1" | grep -Eo 'https://[^[:space:]]+\.vercel\.run' | head -1 || true
}

recipe_json_line() {
  node -e '
    const input = require("node:fs").readFileSync(0, "utf8");
    for (const line of input.split(/\r?\n/).reverse()) {
      try {
        const value = JSON.parse(line);
        if (value && typeof value === "object" && typeof value.pairingCode === "string" && typeof value.projectRoot === "string") {
          process.stdout.write(JSON.stringify(value));
          process.exit(0);
        }
      } catch {}
    }
    process.exit(1);
  '
}

print_state() {
  cat "$ORCA_VM_STATE_FILE"
}
