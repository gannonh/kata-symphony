# Change Evidence: harness-ci-shellcheck-fix

## Summary

Suppress `SC1091` in the remaining harness scripts that source `common.sh` via a
dynamic `SCRIPT_DIR` path so GitHub Actions shellcheck accepts the existing
runtime pattern.

## Changed Files

- `docs/generated/change-evidence/2026-03-06-harness-ci-shellcheck-fix.json`
- `docs/generated/change-evidence/2026-03-06-harness-ci-shellcheck-fix.md`
- `scripts/harness/check_decision_links.sh`
- `scripts/harness/check_doc_relevance.sh`
- `scripts/harness/check_evidence_contract.sh`

## Context Loaded

- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Canonical Docs Updated

- None yet. The documented contract did not change for this repair.

## Waivers

- `docs/harness/BUILDING-WITH-HARNESS.md`: The shellcheck suppression aligns CI
  linting with the existing documented harness flow and does not change agent
  workflow or the build-time contract.
- `docs/harness/change-evidence-schema.md`: The evidence schema did not change;
  this repair only updates inline lint metadata on existing harness scripts.
- `docs/harness/context-map.yaml`: Subsystem ownership stayed the same because
  the repair does not alter which docs govern `scripts/harness` changes.

## Verification

- `bash -n scripts/harness/check_decision_links.sh scripts/harness/check_doc_relevance.sh scripts/harness/check_evidence_contract.sh scripts/ci-local.sh` -> pass

## Verification Artifacts

- None yet.

## Impacted Areas

- `documentation`
- `harness`
- `tests`
