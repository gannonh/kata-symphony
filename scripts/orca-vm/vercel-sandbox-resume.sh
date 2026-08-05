#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=vercel-sandbox-common.sh
source "$SCRIPT_DIR/vercel-sandbox-common.sh"
resolve_provider_values

timeout="$(env_or_state ORCA_VM_TIMEOUT timeout 60m)"
port="$(env_or_state ORCA_VM_PORT port 7331)"
payload="$(cat)"
resource_id="$(node -e '
  const value = JSON.parse(process.argv[1]);
  process.stdout.write(value?.recipeResult?.userData?.resourceId ?? "");
' "$payload")" || fail "lifecycle payload is not valid JSON"
[ -n "$resource_id" ] || fail "lifecycle payload has no recipeResult.userData.resourceId"
stored_url="$(node -e '
  const value = JSON.parse(process.argv[1]);
  process.stdout.write(value?.recipeResult?.userData?.publishedUrl ?? "");
' "$payload")"

cleanup_on_error() {
  local status=$?
  trap - EXIT
  if ((status != 0)); then
    vercel sandbox stop "$resource_id" "${vercel_args[@]}" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup_on_error EXIT

resume_output="$(vercel sandbox run \
  --name "$resource_id" \
  --timeout "$timeout" \
  --publish-port "$port" \
  "${vercel_args[@]}" \
  -- bash -lc 'true' 2>&1)" || {
  status=$?
  printf '%s\n' "$resume_output" >&2
  exit "$status"
}
printf '%s\n' "$resume_output" >&2
public_url="$(parse_public_url "$resume_output")"
[ -n "$public_url" ] || public_url="$stored_url"
[ -n "$public_url" ] || fail "Vercel resume output did not contain a published URL and the create payload had no fallback URL"

recipe_json="$($SCRIPT_DIR/vercel-sandbox-start.sh "$resource_id" "$public_url" resume)"
printf '%s\n' "$recipe_json"
trap - EXIT
