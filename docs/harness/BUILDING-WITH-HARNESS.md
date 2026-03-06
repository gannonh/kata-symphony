# Building Symphony With Harness Engineering

Last reviewed: 2026-03-06

## Goal

Use harness engineering while building Symphony, and ship Symphony with the same discipline.

## Track 1: Build-time harness (repo now)

1. Agent map and docs structure in place
2. Explicit quality/reliability/security baseline docs
3. Scripted repository contract checks
4. CI workflow that runs harness checks on every PR
5. Evidence-backed change artifacts for meaningful repo work
6. Doc relevance checks that reject metadata-only compliance

## Evidence Contract

The repo is moving from `docs changed` hygiene toward an evidence-backed contract.
Meaningful changes should leave behind a durable chain from:

`intent -> decision -> implementation -> verification -> canonical docs`

### Required artifact types

1. Decision artifact
   - Design doc or implementation plan describing what changed and why.
2. Change evidence artifact
   - Human-readable markdown plus machine-readable JSON under
     `docs/generated/change-evidence/`.
3. Canonical doc updates
   - Updates to the durable docs that own the changed subsystem, or an explicit
     waiver explaining why no update was required.
4. Verification evidence
   - Concrete commands and outcomes, not just success claims.

### Context routing

`docs/harness/context-map.yaml` maps changed paths to the source-of-truth docs
agents are expected to load. The evidence contract uses this map to determine:

- which docs were required context,
- which durable docs should have been updated,
- when a waiver must be present.

### What this prevents

This contract is intended to make the following fail loudly:

- metadata-only doc updates such as bare `Last reviewed` changes,
- touching unrelated docs to satisfy the docs-sync gate,
- claiming verification without command evidence,
- skipping architecture-sensitive doc updates without a waiver.

### Current build-time gates

The build-time harness currently layers two document-focused checks:

1. `check_docs_sync.sh`
   - Ensures code, spec, or workflow changes are accompanied by a durable doc update.
2. `check_doc_relevance.sh`
   - Ensures canonical doc changes are substantive and aligned with the evidence-declared context.

Together they prevent the repository from accepting either:

- code changes with no durable documentation updates, or
- low-value documentation updates made only to satisfy the docs-sync requirement.

## Agent Guidance

Use the repo-local `symphony-harness-evidence` skill when work touches
architecture-sensitive paths, harness scripts, or other qualifying multi-file
changes. The skill exists to make the evidence contract the default behavior
before hooks fail, not after.

## Track 2: Runtime harness (product behavior)

1. Enforce workflow/config validation before dispatch
2. Enforce isolation and safety in workspace execution
3. Enforce retry/reconciliation correctness and recovery behavior
4. Expose operator monitoring surfaces (TUI MVP, then web dashboard)

## Dogfooding Loop

1. Build Symphony in this repo using these harness docs and checks.
2. Use Symphony workflows to execute Symphony backlog work.
3. Feed incidents and friction back into:
   - `AGENTS.md`
   - `docs/harness/context-map.yaml`
   - runtime defaults in `WORKFLOW.md`
   - test matrix and operational docs
