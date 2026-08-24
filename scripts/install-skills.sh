#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
npx skills add gannonh/skills --skill plan-build-verify --skill address-pr-comments -y
