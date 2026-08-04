---
type: Spec
title: A3 Implementation Stage
description: Design for turning a pinned A2 approved specification into a validated change bundle and linked draft GitHub pull request.
tags: [symphony, software-factory, implementation-stage, github, docker]
timestamp: 2026-07-27T16:30:13Z
status: Completed
source_status: completed
migrated: false
archived_at: 2026-08-04T19:34:07Z
---

> **Completed before migration** (source status: completed). Retained as history. Not tracked in GitHub Issues.

# A3 Implementation Stage

## Status

Complete — PR1 implementation/validation preview **shipped** in [#606](https://github.com/gannonh/kata-symphony/pull/606) ([build](2026-07-29-a3-pr1-build-report.md), [verify](2026-07-29-a3-pr1-verify-report.md)). **PR2** deterministic draft-PR publication and Agent Review handoff **shipped** in [#607](https://github.com/gannonh/kata-symphony/pull/607) (`d456c051`) ([build](2026-07-29-a3-pr2-build-report.md), [verify](2026-07-29-a3-pr2-verify-report.md) Verified with residuals). Direct GitHub draft-PR, Agent Review preview, and Ratatui TUI UAT passed on Project #16 using issue [#45](https://github.com/gannonh/uat-symphony/issues/45) and draft PR [#46](https://github.com/gannonh/uat-symphony/pull/46). Restart-during-publication, full cleanup, and Docker evidence remain residuals. **Next:** [A4 Agent Review Stage](2026-07-30-a4-agent-review-stage-design.md).

## Goal

Deliver the third software factory stage as a complete user-facing workflow:

`pinned A2 approved spec → isolated implementation → deterministic validation and repair → durable change bundle → linked draft GitHub PR → Agent Review`

A3 implements the PRD's A3 slice. It consumes the exact approved A2 artifact version, gives an implementation worker only the issue and approved specification, runs repository-owned validation outside the agent's success claim, and opens a draft pull request that links the issue, specification, factory run, implementation evidence, and validation result.

A3 replaces prompt convention with a typed stage only for spec-driven work. Direct A1 `implement` routes continue through Symphony's existing implementation worker. This preserves current behavior while A3 proves the safer stage contract.

## Source of truth

- [Symphony Software Factory Platform PRD, A3](/specs/archive/symphony-software-factory-platform-prd.md)
- [A2 Spec Stage](/specs/archive/2026-07-18-a2-spec-stage-design.md)
- [ADR-0003 A2 spec-stage artifacts and human gates](/adrs/0003-a2-spec-stage-artifacts-and-gates.md)
- [A2 UAT Verify Report](/specs/archive/2026-07-26-a2-uat-verify-report.md)
- [A1 GitHub Issue Triage](/specs/archive/2026-07-16-a1-github-issue-triage-design.md)
- [ADR-0001 A1 triage durability and isolation](/adrs/0001-a1-triage-durability-and-isolation.md)
- [ADR-0002 Triage process recovery identity](/adrs/0002-triage-process-recovery-identity.md)
- [Warp spec-driven development article](https://www.warp.dev/blog/how-to-build-a-cloud-software-factory-add-spec-driven-development-skills)
- [GitHub REST pull request API](https://docs.github.com/en/rest/pulls/pulls)
- `apps/symphony/src/orchestrator.rs`
- `apps/symphony/src/workspace.rs`
- `apps/symphony/src/docker.rs`
- `apps/symphony/src/github/client.rs`
- `apps/symphony/src/triage/`

## Product decisions

- A3 admits only factory runs with a terminal A2 approval publication and a pinned `approved_artifact_id` / `approved_version`. A tracker label alone is insufficient.
- Direct A1 `implement` routes and manually implementation-ready issues remain on the existing worker path.
- The A3 coordinator claims approved work before the legacy scheduler. A durable dispatch guard prevents both paths from owning the same issue.
- The worker edits and commits in an isolated repository but receives no forge, tracker-helper, SSH, or Git push credentials.
- Symphony materializes the exact approved specification as a committed repository file. The default path is `specs/{issue_identifier}/APPROVED-v{version}.md`; repositories may configure a validated path template.
- The worker emits a typed implementation manifest. A clean committed tree, a byte-identical approved-spec file, a non-spec change, and a complete acceptance-criterion mapping are required.
- Repository validation is configured as a non-empty ordered command list. Every command is blocking.
- A validation failure enters a bounded repair loop in the same attempt workspace. Each repair is a fresh worker invocation receiving the approved spec and structured validation failure evidence.
- A3 supports credential-isolated local and Docker execution. SSH is deferred.
- Symphony exports the validated commit as a content-addressed Git bundle, verifies it on the host, and stores it durably before preview or publication.
- Symphony alone pushes the remote branch and creates or reconciles the draft pull request.
- A successful publication removes the recorded implementation label and moves the Projects v2 item to configured `Agent Review`.
- A3 ships in two vertical pull requests: implementation/validation preview, then deterministic draft-PR publication and handoff.
- A3 does not perform agent code review, acceptance verification, merge, or deployment. Those remain A4, A5, and A6.

## User stories

### Maintainer

As a maintainer, I can approve a specification and receive a draft PR implementing that exact version, with the approved spec committed in the branch and validation evidence in the PR description.

### Factory operator

As a factory operator, I can inspect implementation attempts, repair turns, validation commands, commit and bundle identities, publication progress, and exact blockers across restart.

### Engineering leader

As an engineering leader, I can measure draft-PR success, implementation cycle time, validation repair, retries, spec gaps, human intervention, and token usage by harness, model, and execution profile.

### Security owner

As a security owner, I can keep forge mutation credentials out of the implementation worker and audit the deterministic service that pushes branches, creates PRs, and changes tracker state.

## Current state

Symphony currently:

- dispatches tracker issues in configured active states to Pi or Codex;
- creates per-issue clone, worktree, Docker, or SSH workspaces;
- relies on prompts to tell agents to implement, validate, commit, push, create a PR, and update tracker state;
- passes GitHub credentials into current Docker workers;
- verifies that an issue entering `Agent Review` has an open PR for its workspace branch;
- records worker runtime state and token totals primarily in process-local orchestrator state;
- has durable SQLite factory runs, stage runs, triage artifacts, publication intents, and events from A1;
- has no GitHub pull request creation method in the focused GitHub client;
- has no durable storage contract for code-sized artifacts;
- does not join an implementation commit or PR to a pinned approved specification.

A2 is implemented and live UAT accepted in commit `8129983` ([verify report](/specs/archive/2026-07-26-a2-uat-verify-report.md); [ADR-0003](/adrs/0003-a2-spec-stage-artifacts-and-gates.md)). A3 consumes its landed `spec_run_state.approved_version`, `approved_artifact_id`, terminal `spec_publication_intents`, stage-scoped attempts, and immutable `spec_artifacts` rather than introducing a second approval mechanism.

A3 must add:

- approved-spec eligibility and dispatch ownership;
- a first-class implementation stage and durable attempts;
- isolated local and Docker implementation runners;
- deterministic approved-spec materialization;
- a typed implementation manifest;
- deterministic validation and repair turns;
- content-addressed Git bundle storage;
- preview publication;
- deterministic branch push and draft-PR publication;
- tracker handoff, HTTP, events, metrics, doctor checks, prompts, documentation, tests, and GitHub UAT.

## Scope

### In scope

- GitHub Issues and Projects v2 runs approved through A2.
- Pinned approved spec consumption.
- Local disposable clone execution.
- Docker execution with credential-free repository import and result export.
- Pi and Codex implementation invocations.
- Versioned implementation, validation, change-bundle, and draft-PR artifacts.
- Ordered blocking validation commands.
- Bounded validation-repair cycles and bounded fresh attempts.
- Preview issue comment with no remote branch or PR effects.
- Idempotent remote branch push and draft PR creation.
- Agent Review Projects v2 handoff.
- Dispatch guards, restart recovery, HTTP, events, metrics, doctor checks, starter prompts, reference docs, tests, and UAT.

### Out of scope

- Direct A1 implement routes under the typed implementation stage.
- Manual attachment of specification artifacts.
- Linear implementation.
- SSH execution.
- Worker access to GitHub or tracker mutation credentials.
- Automatic rebases or merges from a moving base branch.
- Agent review, structured findings, inline PR comments, or review publication (A4).
- User-facing acceptance verification (A5).
- Merge, deployment, or release (A6).
- Authenticated HTTP artifact download (B2).
- Git bundle retention and garbage-collection policy beyond durable retention.
- Commit signing.
- Cost-in-currency attribution.

## Eligibility and dispatch ownership

An A3 candidate must satisfy all of the following:

1. `implementation.enabled` is true.
2. The factory run has a non-null A2 `approved_artifact_id` and `approved_version`.
3. The A2 approval publication intent is terminal and applied.
4. The pinned artifact exists, is a valid A2 spec artifact, and belongs to the run.
5. The current issue revision matches the revision approved by A2.
6. No successful A3 implementation artifact exists for that approved artifact and configuration revision.
7. No nonterminal A3 attempt or publication intent already owns the run.

The A3 runtime reconciles and claims candidates before the legacy orchestrator fetches implementation candidates. The legacy dispatch guard is derived from durable store state, not live configuration:

- a terminal A2 approval plus enabled A3 is guarded while A3 claims;
- every nonterminal A3 attempt or publication is guarded;
- exhausted, blocked, and successful A3 runs remain guarded;
- disabling A3 stops new claims but does not remove guards belonging to durable A3 work;
- direct A1 implementation runs without an approved pin are never guarded by A3.

This ordering prevents the A2 approval route from racing the legacy worker on its first eligible poll.

## Stage attempt model

One A3 attempt owns:

- approved spec artifact and version;
- issue and configuration revisions;
- base branch and captured base commit;
- execution profile;
- ordered implementation and repair turn records;
- ordered validation cycles and command results;
- the final implementation manifest;
- a verified result bundle;
- a preview or automatic publication intent.

An attempt starts in a fresh workspace from its captured base commit. A restart that interrupts an active worker follows A1 recovery identity and cleanup rules. The interrupted attempt fails durably; a retry uses a new attempt and fresh workspace.

Within one live attempt:

1. The initial implementation invocation edits and commits.
2. Symphony validates the manifest and repository postconditions.
3. Symphony executes every validation command in order.
4. If all pass, the attempt exports the result bundle.
5. If one fails and another validation cycle is available, Symphony starts a fresh repair invocation in the same workspace.
6. The repair input contains the issue, approved spec, previous manifest, and structured results from the failed cycle.
7. After repair, all validation commands run again from the beginning.
8. At `implementation.max_validation_cycles`, a remaining failure fails the attempt.

`max_validation_cycles: 3` means one initial validation plus at most two repair invocations. A manifest, integrity, spawn, timeout, bundle, or schema failure fails the attempt immediately and consumes an attempt retry rather than a repair cycle.

## Repository preparation and worker boundary

### Base capture

At claim time, Symphony refreshes `workspace.base_branch` from the pinned publication remote through the trusted controller and records that remote commit SHA. Local commits that are absent from the publication remote are excluded from automatic implementation bases. A moving base after claim does not rewrite the worker's commit. The captured SHA is exposed in evidence and the draft PR may show that the branch is behind.

Automatic publication verifies that the captured SHA remains reachable from the fetched remote base before importing the thin result bundle. Missing base history is a terminal publication conflict; Symphony does not push a branch or retry the same invalid bundle.

### Credential-free base bundle

Symphony creates a base Git bundle containing the captured commit and required reachable objects. The bundle:

- is written to a stage-owned temporary file;
- is verified with `git bundle verify`;
- is hashed and size-checked;
- contains no credentials or remote configuration;
- becomes the source for both local and Docker workspaces.

### Local execution

Local A3 execution clones the base bundle into a disposable attempt workspace. It does not use the general implementation worktree because worktrees share Git configuration and repository state.

The runner:

- clears the environment using the A1 allowlist pattern;
- uses an isolated home and Git configuration;
- removes or disables push URLs;
- does not inject `GH_TOKEN`, `GITHUB_TOKEN`, Linear credentials, Symphony helper variables, SSH agents, lifecycle hooks, or repository credential helpers;
- provides only model authentication required by the selected harness;
- configures a deterministic local commit identity;
- starts the worker as its own process-group leader;
- renews its durable lease while running.

The host account and same-user local process remain trusted as documented by A1; OS-enforced confinement is not claimed.

### Docker execution

Docker A3 execution starts the configured worker image without GitHub, Linear, SSH, or helper credentials. Symphony copies the verified base bundle and stage inputs into the container, clones `/workspace` from the bundle, configures the isolated commit identity, and disables push remotes.

Validation and repair run inside the same container so they observe the implementation environment. Before container removal, Symphony:

1. verifies the manifest and repository state in the container;
2. creates a result Git bundle for `base_commit..head_commit`;
3. copies it to a host-owned temporary path;
4. verifies and imports it into a temporary host repository;
5. checks the exact head/tree SHAs, diff, and approved-spec bytes;
6. stores the bundle durably;
7. only then removes the container.

Failure to export or host-verify the bundle fails the attempt and retains bounded diagnostics.

## Approved-spec repository file

The default expanded path is:

`specs/{issue_identifier}/APPROVED-v{version}.md`

Supported template variables:

- `{issue_identifier}`
- `{run_id}`
- `{artifact_id}`
- `{version}`

The configured template and expanded path must:

- be non-empty UTF-8;
- be relative to the repository root;
- normalize without `.` or `..`;
- not address `.git` or a path beneath it;
- end in `.md`;
- expand to at most 240 UTF-8 bytes;
- identify a regular file, not a symlink;
- not collide with different repository content.

The deterministic file contains:

```markdown
---
symphony_factory_run: 018f...
symphony_issue: KATA-123
symphony_spec_artifact: 018f...
symphony_spec_version: 2
symphony_approved_at: 2026-07-26T20:00:00Z
---

# Approved specification for KATA-123

## Product behavior

...

## Technical approach

...

## Acceptance criteria

1. ...

## Open decisions

- None.
```

Symphony materializes this file before the worker starts. The worker must commit it with the implementation and may not alter it. Post-run validation byte-compares the committed blob with the canonical render.

The final diff must contain at least one changed path other than the approved-spec file. A spec-only commit is not a successful implementation.

## Implementation manifest

The worker writes UTF-8 JSON to `SYMPHONY_STAGE_OUTPUT`. The file is limited to 64 KiB, uses schema version 1, rejects unknown fields, and rejects empty-after-trim strings.

Completed example:

```json
{
  "schema_version": 1,
  "status": "completed",
  "head_commit": "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
  "summary": "Adds the approved retry policy and records each repair cycle.",
  "acceptance_criteria": [
    {
      "index": 1,
      "status": "implemented",
      "evidence": [
        {
          "kind": "repository",
          "reference": "apps/symphony/src/implementation/coordinator.rs",
          "summary": "The coordinator enforces the configured repair-cycle bound."
        },
        {
          "kind": "test",
          "reference": "implementation::tests::validation_repairs_once",
          "summary": "The test proves one failed validation invokes one repair and then passes."
        }
      ]
    }
  ],
  "known_limitations": []
}
```

Blocked example:

```json
{
  "schema_version": 1,
  "status": "blocked",
  "head_commit": null,
  "summary": "The approved spec does not define the required behavior for forked repositories.",
  "acceptance_criteria": [],
  "known_limitations": [],
  "blocker": {
    "kind": "spec_gap",
    "summary": "Fork behavior has two incompatible safe implementations.",
    "evidence": "Acceptance criterion 3 requires fork support but does not choose the push target."
  }
}
```

Contract:

- `schema_version`: integer, exactly `1`.
- `status`: `completed` or `blocked`.
- `head_commit`: 40- or 64-character lowercase hexadecimal object ID for `completed`; null for `blocked`.
- `summary`: non-empty string, at most 4,000 UTF-8 bytes.
- `acceptance_criteria`: for `completed`, exactly one entry for each approved criterion; for `blocked`, empty.
- `acceptance_criteria[].index`: unique, one-based index covering the complete approved list.
- `acceptance_criteria[].status`: exactly `implemented` in a completed manifest.
- `acceptance_criteria[].evidence`: array of 1 to 10 entries.
- `evidence[].kind`: `repository`, `test`, or `documentation`.
- `evidence[].reference`: non-empty string, at most 500 UTF-8 bytes.
- `evidence[].summary`: non-empty string, at most 1,000 UTF-8 bytes.
- `known_limitations`: array of 0 to 20 non-empty strings, each at most 1,000 UTF-8 bytes.
- `blocker`: required only for `blocked`.
- `blocker.kind`: `spec_gap`, `environment`, or `repository`.
- `blocker.summary`: non-empty string, at most 2,000 UTF-8 bytes.
- `blocker.evidence`: non-empty string, at most 4,000 UTF-8 bytes.

Acceptance-criterion entries are implementation claims for A4 and A5 to check. They do not replace review or verification.

A `spec_gap` blocker moves the stage to `awaiting_human`, publishes a diagnostic, and does not auto-retry. `environment` and `repository` blockers fail the attempt and follow bounded attempt retry semantics.

## Repository postconditions

Before validation Symphony requires:

- `HEAD` equals the manifest's `head_commit`;
- `HEAD` descends from the captured base commit;
- the working tree, index, and submodules are clean;
- no unresolved conflicts exist;
- the canonical spec file is committed and byte-identical;
- at least one non-spec path differs from the base;
- the changed-file count and rendered evidence fit API bounds;
- no Git remote contains embedded credentials;
- the result can be represented as a verified Git bundle.

Ignored build output may remain. Staged, unstaged, or untracked non-ignored files fail the postcondition.

## Validation

Validation commands are trusted repository configuration and run sequentially in the attempt workspace:

```yaml
validation:
  - name: affected-validation
    command: pnpm run validate:affected
    timeout_ms: 1800000
```

Each command record contains:

- configured name;
- SHA-256 of the exact command;
- validation cycle;
- start and completion timestamps;
- duration;
- exit code or termination reason;
- pass/fail;
- bounded redacted stdout and stderr tails;
- SHA-256 of the captured output;
- local or Docker execution profile.

Command names are required, unique, and at most 100 UTF-8 bytes. Commands are non-empty and at most 4,000 UTF-8 bytes. `timeout_ms` is required, greater than zero, and applies independently to each command.

All commands run on every cycle. The first failure stops the current cycle; a repair turn receives results through that failure. The next cycle restarts from the first configured command.

Only command name, pass/fail, exit code, and duration are published. Raw output remains bounded operator evidence and passes through existing secret-redaction helpers before persistence.

## Durable change-bundle storage

Code-sized bundles do not belong in SQLite. A3 adds a content-addressed blob directory beside the factory database:

- explicit future configuration is deferred; the initial path is derived from the resolved `storage.path`;
- layout: `<database-file>.artifacts/sha256/<first-two>/<full-digest>`;
- writes use a stage-owned temporary file, size check, file sync, atomic rename, and parent-directory sync;
- existing content at a digest is re-verified before reuse;
- metadata is committed to SQLite only after the blob is durable;
- incomplete temporary files are ignored and cleaned on startup;
- `implementation.max_bundle_bytes` defaults to 100 MiB and is enforced while copying;
- bundle contents are verified at ingestion and immediately before publication;
- HTTP returns digest, size, and Git metadata but never the filesystem path or bundle bytes.

The bundle metadata records:

- artifact ID;
- base, head, and tree SHAs;
- SHA-256 and byte length;
- changed-file count and bounded path summary;
- approved-spec path and blob SHA;
- harness, model, and execution profile;
- created and verified timestamps.

Retention and garbage collection are deferred. Build must document the disk-growth implication.

## Configuration

```yaml
implementation:
  enabled: true
  mode: preview
  prompt: prompts/implementation.md
  repair_prompt: prompts/implementation-repair.md
  model: anthropic/claude-sonnet-4-6
  max_turns: 20
  invocation_timeout_ms: 3600000
  max_attempts: 3
  max_validation_cycles: 3
  max_bundle_bytes: 104857600
  spec_file: specs/{issue_identifier}/APPROVED-v{version}.md
  validation:
    - name: affected-validation
      command: pnpm run validate:affected
      timeout_ms: 1800000
  completion_route:
    state: Agent Review
```

Configuration behavior:

- `implementation.enabled` defaults to `false`.
- `implementation.mode` accepts `preview` or `automatic` and defaults to `preview`.
- Both prompts are required when enabled and resolve relative to the active `WORKFLOW.md`.
- `implementation.model` uses Pi precedence: stage model, then `agent.model`, then harness default. Config rejects it for Codex while the Codex contract lacks a model override.
- `max_turns` defaults to `agent.max_turns` and must be greater than zero.
- `invocation_timeout_ms` defaults to `3600000` and applies to each implementation or repair invocation.
- `max_attempts` defaults to `3` and must be greater than zero.
- `max_validation_cycles` defaults to `3` and must be greater than zero.
- `max_bundle_bytes` defaults to `104857600`, must be greater than zero, and has a documented hard upper validation bound of 1 GiB.
- `spec_file` defaults to the path above and follows the path contract.
- `validation` must contain 1 to 20 uniquely named commands.
- `completion_route.state` is required in automatic mode and must resolve through the configured Projects v2 status field. The starter value is `Agent Review`.
- The implementation configuration revision hashes schema versions, both prompt contents, model, timeouts, attempt/cycle/bundle bounds, spec template, validation commands, and completion route.
- Disabling implementation stops new claims. It does not cancel attempts, remove dispatch guards, or stop publication reconciliation.

`symphony doctor` validates:

- A2/spec-stage configuration presence and approval-route composition;
- prompt readability;
- path-template expansion with representative maximum identifiers;
- validation uniqueness and bounds;
- artifact-directory creation, writability, and atomic rename;
- base repository and branch access;
- local or Docker harness authentication;
- Docker availability and image resolution when configured;
- GitHub contents and pull-request write access;
- completion Projects v2 state.

`symphony init` writes both prompts and commented starter configuration. It creates no remote state or labels.

## Coordinator and store

A3 extends the stage-neutral store expected from A2 rather than adding another triage-shaped store.

Required durable additions:

- implementation attempt inputs: approved artifact/version, approval revision, base branch/SHA, execution profile;
- turn records for `implement` and `repair`;
- validation cycle and command-result tables;
- typed implementation-manifest artifacts;
- bundle artifact metadata;
- preview and automatic publication intents;
- draft-PR artifacts;
- dispatch-guard query state;
- awaiting-human blocker state.

Uniqueness:

- at most one nonterminal implementation attempt per `(run_id, approved_artifact_id, configuration_revision)`;
- at most one successful implementation artifact per the same key;
- at most one durable bundle per successful implementation artifact;
- at most one nonterminal publication intent per implementation artifact;
- at most one draft-PR artifact per implementation artifact.

Startup order:

1. acquire the repository-scoped store lock and migrate;
2. clean incomplete blob temporary files;
3. recover interrupted processes and containers;
4. reconcile preview and automatic publication intents;
5. rebuild durable dispatch guards;
6. inspect approved A2 candidates;
7. allow the legacy orchestrator to fetch its remaining candidates.

Long implementation and validation work runs outside the poll loop. Lease renewal and turn boundaries allow A1/A2 reconciliation to continue.

## Preview publication

PR1 creates or updates one publisher-owned issue comment:

`<!-- symphony:implementation:{intent_id} -->`

The comment contains:

- preview notice stating no remote branch or PR was created;
- factory run, stage run, and implementation artifact IDs;
- approved spec version and committed path;
- base/head commit abbreviations;
- summary and known limitations;
- bounded changed-file list;
- acceptance-criterion implementation claims;
- validation command statuses and durations;
- instructions for switching the workflow to automatic mode.

Comment ownership, authenticated-author verification, pagination, spoofed-marker rejection, create-before-record recovery, and bounded rendering follow A1/A2.

## Automatic publication

Automatic publication uses a durable expected-projection intent.

### Branch

The desired branch is the existing deterministic workspace branch:

`{workspace.branch_prefix}/{sanitized issue identifier}`

Publication reconstructs a temporary trusted repository from the current remote base and verified result bundle. It observes the remote branch:

- absent: push the desired head;
- present at desired head: record prior success and continue;
- present at the recorded expected SHA and fast-forwardable to desired: push normally;
- any other SHA: record a human conflict and stop;
- force push is prohibited.

The intent retains the repository, base, branch, expected remote SHA, desired head SHA, and bundle identity. A configuration reload cannot retarget an existing intent.

### Draft pull request

After branch reconciliation, Symphony lists pull requests with `state=all`, the repository owner/head branch, and configured base branch.

The PR body contains:

- publisher ownership marker;
- `Closes #<issue number>`;
- factory run, stage run, implementation artifact, and bundle IDs;
- approved spec artifact and version;
- committed approved-spec path;
- implementation summary;
- acceptance-criterion claims;
- validation summary;
- known limitations;
- captured base and desired head SHAs.

Recovery rules:

- reuse one open draft PR only when marker, head, base, and head SHA match;
- a create-before-record crash recovers that PR and stores its identity;
- multiple candidates are a conflict;
- an open PR without the ownership marker is a conflict;
- a closed, merged, or converted-to-ready owned PR is a conflict while publication is nonterminal;
- head/base/SHA drift is a conflict;
- Symphony never silently edits a human-modified PR back to its desired projection.

### Publication steps

1. Update the owned issue comment to `publication: pending`.
2. Re-verify the approved pin, current issue revision, and stored bundle.
3. Observe and reconcile the remote branch.
4. Record `implementation_branch_pushed`.
5. Find or create the draft PR.
6. Verify draft state, marker, head/base, and head SHA.
7. Store the immutable draft-PR artifact.
8. Remove the exact implementation label recorded by the A2 approval intent.
9. Apply `completion_route.state`.
10. Update the owned issue comment to `draft PR created — Agent Review`.
11. Mark the publication intent applied and emit completion.

The API reports publication success only after step 11. Tracker state never advances before the verified draft-PR artifact exists.

Every remote step uses expected-projection reconciliation. Authorization, branch protection, missing state, and transient API failures remain pending with remediation. Human divergence records a conflict and stops automatic effects.

## Staleness and blockers

- Issue revision changed after A2 approval: `approved_spec_stale`, no attempt.
- Issue revision changed during implementation: attempt may finish locally, but publication is blocked and no remote effect occurs.
- Pinned A2 artifact changed: existing attempt or intent conflicts; it is never silently retargeted.
- Approved artifact missing or invalid: durable configuration/data-integrity failure.
- Base branch moved: record old/current SHAs; do not automatically merge or rebase.
- Spec path collision: fail before worker start.
- Manifest `spec_gap`: awaiting human, diagnostic comment, no retry.
- Validation exhaustion: fail attempt and retry fresh up to `max_attempts`.
- Attempt exhaustion: durable failed stage with exact validation/remediation evidence.
- Bundle exceeds size cap or fails verification: fail attempt.
- Remote branch or PR drift: publication conflict, retain bundle and guard.
- Missing GitHub or Projects v2 permissions: publication blocked and retryable after operator remediation.

## HTTP API

Factory-run responses add an `implementation` object:

```json
{
  "implementation": {
    "status": "published",
    "approved_spec": {
      "artifact_id": "018f...",
      "version": 2,
      "file_path": "specs/KATA-123/APPROVED-v2.md"
    },
    "attempts": 1,
    "validation_cycles": 2,
    "base_commit": "a1b2c3...",
    "head_commit": "d4e5f6...",
    "changed_files": 8,
    "artifact_id": "018f...",
    "bundle": {
      "artifact_id": "018f...",
      "sha256": "4b825d...",
      "bytes": 18422
    },
    "publication": {
      "intent_id": "018f...",
      "mode": "automatic",
      "status": "applied",
      "completed_steps": [
        "comment_pending",
        "branch_pushed",
        "pr_verified",
        "route_applied",
        "comment_final"
      ],
      "error": null
    },
    "pull_request": {
      "number": 42,
      "url": "https://github.com/example/repo/pull/42",
      "draft": true,
      "head": "symphony/KATA-123",
      "base": "main",
      "head_sha": "d4e5f6..."
    },
    "blocker": null
  }
}
```

Attempt entries expose implementation/repair turns with timing, usage, harness, model, execution profile, status, and bounded error. Validation entries expose command metadata and redacted output through the artifact subresource.

Bundle bytes and physical paths are not served. Unknown artifact IDs return `404`; unsupported stage filters return `400`.

`GET /api/v1/factory-runs/metrics?stage=implementation` returns eligibility, attempt, success, failure, validation-cycle, repair, spec-gap, preview, publication, conflict, approval-to-PR latency, duration, and token aggregates grouped by harness, model, and execution profile.

## Events

Live and durable events:

- `implementation_started`
- `implementation_turn_completed`
- `implementation_repair_started`
- `implementation_validation_completed`
- `implementation_completed`
- `implementation_blocked`
- `implementation_failed`
- `implementation_preview_published`
- `implementation_publication_started`
- `implementation_branch_pushed`
- `implementation_pr_created`
- `implementation_route_applied`
- `implementation_publication_blocked`
- `implementation_publication_conflict`

Payloads follow the factory event envelope and include run, stage-run, approved-spec, implementation-artifact, bundle, intent, and PR identifiers when applicable. Errors use bounded structured codes and remediation.

## Security and trust boundaries

- Issue text, spec content, repository content, worker output, validation output, and Git history are untrusted inputs.
- Workflow configuration and the Symphony controller are trusted.
- The worker may mutate only its disposable workspace and write its output manifest.
- The worker receives model auth but no forge/tracker mutation credentials.
- The local same-user worker remains trusted at the OS level; filesystem sandboxing is not claimed.
- Docker receives no GitHub, Linear, SSH agent, helper, or host credential mounts.
- Only the trusted publisher imports verified bundles, pushes branches, creates PRs, and changes tracker state.
- Git commands use explicit arguments and validated refs/paths; user content is never interpolated into an unquoted shell command.
- Bundle size, manifest size, output size, changed paths, comments, PR bodies, validation output, and persisted errors are bounded.
- Validation output is redacted before persistence and never copied verbatim into GitHub.
- HTTP exposes source-derived metadata without bundle download until authenticated remote control exists.
- Publication never force-pushes or overwrites human divergence.

## User-visible behavior

### PR1: implementation and validation preview

Approving a current spec starts A3 instead of the legacy worker. Symphony implements in local or Docker isolation, repairs configured validation failures within bounds, stores a verified bundle, and posts one preview comment. No branch or PR is created and tracker state does not advance.

### PR2: draft PR publication

Automatic mode publishes the verified bundle to the deterministic branch, creates one owned draft PR, removes the implementation label, moves the issue to Agent Review, and updates the issue comment with the PR URL.

## Delivery slices

### Pull request 1: implementation and validation preview

Delivers:

- configuration and doctor validation;
- A2 eligibility and dispatch guard;
- store and blob extensions;
- local and Docker bundle-backed runners;
- approved-spec materialization;
- manifest validation;
- repository postconditions;
- validation and repair loop;
- preview comment;
- HTTP, metrics, and events;
- starter prompts, reference docs, automated tests, and preview UAT.

User value: maintainers can evaluate spec-driven implementation quality and validation behavior without allowing code publication.

### Pull request 2: deterministic draft-PR publication

Delivers:

- trusted bundle import;
- remote branch expected-projection publisher;
- GitHub list/create pull request client methods;
- PR ownership and recovery;
- tracker handoff;
- publication HTTP, metrics, events, tests, and full UAT.

User value: the complete PRD A3 journey ends in a linked, validated draft PR without exposing forge mutation credentials to the worker.

## Acceptance criteria

1. Enabling A3 requires valid prompts, model settings, attempt/cycle/bundle bounds, a safe spec path template, 1–20 unique blocking validation commands, artifact-directory access, and an automatic completion state; doctor reports exact remediation for every failure.
2. Only a terminal A2 approval with a valid pinned artifact and matching current issue revision is eligible. Labels alone, direct A1 implement routes, stale approvals, and incomplete A2 publications never enter A3.
3. A3 claim/reconciliation runs before legacy candidate dispatch. Durable guards prevent duplicate ownership across normal polling, restart, configuration reload, blocked runs, exhausted runs, and successful Agent Review handoff. A3 disabled leaves direct and legacy behavior unchanged.
4. Before a worker starts, Symphony persists the attempt, approved artifact/version, issue/configuration revisions, base branch/SHA, execution profile, and lease under stage-scoped uniqueness.
5. Local execution uses a disposable bundle clone, isolated home and Git configuration, its own process group, no push remote, and no forge, tracker-helper, SSH, or lifecycle-hook credentials.
6. Docker execution receives the same bounded inputs and model auth only; tests prove GitHub, Linear, helper, and SSH credentials are absent and prove the repository enters and leaves through verified bundles.
7. The approved-spec file path contract rejects traversal, absolute paths, `.git`, symlinks, non-Markdown targets, oversize expansion, and conflicting existing content. The committed file is byte-identical to the deterministic A2 artifact render.
8. Manifest validation enforces schema version 1, unknown-field rejection, the file and field bounds, exact completed/blocked shape, exact head SHA, and a unique complete mapping of every approved acceptance criterion.
9. A completed worker result requires a clean committed repository, head equality, base ancestry, unchanged canonical spec file, no unresolved submodule/conflict state, and at least one non-spec changed path.
10. Validation runs configured commands in order with independent timeouts and bounded redacted evidence. Any failure blocks preview/publication. Repair invocations receive structured failure input, run in the same workspace, and are bounded by `max_validation_cycles`; each new cycle reruns all commands from the beginning.
11. Repair success produces one successful attempt. Cycle exhaustion fails the attempt; fresh attempts are bounded by `max_attempts`. A `spec_gap` manifest enters awaiting-human without automatic retry.
12. Local and Docker results produce host-verified Git bundles. Atomic content-addressed storage enforces digest and size, survives restart, detects tampering before publication, and never exposes bundle paths or bytes through HTTP.
13. PR1 publishes exactly one owned preview comment with spec, commit, changed-file, criterion, validation, and limitation evidence and makes no branch, PR, label, or state mutation. Crash and create-before-record recovery are idempotent and spoofed markers are ignored.
14. Automatic branch publication handles absent, already-desired, and expected-fast-forward projections without force; any unrelated remote SHA records a human conflict and stops.
15. Draft-PR publication uses GitHub's pull request API with `draft: true`, issue/spec/run/evidence links, validation summary, and an ownership marker. It recovers create-before-record by head/base query and rejects duplicate, foreign, closed, ready, or drifted candidates.
16. Tracker state cannot advance before a verified draft-PR artifact exists. Applied publication removes the recorded implementation label, moves to configured Agent Review, finalizes the issue comment, and only then reports success.
17. Restart tests cover active local and Docker workers, result export, blob persistence, branch push, PR create-before-record, tracker handoff, and final comment without duplicate external effects.
18. HTTP exposes attempts, turns, validation cycles, approved-spec identity, commit/bundle metadata, publication steps, PR identity, and blockers. Metrics and all fourteen events match their documented contracts.
19. Automated tests cover configuration, eligibility, dispatch races, schemas, path safety, local/Docker isolation, manifest and Git postconditions, repair loops, blob durability, publication projections, HTTP/events/metrics, and unchanged A1/A2/legacy suites.
20. Manual PR1 UAT demonstrates successful local and Docker previews plus a failed validation repaired within bounds. Manual PR2 UAT creates one linked draft PR, proves Agent Review occurs only afterward, restarts during publication, and cleans all issues, labels, states, branches, PRs, containers, workspaces, and test blobs.

## Measures

From durable records:

- eligible approved specs;
- draft-PR success rate;
- approval-to-draft-PR cycle time;
- implementation attempt and retry counts;
- validation first-pass rate and repair cycles;
- validation exhaustion;
- spec-gap and stale-approval counts;
- publication blocks and human conflicts;
- local versus Docker outcome;
- human intervention proxies;
- token usage by harness and model;
- bundle sizes and changed-file counts.

Implementation rework attributed to spec gaps starts with A3 `spec_gap` records and later joins A4/A5 findings to the pinned artifact ID.

## Testing strategy

### Unit tests

- Configuration defaults, bounds, model behavior, validation uniqueness, and completion state.
- Spec path template parsing, expansion, normalization, collision, and bounds.
- Deterministic approved-spec rendering.
- Manifest schema, bounds, blocked shapes, criterion coverage, and SHA validation.
- Validation-cycle and repair state machine.
- Git postcondition assessment.
- Bundle metadata, digest, size, and atomic-path calculation.
- Preview and PR body rendering with strict bounds.
- Remote branch and PR expected-projection decision tables.

### Integration tests

- A2 approval eligibility, stale revision, incomplete approval, direct-route exclusion, and first-poll legacy dispatch race.
- Local fake harness producing commits, malformed manifests, dirty trees, missing spec, spec-only changes, and successful bundles.
- Docker fake harness with credential assertions, commit export, host import, timeout, and cleanup.
- Validation pass, failure at each command, repair pass, repeated failure, cycle cap, attempt cap, and spec-gap block.
- Blob crash windows before rename, after rename/before SQLite, hash collision/tamper, oversize stream, and restart cleanup.
- Preview comment ownership and recovery.
- Remote bare-Git branch publication projections.
- Mock GitHub PR create and list recovery across every external-success/local-recording crash window.
- Tracker transition ordering and conflicts.
- Restart recovery and durable guard behavior independent of live configuration.
- Exact HTTP, artifact, metrics, and event envelopes.
- Existing triage, orchestrator, workspace, and A2 suites unchanged.

### Manual UAT

Use the established UAT harness and cleanup discipline:

1. Approve an A2 spec on a real GitHub Projects v2 issue.
2. Run local preview and capture comment/API/bundle evidence.
3. Run Docker preview and prove the same contract.
4. Use a fixture whose first validation fails and whose repair turn fixes it.
5. Enable automatic mode and observe one remote branch and one draft PR.
6. Verify the committed approved-spec file and PR linkage.
7. Restart after branch push but before local recording and recover without duplication.
8. Verify Agent Review occurs only after PR artifact persistence.
9. Remove all UAT state and record proof links.

### Quality gate

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
pnpm run validate:affected
```

## Likely file map

New focused modules under `apps/symphony/src/implementation/`:

- `domain.rs` — attempts, manifests, validation, bundle, and PR types;
- `artifact.rs` — manifest parsing and deterministic spec rendering;
- `coordinator.rs` — eligibility, attempts, repair loop, and reconciliation;
- `runner.rs` — local and Docker invocation profiles;
- `validation.rs` — deterministic command execution and evidence;
- `bundle.rs` — bundle creation, verification, import, and blob storage;
- `publisher.rs` — preview, branch, PR, and tracker publication;
- `comment.rs` — bounded issue and PR rendering;
- `runtime.rs` — startup/poll integration.

Existing areas likely affected:

- A2's stage-neutral factory store and migrations;
- `apps/symphony/src/config.rs`, `domain.rs`, and `doctor.rs`;
- `apps/symphony/src/orchestrator.rs` for ordering and durable dispatch guards;
- `apps/symphony/src/docker.rs` and `workspace.rs` for credential-free bundle workspaces;
- `apps/symphony/src/github/client.rs` for list/create PR APIs;
- `apps/symphony/src/http_server.rs` and `event_stream.rs`;
- `apps/symphony/src/starter_assets.rs`, prompts, workflow reference, README, and tests.

Build confirms names. Shared A1/A2 infrastructure may be lifted into `factory/` only when A3 consumes the seam; unrelated renaming is out of scope.

## Risks and mitigations

### A2 interface drift

A2's prerequisite is shipped. A3 must trace the landed `spec_run_state`, `spec_artifacts`, `spec_publication_intents`, and stage-scoped store interfaces before extending them, and keep the full A2 suite green while adding implementation records.

### Implementation bundles consume disk

Use size bounds, content addressing, deduplication, operator metrics, and explicit retention disclosure. Garbage collection remains a follow-up.

### Docker export loses work

Do not remove the container until the host has verified and durably stored the result bundle. Export failure fails the attempt visibly.

### Agent claims spec conformance incorrectly

Treat manifest entries as claims. A4 reviews against the same pinned spec and A5 verifies acceptance evidence.

### Validation repair burns time and tokens

Bound cycles, commands, timeouts, output, and fresh attempts. Record first-pass and repair metrics.

### Base branch moves during a long run

Record the captured base and current PR relationship. Do not perform an unreviewed automatic merge; later review/CI handles branch freshness.

### Existing remote branch or PR collides

Use stable ownership markers and expected projections. Stop on divergence and never force-push.

### Local same-user process can reach host data

Retain A1's trusted-host limitation, isolate normal credentials and homes, disable push remotes, and document that stronger OS confinement requires Docker or future sandbox work.

### A3 and legacy implementation both dispatch

Claim before legacy fetch and compute guards from durable A2/A3 state. Add explicit first-poll, restart, and disabled-config race tests.

## Explicitly deferred work

- Typed implementation for direct A1 routes.
- Linear.
- SSH.
- Automatic base updates.
- Structured review and finding publication (A4).
- Acceptance verification (A5).
- Merge/deploy (A6).
- HTTP bundle download and remote auth.
- Retention/garbage collection.
- Commit signing.
- Risk-based execution profiles.
- Cost-in-currency attribution.

## Build handoff

### Approved scope

Build A3 as two user-facing pull requests: implementation/validation preview, then deterministic draft-PR publication and Agent Review handoff.

### Non-negotiable constraints

- A2 terminal approval and pinned artifact are the only A3 intake.
- Direct implementation stays legacy.
- Durable attempt before worker execution.
- No forge/tracker mutation credentials in local or Docker workers.
- Exact approved spec committed and immutable.
- Typed manifest plus deterministic blocking validation.
- Bounded repair cycles and attempts.
- Host-verified durable bundle before success.
- Only the trusted publisher pushes, creates PRs, and changes tracker state.
- No force push or overwrite of human divergence.
- Every pull request has tracker-visible output, durable evidence, tests, and UAT.

### Build sequence

1. Trace the shipped A2 pinning, publication-intent, artifact, and stage-scoped store interfaces and lock focused compatibility tests before extending them.
2. Add stage-neutral attempt/artifact/blob seams with existing A1/A2 suites green.
3. Build PR1 end to end: eligibility, guard, runners, manifest, validation/repair, bundle, preview, API/events/metrics/docs/UAT.
4. Review preview evidence and credential-isolation tests.
5. Build PR2 end to end: branch publisher, GitHub PR API, ownership/recovery, handoff, API/events/metrics/docs/UAT.
6. Add an ADR when the durability, blob, and publication contracts ship.
7. Update the OKF roadmap, PRD progress, reference documentation, and logs after each merged slice.

### Verification contract

Build completion requires all twenty acceptance criteria passing or explicitly blocked with evidence; focused and full automated gates; real local and Docker UAT; real GitHub draft-PR and restart evidence; cleanup proof; adversarial review with no unresolved blockers; and a completion report listing commits, commands, evidence, and residual risks.

### Blocking conditions

Stop Build and request a decision if:

- the landed A2 immutable pin or terminal approval boundary differs from the documented contract in a way that requires migration;
- credential-free local or Docker repository preparation cannot be enforced at the documented level;
- a validated Docker commit cannot be exported and host-verified before cleanup;
- the stage-neutral store cannot support code-sized blob metadata without destabilizing A1/A2;
- the legacy dispatch race cannot be closed before candidate fetch;
- GitHub permissions cannot separate worker execution from trusted publication;
- a user-facing slice would require a foundation-only pull request;
- an acceptance criterion requires A4, A5, SSH, Linear, merge, or deployment.
