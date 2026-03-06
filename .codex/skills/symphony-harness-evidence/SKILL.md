---
name: symphony-harness-evidence
description: Apply Symphony's evidence-backed build-time harness when changes touch architecture-sensitive paths, harness scripts, workflow policy, or other qualifying multi-file code changes.
---

# Symphony Harness Evidence

## Purpose

Keep qualifying repo changes traceable from context selection through verification.

## Use This For

- Changes under `scripts/harness/**`
- Changes under `src/config/**`, `src/execution/**`, or `src/orchestrator/**`
- Changes to `WORKFLOW.md`, `SPEC.md`, `ARCHITECTURE.md`, `SECURITY.md`, or `RELIABILITY.md`
- Multi-file code changes that should leave behind evidence artifacts

## Do Not Use This For

- Trivial typo fixes with no behavior change
- Pure ticket lifecycle or PR automation
- Generic planning without an implementation delta

## Workflow

1. Read `docs/harness/context-map.yaml` to determine required context and owning docs.
2. Create or update the matching change-evidence markdown and JSON under `docs/generated/change-evidence/`.
3. Record:
   - changed files
   - context loaded
   - decision artifacts
   - canonical docs updated or waivers
   - verification commands and verification artifacts
4. Update the owning docs named by the context map, or add explicit waivers with rationale.
5. Run the relevant harness checks:
   - `bash scripts/harness/check_doc_relevance.sh`
   - `bash scripts/harness/check_evidence_contract.sh`
   - `bash scripts/harness/check_decision_links.sh`
6. Run broader verification as required by the task (`bash scripts/ci-local.sh` or `make check`).

## Output

- Updated change-evidence JSON and markdown
- Durable doc updates or waivers
- Verification artifact links
- Residual risk notes when relevant
