#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=vercel-sandbox-common.sh
source "$SCRIPT_DIR/vercel-sandbox-common.sh"

command_name="${1:-}"
case "$command_name" in
  start|finish) ;;
  *)
    echo "Usage: $0 start | finish [sandbox-name]" >&2
    exit 2
    ;;
esac

resolve_provider_values
runtime="$(env_or_state ORCA_VM_RUNTIME runtime node24)"
timeout="$(env_or_state ORCA_VM_TIMEOUT timeout 60m)"
snapshot_expiration="$(env_or_state ORCA_VM_SNAPSHOT_EXPIRATION snapshotExpiration 30d)"
base_name="$(env_or_state ORCA_VM_BASE_NAME baseName kata-symphony-orca-base)"
base_snapshot_id="$(state_value snapshotId)"

if [ "$command_name" = "start" ]; then
  [ -n "$base_snapshot_id" ] || fail "snapshotId is empty; run the base snapshot phase first"
  auth_name="$(sanitize_name "${base_name}-auth-$(date +%s)")"
  create_output="$(vercel sandbox create \
    --name "$auth_name" \
    --snapshot "$base_snapshot_id" \
    --runtime "$runtime" \
    --timeout "$timeout" \
    --snapshot-expiration "$snapshot_expiration" \
    "${vercel_args[@]}" 2>&1)" || {
    status=$?
    printf '%s\n' "$create_output" >&2
    exit "$status"
  }
  printf '%s\n' "$create_output" >&2
  patch="$(node -e 'process.stdout.write(JSON.stringify({ authSandboxName: process.argv[1] }))' "$auth_name")"
  state_merge "$patch"
  cat >&2 <<EOF

Auth sandbox is ready: $auth_name
Run the interactive login from a TTY, then report back when it finishes:
  set -a; . ./.env; set +a
  vercel sandbox exec --interactive --tty "$auth_name" --scope "$scope" --project "$project" --token "\$VERCEL_TOKEN" -- bash -lc 'codex login --device-auth'

Do not run the finish phase until Codex reports a completed login.
EOF
  print_state
  exit 0
fi

auth_name="${2:-$(state_value authSandboxName)}"
[ -n "$auth_name" ] || fail "auth sandbox name is missing; run '$0 start' first"
source_snapshot_id="$base_snapshot_id"
[ -n "$source_snapshot_id" ] || fail "state.snapshotId is empty; run the base snapshot phase first"

cleanup_on_error() {
  local status=$?
  trap - EXIT
  if ((status != 0)); then
    vercel sandbox remove "$auth_name" "${vercel_args[@]}" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup_on_error EXIT

status_output="$(vercel sandbox exec "$auth_name" "${vercel_args[@]}" --timeout 30s -- bash -lc 'codex login status' 2>&1)" || {
  status=$?
  printf '%s\n' "$status_output" >&2
  fail "Codex is not authenticated in $auth_name; the sandbox was removed"
}
printf '%s\n' "$status_output" >&2

snapshot_output="$(vercel sandbox snapshot "$auth_name" --stop --expiration "$snapshot_expiration" "${vercel_args[@]}" 2>&1)" || {
  status=$?
  printf '%s\n' "$snapshot_output" >&2
  exit "$status"
}
printf '%s\n' "$snapshot_output" >&2
new_snapshot_id="$(parse_snapshot_id "$snapshot_output")"
[ -n "$new_snapshot_id" ] || fail "Vercel auth snapshot output did not contain a snapshot id"

vercel sandbox remove "$auth_name" "${vercel_args[@]}" >&2
patch="$(node -e '
  const [snapshotId, sourceSnapshotId] = process.argv.slice(1);
  process.stdout.write(JSON.stringify({ snapshotId, authSourceSnapshotId: sourceSnapshotId, authSandboxName: "" }));
' "$new_snapshot_id" "$source_snapshot_id")"
state_merge "$patch"
trap - EXIT
print_state
