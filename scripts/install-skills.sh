#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# Project-specific skills are git-tracked under .agents/skills/.
# Product OS (plan, build, verify, triage, ship) comes from the plan-build-verify plugin.

PLUGIN_DEST="${HOME}/.cursor/plugins/local/plan-build-verify"

if [[ -d "${PLUGIN_DEST}/.cursor-plugin" ]]; then
  echo "install-skills: plan-build-verify plugin already at ${PLUGIN_DEST}"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

git clone --depth 1 https://github.com/gannonh/plan-build-verify.git "${tmp}/plan-build-verify"
mkdir -p "$(dirname "${PLUGIN_DEST}")"
cp -R "${tmp}/plan-build-verify/plugins/cursor" "${PLUGIN_DEST}"

echo "install-skills: installed plan-build-verify plugin to ${PLUGIN_DEST}"
echo "install-skills: enable Allow Local Plugin Imports in Cursor, then enable plan-build-verify"
