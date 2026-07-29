---
type: ADR
title: A3 implementation durability, bundles, and preview publication
status: Accepted
description: Durable A3 stage records, content-addressed Git bundles beside SQLite, credential-isolated local execution, and preview-only publication before draft-PR handoff.
tags: [symphony, software-factory, a3, implementation, durability]
timestamp: 2026-07-29T16:00:00Z
---

# ADR-0004 A3 implementation durability, bundles, and preview publication

## Status

Accepted — A3 PR1; extended by A3 PR2 (draft-PR artifacts and progressive publication)

## Context

A3 turns a pinned A2 approved specification into a validated change bundle and (in PR2) a linked draft GitHub PR. Code-sized Git bundles do not belong in SQLite. Workers must not receive forge mutation credentials. Preview publication must be idempotent without advancing tracker state. Automatic publication must reconcile expected branch/PR projections without force-push and must not advance tracker state before a verified draft-PR artifact exists.

## Decision

1. **Stage-scoped durability.** Implementation attempts reuse `stage_runs` with `stage='implementation'`. Additive tables record attempt inputs, implement/repair turns, validation cycles, implementation-manifest artifacts, bundle metadata, run state, and publication intents (`004_implementation_stage.sql`). PR2 adds immutable draft-PR artifacts (`005_implementation_draft_pr.sql`).
2. **Content-addressed blobs.** Bundle bytes live at `{storage.path}.artifacts/sha256/<aa>/<digest>` with atomic temp→rename writes. SQLite stores digest, size, and Git metadata only. HTTP never serves paths or bytes. Digests are re-verified immediately before branch push.
3. **Credential-isolated execution.** Local attempts clone from a verified base bundle into a disposable workspace with isolated HOME, disabled push URLs, and an allowlisted environment (model auth only). Docker env builders omit GitHub, Linear, helper, and SSH credentials; repository enter/leave is via verified bundles. Only the trusted publisher pushes branches and creates PRs.
4. **Preview-only publisher (PR1).** Owned issue comments use `<!-- symphony:implementation:{intent_id} -->` with create-before-record recovery. No branch push, draft PR, label removal, or Projects v2 state change until PR2.
5. **Automatic publication (PR2).** Progressive `record_implementation_publication_step` updates completed steps without forcing `applied` until the final comment. Branch expected-projection never force-pushes. Draft PRs use `<!-- symphony:implementation-pr:{intent_id} -->`, recover create-before-record by head/base list, and reject foreign/closed/ready/drifted candidates. Tracker label removal and `completion_route.state` run only after the draft-PR artifact is stored.
6. **Dispatch ownership.** A3 claims before legacy candidate fetch. Durable guards cover nonterminal attempts/publications and retained A3 run state so approved A2 work cannot race the legacy worker.

## Consequences

- A1/A2 suites remain green; migrations are additive.
- Disk growth from retained bundles is operator-visible; GC is deferred.
- Live draft-PR / Agent Review UAT remains operator residual after PR2 automation.
- Live Docker container orchestration beyond the env/bundle contract is residual where no daemon is available.
