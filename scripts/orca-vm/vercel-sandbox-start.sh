#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=vercel-sandbox-common.sh
source "$SCRIPT_DIR/vercel-sandbox-common.sh"

resource_name="${1:-}"
public_url="${2:-}"
mode="${3:-create}"
[ -n "$resource_name" ] || fail "sandbox resource name is required"
[ -n "$public_url" ] || fail "published Vercel URL is required"
case "$mode" in
  create|resume) ;;
  *) fail "unsupported start mode: $mode" ;;
esac

resolve_provider_values
resolve_git_token

snapshot_id="$(state_value snapshotId)"
[ -n "$snapshot_id" ] || fail "authenticated snapshotId is missing; run the base and auth phases first"
runtime="$(env_or_state ORCA_VM_RUNTIME runtime node24)"
timeout="$(env_or_state ORCA_VM_TIMEOUT timeout 60m)"
port="$(env_or_state ORCA_VM_PORT port 7331)"
repo_url="$(env_or_state ORCA_REPO_URL repoUrl https://github.com/gannonh/kata-symphony.git)"
repo_ref="$(env_or_state ORCA_REPO_REF repoRef main)"
project_root="$(env_or_state ORCA_PROJECT_ROOT projectRoot /vercel/sandbox/kata-symphony)"

pairing_ws="${public_url/https:\/\//wss://}"
case "$pairing_ws" in
  wss://*) ;;
  *) fail "published URL did not convert to a wss:// address" ;;
esac

if [ "$mode" = "create" ]; then
  IFS= read -r -d '' remote_sync <<'REMOTE' || true
set -euo pipefail
export PATH="/usr/local/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

if [ ! -d "$ORCA_PROJECT_ROOT/.git" ]; then
  mkdir -p "$(dirname "$ORCA_PROJECT_ROOT")"
  if [ -n "${GH_TOKEN:-}" ]; then
    printf '%s\n' '#!/usr/bin/env bash' 'case "$1" in *Username*) printf "%s\\n" x-access-token ;; *Password*) printf "%s\\n" "$GH_TOKEN" ;; esac' >/tmp/orca-askpass.sh
    chmod 0700 /tmp/orca-askpass.sh
    trap 'rm -f /tmp/orca-askpass.sh' EXIT
    export GIT_ASKPASS=/tmp/orca-askpass.sh GIT_TERMINAL_PROMPT=0
  fi
  git clone "$ORCA_REPO_URL" "$ORCA_PROJECT_ROOT"
fi
cd "$ORCA_PROJECT_ROOT"
if [ -n "${GH_TOKEN:-}" ]; then
  printf '%s\n' '#!/usr/bin/env bash' 'case "$1" in *Username*) printf "%s\\n" x-access-token ;; *Password*) printf "%s\\n" "$GH_TOKEN" ;; esac' >/tmp/orca-askpass.sh
  chmod 0700 /tmp/orca-askpass.sh
  trap 'rm -f /tmp/orca-askpass.sh' EXIT
  export GIT_ASKPASS=/tmp/orca-askpass.sh GIT_TERMINAL_PROMPT=0
fi
git fetch origin "$ORCA_REPO_REF"
git checkout -B "$ORCA_REPO_REF" "origin/$ORCA_REPO_REF"
rm -f /tmp/orca-askpass.sh
current_commit="$(git rev-parse HEAD)"
if [ ! -f .orca-built ] || [ "$(cat .orca-built)" != "$current_commit" ]; then
  pnpm install --frozen-lockfile
  pnpm run build
  printf '%s' "$current_commit" >.orca-built
fi
printf '%s\n' '__ORCA_SYNC_OK__'
REMOTE
  sync_output="$(vercel sandbox exec "$resource_name" "${vercel_args[@]}" --timeout 50m \
    --env "GH_TOKEN=$gh_token" \
    --env "ORCA_PROJECT_ROOT=$project_root" \
    --env "ORCA_REPO_URL=$repo_url" \
    --env "ORCA_REPO_REF=$repo_ref" \
    -- bash -lc "$remote_sync" 2>&1)" || {
    status=$?
    printf '%s\n' "$sync_output" >&2
    exit "$status"
  }
  printf '%s\n' "$sync_output" >&2
  grep -Fx '__ORCA_SYNC_OK__' <<<"$sync_output" >/dev/null || fail "workspace sync did not reach its success marker"
fi

IFS= read -r -d '' remote_serve <<'REMOTE' || true
set -euo pipefail
export PATH="/usr/local/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
cd "$ORCA_PROJECT_ROOT"
rm -f /tmp/orca-recipe.json /tmp/orca-serve.log /tmp/orca-serve.pid
nohup env LIBGL_ALWAYS_SOFTWARE=1 orca serve \
  --port "$ORCA_PORT" \
  --project-root "$ORCA_PROJECT_ROOT" \
  --pairing-address "$ORCA_PAIRING_ADDRESS" \
  --recipe-json \
  >/tmp/orca-recipe.json 2>/tmp/orca-serve.log </dev/null &
pid=$!
printf '%s' "$pid" >/tmp/orca-serve.pid
for _ in $(seq 1 120); do
  if node -e 'JSON.parse(require("node:fs").readFileSync("/tmp/orca-recipe.json", "utf8"))' >/dev/null 2>&1; then
    cat /tmp/orca-recipe.json
    exit 0
  fi
  sleep 0.25
done
cat /tmp/orca-serve.log >&2
echo 'orca serve did not emit recipe JSON before the startup deadline' >&2
exit 1
REMOTE
recipe_output="$(vercel sandbox exec "$resource_name" "${vercel_args[@]}" --timeout 60s \
  --env "ORCA_PORT=$port" \
  --env "ORCA_PROJECT_ROOT=$project_root" \
  --env "ORCA_PAIRING_ADDRESS=$pairing_ws" \
  -- bash -lc "$remote_serve" 2>&1)" || {
  status=$?
  printf '%s\n' "$recipe_output" | sed -E 's/(pairingCode["'"'"' :]+)[^,"'"'"' }]*/\1[redacted]/g' >&2
  exit "$status"
}
recipe_json="$(recipe_json_line <<<"$recipe_output")" || {
  printf '%s\n' "$recipe_output" | sed -E 's/(pairingCode["'"'"' :]+)[^,"'"'"' }]*/\1[redacted]/g' >&2
  fail "orca serve did not return a valid recipe JSON object"
}

node -e '
  const [recipeText, resourceId, snapshotId, publishedUrl] = process.argv.slice(1);
  const recipe = JSON.parse(recipeText);
  if (recipe.schemaVersion !== undefined && recipe.schemaVersion !== 1) throw new Error("unsupported recipe schema");
  if (typeof recipe.pairingCode !== "string" || typeof recipe.projectRoot !== "string") throw new Error("recipe JSON is missing pairingCode/projectRoot");
  process.stdout.write(JSON.stringify({
    ...recipe,
    schemaVersion: 1,
    userData: {
      ...(recipe.userData || {}),
      provider: "vercel-sandbox",
      resourceId,
      snapshotId,
      publishedUrl
    }
  }));
' "$recipe_json" "$resource_name" "$snapshot_id" "$public_url"
