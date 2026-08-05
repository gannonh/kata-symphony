#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=vercel-sandbox-common.sh
source "$SCRIPT_DIR/vercel-sandbox-common.sh"

resolve_provider_values

snapshot_id="$(state_value snapshotId)"
[ -n "$snapshot_id" ] || fail "authenticated snapshotId is missing; run the base and auth phases first"
runtime="$(env_or_state ORCA_VM_RUNTIME runtime node24)"
timeout="$(env_or_state ORCA_VM_TIMEOUT timeout 60m)"
vcpus="$(env_or_state ORCA_VM_VCPUS vcpus 4)"
port="$(env_or_state ORCA_VM_PORT port 7331)"
snapshot_expiration="$(env_or_state ORCA_VM_SNAPSHOT_EXPIRATION snapshotExpiration 30d)"
keep_last_snapshots="$(env_or_state ORCA_VM_KEEP_LAST_SNAPSHOTS keepLastSnapshots 2)"
recipe_id="${ORCA_VM_RECIPE_ID:-${ORCA_VM_RECIPE:-vercel-sandbox}}"
instance_id="${ORCA_VM_INSTANCE_ID:-$(date +%s)}"
name="$(sanitize_name "orca-${recipe_id}-${instance_id}")"

cleanup_on_error() {
  local status=$?
  trap - EXIT
  if ((status != 0)); then
    vercel sandbox remove "$name" "${vercel_args[@]}" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup_on_error EXIT

create_output="$(vercel sandbox create \
  --name "$name" \
  --snapshot "$snapshot_id" \
  --runtime "$runtime" \
  --timeout "$timeout" \
  --vcpus "$vcpus" \
  --publish-port "$port" \
  --snapshot-expiration "$snapshot_expiration" \
  --keep-last-snapshots "$keep_last_snapshots" \
  "${vercel_args[@]}" 2>&1)" || {
  status=$?
  printf '%s\n' "$create_output" >&2
  exit "$status"
}
printf '%s\n' "$create_output" >&2
public_url="$(parse_public_url "$create_output")"
[ -n "$public_url" ] || fail "Vercel create output did not contain a published https://*.vercel.run URL"

recipe_json="$($SCRIPT_DIR/vercel-sandbox-start.sh "$name" "$public_url" create)"
printf '%s\n' "$recipe_json"
trap - EXIT
