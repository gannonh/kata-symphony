#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=vercel-sandbox-common.sh
source "$SCRIPT_DIR/vercel-sandbox-common.sh"
resolve_provider_values

payload="$(cat)"
resource_id="$(node -e '
  const value = JSON.parse(process.argv[1]);
  process.stdout.write(value?.recipeResult?.userData?.resourceId ?? "");
' "$payload")" || fail "lifecycle payload is not valid JSON"
[ -n "$resource_id" ] || fail "lifecycle payload has no recipeResult.userData.resourceId"

vercel sandbox remove "$resource_id" "${vercel_args[@]}" >&2
