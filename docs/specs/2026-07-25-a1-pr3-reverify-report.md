---
type: Verify Report
title: A1 PR3 Recovery and Agreement Measurement Re-verify Report
status: Incomplete
description: Automated remediation proof for A1 PR3; live GitHub re-verification remains gated on credential revocation and UAT repository cleanup.
tags: [symphony, triage, a1, pr3, verify, uat, github, security]
timestamp: 2026-07-25T19:30:00Z
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
- Runtime/backend: Symphony runtime and GitHub Projects v2 evidence harness
- Intended live target after containment: `gannonh/uat-symphony`, Project
  [#16](https://github.com/users/gannonh/projects/16)

## Remediation results

| Area | Result | Evidence |
| --- | --- | --- |
| Stable recovery identity | Pass (automated) | PID, process group, and OS start token authorize signaling; executable drift is diagnostic |
| Launcher `exec` regression | Pass (automated) | Deterministic FIFO barrier captures the shell, releases `exec sleep`, and verifies termination |
| Recovery state cleanup | Pass (automated) | Prior stage stays interrupted; process/path fields and attempt directory are cleared |
| Flake gate | Pass | 10 consecutive library runs, 233/233 each (2,330 tests total) |
| Full Rust suite | Pass | `cargo test --manifest-path apps/symphony/Cargo.toml` |
| Affected validation | Pass | `pnpm run validate:affected` — lint, typecheck, and tests |
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
