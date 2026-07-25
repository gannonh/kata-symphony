---
type: Verify Report
title: A1 PR2 Automatic Route Publication Verify Report
status: Accepted
description: Verify results for A1 PR2 automatic route publication against acceptance criteria 9–13 and related auto/handoff criteria.
tags: [symphony, triage, a1, pr2, verify, uat]
timestamp: 2026-07-24T17:16:00-07:00
---

# A1 PR2 Automatic Route Publication — Verify Report

## Status

**Accepted** (maintainer sign-off 2026-07-24) — automated suite + live GitHub UAT on the dedicated UAT project both Pass. Merge/PR still pending.

## Spec / Build

- Spec: [`2026-07-16-a1-github-issue-triage-design.md`](2026-07-16-a1-github-issue-triage-design.md) (PR2 slice)
- Build: [`2026-07-24-a1-pr2-build-report.md`](2026-07-24-a1-pr2-build-report.md)

## Environments

| Layer | Target |
| --- | --- |
| Automated | `cargo test --lib` / `triage::` in this worktree |
| Live UAT | `/Volumes/EVO/dev/uat-runs/kata-symphony` → `gannonh/uat-symphony` Project [#16](https://github.com/users/gannonh/projects/16) |

## Results

| AC | Result | Notes |
| --- | --- | --- |
| 9 | Pass | Preview→automatic promotion on #8/#9/#10 with **no second stage attempt** |
| 10 | Pass | Live `completed_steps_json` shows full 7-step chain |
| 11 | Pass | Automated publisher crash/conflict tests |
| 12 | Pass | Guard tests + #10 only dispatched after publication applied |
| 13 | Pass | Live needs-info comments include clarification questions |
| 14 (auto) | Pass | Automated human-conflict test |
| 15 (auto) | Pass | Live `triage_route_applied` events |
| 17 (auto) | Pass | 61 triage / 206 lib tests |
| 18 (promo/handoff) | Pass | #10 implement handoff completed by coding worker; #12 cold automatic |

## Live fixtures

- [#7](https://github.com/gannonh/uat-symphony/issues/7) — off-project diagnostic
- [#8](https://github.com/gannonh/uat-symphony/issues/8), [#9](https://github.com/gannonh/uat-symphony/issues/9) — preview→auto `needs_information`
- [#10](https://github.com/gannonh/uat-symphony/issues/10) — preview→auto `implement` + handoff
- [#12](https://github.com/gannonh/uat-symphony/issues/12) — cold automatic publication

Evidence bundle (gitignored): `uat-evidence/cli-a1-pr2-20260724/`.

## Caveats

- UAT workflow temporarily used `openrouter/anthropic/claude-sonnet-4` because `openai-codex` OAuth refresh failed and Anthropic OAuth reported out of extra usage.
- Working tree still uncommitted at Verify time.
- No dedicated automated unit test solely for promotion hash matching (covered live + coordinator code path).

## Recommendation

**Accepted.** Next: commit the working tree and open the PR2 merge PR when ready.
