---
type: Verify Report
title: A1 PR3 Recovery and Agreement Measurement Re-verify Report
status: Incomplete
description: Automated remediation proof for A1 PR3; live GitHub re-verification remains gated on credential revocation and UAT repository cleanup.
tags: [symphony, triage, a1, pr3, verify, uat, github, security]
timestamp: 2026-07-25T22:35:00Z
---

# A1 PR3 Recovery and Agreement Measurement — Re-verify Report

## Status

**Incomplete** — the post-`exec` recovery defect, flaky test, and evidence
cleanup defect are fixed and pass automated validation. Live GitHub
re-verification was intentionally not run because credentials committed by the
UAT repository have not yet been confirmed revoked. A1 PR3 remains blocked.

The original [rejected Verify report](2026-07-25-a1-pr3-verify-report.md) is
preserved as the historical result.

## Code under test

- Runtime recovery remediation: `66f3da1`
- Evidence and cleanup remediation: `424bd4b`
- PR review remediation: `ea6bd9d9`, formatting follow-up `6833395a`
- Runtime/backend: Symphony runtime and GitHub Projects v2 evidence harness
- Intended live target after containment: `gannonh/uat-symphony`, Project
  [#16](https://github.com/users/gannonh/projects/16)

## Remediation results

| Area | Result | Evidence |
| --- | --- | --- |
| Stable recovery identity | Pass (automated) | PID, process group, and OS start token authorize signaling; executable drift is diagnostic |
| Launcher `exec` regression | Pass (automated) | Deterministic FIFO barrier captures the shell, releases `exec sleep`, and verifies termination |
| Recovery state cleanup | Implemented; CI pass | Process/path fields and attempt directory clear only after termination or confirmed absence; uncertain/live outcomes retain durable recovery state (retention branches are not covered by a dedicated assertion) |
| Codex process isolation | Implemented; CI pass | Unix Codex children are configured as process-group leaders, matching Pi recovery isolation (no dedicated Codex group-leader assertion) |
| Cleanup path authorization | Pass (automated) | Recursive cleanup requires exact stage-run directory under configured `workspace.root` |
| Correction candidate semantics | Pass (automated) | Latest applied publication per run is measured; randomized bounded batches replace the fixed newest-only selection |
| Flake gate (pre-review remediation) | Pass | 10 consecutive library runs at `424bd4b`, 233/233 each (2,330 tests total); current HEAD contains 234 library tests |
| Full Rust suite | Pass | `cargo test --manifest-path apps/symphony/Cargo.toml` |
| Affected validation | Pass | `pnpm run validate:affected` — lint, typecheck, and tests |
| PR #599 required checks | Pass | CI run [30177679884](https://github.com/gannonh/kata-symphony/actions/runs/30177679884): validate, coverage, backend validation, smoke, distributions, and gate |
| Evidence config | Pass (automated/dry-run) | Sanitized effective repository/project coordinates are stored without token values |
| Cleanup target safety | Pass (automated/dry-run) | Stored config and legacy URL resolution pass; conflicting repositories fail before provider work |
| UAT repository current-tree cleanup | Draft | [gannonh/uat-symphony#18](https://github.com/gannonh/uat-symphony/pull/18) removes 40,800 generated lines, credentials, logs, and unintended gitlinks |
| Provider credential revocation | **Blocked** | Account-owner confirmation is unavailable in this environment |
| Live restart recovery | Not run | Deliberately gated on credential revocation and cleanup merge |
| Live evidence cleanup | Not run | Deliberately gated on credential revocation and cleanup merge |

## Credential containment finding

Seven tracked Pi authentication files exposed credentials for OpenAI Codex,
Cursor, NVIDIA, Z.ai, Anthropic, Hyper, OpenRouter, Kimi Coding, and xAI. No
secret values were printed or copied into this report. The current-tree copies
and generated runtime state are removed by the UAT cleanup PR, with ignore rules
to prevent recurrence.

The repository intentionally retains history. Every represented credential must
therefore be revoked or rotated by its provider account owner before live UAT.
Current-tree deletion alone does not make historical values inert.

## Automated commands

```bash
cargo test --manifest-path apps/symphony/Cargo.toml process_identity
cargo test --manifest-path apps/symphony/Cargo.toml \
  recovery_terminates_orphaned_child_and_removes_attempt_directory
node --test \
  .agents/skills/uat-evidence/scripts/symphony-runtime-config.test.mjs

# Ten consecutive successful runs
cargo test --manifest-path apps/symphony/Cargo.toml --lib

cargo test --manifest-path apps/symphony/Cargo.toml
pnpm run validate:affected
```

The UAT evidence runner also completed a GitHub dry run for
`gannonh/uat-symphony` Project #16. A synthetic cleanup dry run used stored
coordinates without overrides, and a mismatched repository dry run failed
before any provider call. No live issue or project state was created.

## PR thread and CI closeout

PR [#599](https://github.com/gannonh/kata-symphony/pull/599) has a clean merge
state, all review threads resolved, and all required checks passing at
`6833395a`. Review remediation added Codex process-group isolation, durable
retention for unresolved orphan recovery, cleanup-root authorization, and
latest-per-run randomized correction candidates. The focused `triage::` suite
passed 89/89 tests before the final CI run.

The narrow spawn-to-coordinator persistence window remains a follow-up: a hard
process exit immediately after child spawn can occur before SQLite records the
identity. Closing it requires runner-side durable recording. This does not alter
the existing credential-containment and live-UAT acceptance gate below.

## Remaining acceptance gate

Re-verification can become **Accepted** only after:

1. Account owners confirm revocation or rotation for every provider listed
   above and verify replacement credentials outside Git.
2. UAT cleanup PR #18 is merged and a fresh clone passes current-tree secret and
   gitlink checks.
3. The bundled Symphony/GitHub evidence run passes against Project #16 and its
   cleanup succeeds from the evidence file without repeated overrides.
4. A hard-restart fixture proves the recorded launcher changes executable,
   the orphan is live before restart, recovery terminates the old process group,
   the attempt is interrupted and cleaned, and its retry completes.
5. All created fixtures are closed and no unrelated implementation dispatch
   occurs.

Until those gates pass, the roadmap remains blocked and this report must not be
promoted to Accepted.
