---
type: Spec
title: A1 GitHub Issue Triage
status: Active
description: Design for a durable, repository-backed GitHub triage stage that routes issues into Symphony's software factory.
tags: [symphony, software-factory, triage, github]
timestamp: 2026-07-18T16:45:00Z
---

# A1 GitHub Issue Triage

## Status

Active — **PR1 (triage preview) shipped** in [#587](https://github.com/gannonh/kata-symphony/pull/587) (`80b3c215`). **PR2 (automatic route publication) Verify accepted** ([build](/specs/2026-07-24-a1-pr2-build-report.md), [verify](/specs/2026-07-24-a1-pr2-verify-report.md)); merge/PR pending. **PR3 (recovery and agreement measurement) remediation passes automated gates; live re-verify is blocked on UAT credential rotation** ([build](/specs/2026-07-25-a1-pr3-build-report.md), [rejected verify](/specs/2026-07-25-a1-pr3-verify-report.md), [incomplete re-verify](/specs/2026-07-25-a1-pr3-reverify-report.md)).

Related decisions: [ADR-0001 A1 triage durability and isolation](/adrs/0001-a1-triage-durability-and-isolation.md).

## Goal

Deliver the first software factory stage as a complete user-facing workflow:

`GitHub issue labeled needs-triage → repository-backed triage → durable decision → visible route → optional implementation handoff`

A1 introduces the minimum durable factory-run model required by the [Symphony Software Factory Platform PRD](/specs/symphony-software-factory-platform-prd.md). It gives maintainers a reviewable triage decision, applies the configured route through a deterministic GitHub publisher, and makes implementation-ready issues eligible for Symphony's existing dispatcher.

A1 ships through independently usable preview, automatic-routing, and recovery increments. Every pull request includes a trigger, tracker-visible result, durable evidence, focused tests, documentation, and a demo path.

## Source of truth

- [Symphony Software Factory Platform PRD, A1](/specs/symphony-software-factory-platform-prd.md)
- [Triage label vocabulary](/agents/triage-labels.md)
- [Issue tracker conventions](/agents/issue-tracker.md)
- [Warp automatic triage article](https://www.warp.dev/blog/how-to-build-a-cloud-software-factory-the-automatic-triage-skill)
- `apps/symphony/README.md`
- `apps/symphony/src/domain.rs`
- `apps/symphony/src/orchestrator.rs`
- `apps/symphony/src/linear/adapter.rs`
- `apps/symphony/src/github/adapter.rs`
- `apps/symphony/src/event_stream.rs`

## Product decisions

- A1 supports GitHub Projects v2 first. Linear parity and the dormant GitHub label-state path are separate vertical slices.
- Symphony polls open issues carrying a configured intake label and belonging to the configured GitHub Project. A1 does not add a webhook receiver or add issues to projects.
- Canonical routes are backend-neutral and map to configurable GitHub labels and optional Projects v2 states.
- A1 persists factory runs, triage stage runs, artifacts, publication progress, and durable events in SQLite.
- Triage executes locally in a clone-only disposable workspace. Docker and SSH triage execution are deferred.
- A1 treats the local runner and host account as trusted. Symphony does not inject GitHub, Git/SSH, Symphony-helper, or lifecycle-hook write capabilities into the child; OS-enforced same-user isolation is deferred.
- Automatic mode applies the selected route without a pre-publication human gate.
- Preview mode remains available for baseline collection and safe rollout, with an explicit promotion path into automatic publication.
- The default implement route makes the issue eligible for the existing implementation flow only after publication completes.
- A1 creates a first-class triage subsystem. It does not model triage as another implementation worker state.

## User stories

### Maintainer

As a maintainer, I can label an issue `needs-triage` and receive a concise route, rationale, evidence, risk class, and next action without starting an interactive coding session.

### Factory operator

As a factory operator, I can inspect the durable triage attempt, diagnose failures, restart Symphony safely, and see whether GitHub publication completed.

### Engineering leader

As an engineering leader, I can measure triage latency, model and token usage, route distribution, failures, and human route corrections from durable records.

## Current state

Symphony currently:

- polls GitHub and Linear issues in configured implementation-active states;
- normalizes tracker issues into `Issue` values;
- dispatches Pi or Codex workers into isolated workspaces;
- changes prompts when tracker state changes;
- updates tracker state and comments through tracker adapters and helpers;
- streams versioned in-memory events and exposes runtime snapshots over HTTP;
- retries failed implementation runs and preserves process-local state;
- contains adapter code for GitHub label-state and Projects v2 state modes, while current workflow validation requires Projects v2.

A1 must add:

- intake by triage label independently of implementation-active states;
- typed triage routes and artifacts;
- durable factory and stage records;
- a triage-specific runner and output contract;
- deterministic route publication;
- restart-safe reconciliation;
- minimal factory-run HTTP visibility;
- correction and triage outcome telemetry.

## Scope

### In scope

- GitHub Issues intake through polling, limited to issues already in the configured GitHub Project.
- GitHub Projects v2 state mode.
- Preview and automatic triage modes, including preview-to-automatic promotion.
- Local repository-backed Pi or Codex triage using the configured harness.
- Configurable intake label, prompt, route labels, and optional route states.
- SQLite durability and schema migrations.
- Durable factory-run, stage-run, artifact, publication, and event records.
- Idempotent GitHub comments, route labels, and state updates.
- Immediate handoff from the implement route to the existing dispatcher.
- Minimal HTTP read API and event-stream additions.
- `symphony doctor`, starter configuration, reference documentation, automated tests, and GitHub UAT.

### Out of scope

- Linear triage.
- GitHub label-state triage and re-enabling label-state workflow configuration.
- Automatically adding issues to a GitHub Project.
- Docker or SSH triage execution.
- GitHub webhooks.
- A general workflow or DAG engine.
- Product or technical specification generation; A2 owns specification.
- Model-confidence routing thresholds.
- Human approval before automatic route publication.
- Automatic GitHub label creation.
- Authentication or RBAC for the HTTP server; Horizon B2 owns remote control security.
- A complete historical control-room UI; Horizon B1 owns recovery presentation and the full timeline.
- Autonomous merge or deployment.
- Durable shared memory or self-improvement.

## Canonical triage model

### Routes

A triage artifact contains exactly one canonical route:

- `implement`: sufficiently clear and bounded for the existing implementation flow.
- `spec`: aligned work that needs product or technical specification before implementation.
- `needs_information`: blocked on a specific answer from a human.
- `park`: valid work that should remain deferred.
- `human_owned`: work whose current risk, ambiguity, or nature requires human implementation.

### Risk class

A1 records one descriptive risk class:

- `low`
- `medium`
- `high`

Risk class is evidence for maintainers and future policy. A1 does not make approval decisions from it.

### Artifact schema

The stage output is UTF-8 JSON with schema version `1`. Unknown fields are rejected. Symphony records stage start, receipt, validation, and completion timestamps; agent-provided timestamps are not accepted.

```json
{
  "schema_version": 1,
  "route": "needs_information",
  "risk_class": "medium",
  "rationale": "The expected behavior is not defined for repositories with no default branch.",
  "evidence": [
    {
      "kind": "issue",
      "reference": "body",
      "summary": "The issue requests fallback behavior but does not identify the desired branch-selection rule."
    },
    {
      "kind": "repository",
      "reference": "apps/symphony/src/workspace.rs",
      "summary": "Current workspace bootstrap requires an explicit or repository default branch."
    }
  ],
  "next_action": "Ask the reporter to choose the required fallback behavior.",
  "clarification_question": "When a repository has no default branch, should Symphony fail setup or use the configured base branch?",
  "reproduction": {
    "attempted": true,
    "outcome": "The missing requirement prevents a definitive reproduction assertion."
  }
}
```

Contract:

- `schema_version`: integer, exactly `1`.
- `route`: one of `implement`, `spec`, `needs_information`, `park`, or `human_owned`.
- `risk_class`: one of `low`, `medium`, or `high`.
- `rationale`: non-empty string, at most 2,000 UTF-8 bytes.
- `evidence`: array of 1 to 20 objects.
- `evidence[].kind`: one of `issue`, `repository`, or `reproduction`.
- `evidence[].reference`: non-empty string at most 500 UTF-8 bytes. Repository references use repo-relative paths with an optional line range.
- `evidence[].summary`: non-empty string at most 1,000 UTF-8 bytes.
- `next_action`: non-empty string at most 1,000 UTF-8 bytes.
- `clarification_question`: string at most 1,000 UTF-8 bytes or `null`. It must be non-empty only for `needs_information` and must be `null` for every other route.
- `reproduction`: object or `null`.
- `reproduction.attempted`: boolean.
- `reproduction.outcome`: non-empty string at most 2,000 UTF-8 bytes.

The complete output file is limited to 64 KiB. Empty strings after trimming are invalid. Duplicate evidence entries are invalid after normalizing kind and reference.

## Configuration

A1 adds generic storage configuration and a triage section to `WORKFLOW.md`. Exact Rust type names may follow repository conventions, but the user-facing concepts are fixed by this spec.

```yaml
storage:
  path: $SYMPHONY_STATE_PATH
  busy_timeout_ms: 5000

triage:
  enabled: true
  mode: preview
  intake_label: needs-triage
  prompt: prompts/triage.md
  model: anthropic/claude-sonnet-4-6
  turn_timeout_ms: 900000
  max_attempts: 3
  max_intake_pages: 100
  routes:
    implement:
      label: ready-for-agent
      state: Todo
    spec:
      label: ready-to-spec
    needs_information:
      label: needs-info
    park:
      label: wait-to-implement
    human_owned:
      label: ready-for-human
```

### Configuration behavior

- `triage.enabled` defaults to `false` so existing workflows remain unchanged.
- `triage.mode` accepts `preview` or `automatic` and defaults to `preview` when triage is enabled.
- `triage.intake_label` defaults to `needs-triage`.
- `triage.prompt` resolves relative to the active `WORKFLOW.md` directory.
- Triage executes exactly one model turn. Escalation, steering, tracker refresh between turns, Symphony dynamic tools, and continuation turns are disabled.
- For Pi, model precedence is `triage.model`, then `agent.model`, then the harness default. No state- or label-based model mapping applies to triage.
- Codex uses its app-server harness default because the current Symphony Codex contract has no model parameter. Config validation rejects `triage.model` when `agent.name` is `codex`.
- `triage.turn_timeout_ms` defaults to `900000` and must be greater than zero.
- Every canonical route requires a non-empty label.
- A route state is optional and resolves through the configured Projects v2 status field.
- The implement route in starter configuration maps to `ready-for-agent` and `Todo`.
- Managed route labels must be distinct and cannot equal the intake label.
- `triage.max_attempts` defaults to `3` and must be greater than zero. Terminal failed attempts for one revision cannot exceed this value without a new issue revision.
- `triage.max_intake_pages` defaults to `100` and must be greater than zero. Reaching the cap before GitHub pagination completes fails the poll visibly and starts no attempts from a partial result set.
- `storage.path` accepts environment-variable indirection and tilde expansion.
- `storage.busy_timeout_ms` defaults to `5000` and must be greater than zero.
- When `storage.path` is absent, Symphony resolves a platform data directory namespaced by normalized forge host, GitHub owner, and repository and prints the resolved path in startup diagnostics.
- `symphony doctor` validates the database path, prompt, intake label, route labels, configured route states, and Projects v2 access. Missing labels or states are reported with exact remediation and block triage startup.
- `symphony init` writes the triage prompt and commented starter configuration. It does not create remote labels.

## Architecture

### Triage configuration

`ServiceConfig` gains typed storage and triage configuration. Parsing and validation remain in `config.rs`. Dynamic workflow reload may update prompts and mappings for new attempts, while an active or completed attempt and publication intent retain their recorded configuration revision and intake label. Disabling triage stops new intake but does not cancel durable attempts or publication reconciliation, and the implementation scheduler continues to guard every issue ID with a nonterminal publication intent.

### GitHub triage intake

A focused GitHub intake port queries open issues carrying the configured intake label without requiring an implementation-active state, paginating until exhaustion. It intersects those issues with membership in the configured GitHub Project.

An intake-labeled issue outside the project is ineligible. Symphony creates or updates its durable factory run as ineligible, reconciles a durable publisher-owned diagnostic-comment intent explaining that project membership is required, emits `triage_ineligible`, leaves the intake label unchanged, and does not create an agent stage attempt. The intent uses the same comment ID, author verification, marker, pagination, and create-before-record recovery contract as triage publication.

If pagination reaches `triage.max_intake_pages` before exhaustion, the entire intake poll fails visibly and creates no attempts from the partial result set.

The normalized input includes:

- issue ID and number;
- title and body;
- non-managed labels;
- assignee and milestone metadata when present;
- relevant issue conversation excluding Symphony's marked triage comment;
- created and updated timestamps;
- forge host and repository identity.

The generic implementation candidate query remains unchanged except for the dispatch guard defined under immediate implementation handoff.

### Durable factory run store

A storage interface owns persistence. Its first implementation uses SQLite with embedded migrations. The interface keeps SQL out of the coordinator and allows future storage decisions without changing triage behavior.

Minimum records:

- **factory run:** stable run ID, forge host, tracker/repository identity, issue ID, status, current stage, timestamps;
- **stage run:** run ID, stage, issue revision, attempt number, owner instance, PID, process-group ID, OS process-start token, executable identity, lease heartbeat, status, configuration revision, harness, nullable effective model, workspace/output paths, timing, usage, and an error of at most 2,000 UTF-8 bytes;
- **triage artifact:** schema version, route, risk, rationale, evidence, next action, question, and reproduction summary;
- **publication intent:** desired effects, observed baseline, current expected managed projection, completed steps, status, retry count, and a last error of at most 2,000 UTF-8 bytes;
- **factory event:** durable event ID, run/stage IDs, type, timestamp, and payload of at most 64 KiB.

SQLite uniqueness includes normalized forge host, repository, and issue ID. Constraints allow at most one nonterminal attempt and one successful artifact per `(issue_revision, configuration_revision)` while retaining multiple terminal failed attempts up to `triage.max_attempts`.

Symphony acquires an OS-released exclusive lock adjacent to the database before starting triage. Each process records an owner instance, renews active-attempt leases every 10 seconds, and treats a lease as stale after 60 seconds. A second Symphony process using the same store cannot start triage. Migrations execute while the exclusive lock is held and before triage starts.

### Triage coordinator

The coordinator is a separate unit from the implementation-heavy orchestration path. On each poll it:

1. fetches intake issues;
2. computes the triage-relevant revision;
3. claims or skips each revision transactionally;
4. prepares a disposable workspace;
5. invokes the triage runner;
6. validates output and repository cleanliness;
7. stores the artifact and publication intent;
8. asks the publisher to reconcile pending effects;
9. emits durable and live events.

The existing orchestrator owns scheduling cadence and global shutdown. The triage coordinator owns triage-specific state transitions.

Startup and each poll reconcile in this order: acquire lock and migrate storage; recover publisher-owned comment identities; reconcile pending diagnostic, preview, and automatic intents; then fetch and fingerprint intake. Fingerprinting never runs before a create-before-record comment window is resolved.

### Triage runner

A1 triage execution is local-only. The runner reuses the configured Pi or Codex harness through a triage-specific process profile. It executes exactly one turn with `triage.turn_timeout_ms`, uses Pi's documented triage model precedence or the Codex harness default, and disables continuation, escalation, steering, tracker refresh, Symphony dynamic tools, and implementation hooks.

A1's security boundary trusts the local Symphony account and runner binary. It guarantees that Symphony does not inject source-forge mutation capabilities into triage. It does not claim to prevent a malicious same-UID process from discovering host files or network credentials by other means.

Before spawn, Symphony:

- creates a clone-only, attempt-unique workspace with independent Git metadata and removes or disables every push URL;
- records the initial `HEAD` commit and submodule commits;
- creates an attempt-unique stage-output directory outside the clone;
- creates an isolated temporary `HOME`;
- clears the inherited environment;
- restores a runtime-specific allowlist for `PATH`, locale, temporary directory, model-provider authentication, and required harness configuration;
- omits `GH_TOKEN`, `GITHUB_TOKEN`, `SSH_AUTH_SOCK`, `GIT_ASKPASS`, Git credential variables, GitHub CLI configuration, `SYMPHONY_BIN`, `SYMPHONY_WORKFLOW_PATH`, and implementation helper variables;
- skips `after_create`, `before_run`, `after_run`, and `before_remove` lifecycle hooks;
- starts the child in its own process group.

If a harness requires file-backed provider authentication, Symphony copies the runtime-specific provider material needed to run the configured model into the isolated home. A doctor check proves the selected harness can authenticate under this profile before triage starts.

For Codex, the triage turn sandbox is derived from the configured policy with the clone and stage-output directory as its only stage-specific writable roots. Pi receives `SYMPHONY_STAGE_OUTPUT` directly. Fake-runner tests assert one turn, timeout, model selection, disabled escalation/dynamic tools, helper omission, and writable output transport for both harnesses.

The runner may inspect code and run reproduction commands. Ignored build products may be created. Completion requires unchanged `HEAD`, unchanged submodule commits, and no staged, unstaged, or untracked source entries. Integrity-check failure is a stage failure even when `git status` is clean after an agent-created commit.

The runner writes a maximum 64 KiB result to `SYMPHONY_STAGE_OUTPUT`. Symphony parses and validates that file; prose claims in model output do not complete the stage.

On timeout, cancellation, or shutdown, Symphony terminates the process group, waits up to 5 seconds before force termination, and removes the disposable workspace and isolated home. Late output is ignored unless the attempt remains nonterminal and its owner token matches.

### Deterministic route publisher

The publisher consumes a persisted artifact and desired route mapping. It is the only A1 component allowed to mutate GitHub.

Automatic mode reconciles these steps:

1. upsert a marked comment with publication status `pending`;
2. apply the desired route label;
3. apply the optional Projects v2 state through the existing GitHub state adapter;
4. remove other configured route labels;
5. update the marked comment to `route effects: applied; publication: pending`, describing the route effects that succeeded;
6. remove the intake label as the final routing mutation;
7. update the owned comment to `publication: applied`.

The route-applied event and durable publication status become final only after step 7. The scheduler's nonterminal-intent guard prevents implementation dispatch between steps 6 and 7. If step 6 or 7 fails, HTTP, events, and the durable intent remain pending and reconciliation resumes idempotently.

The publication intent stores the managed-label and project-state baseline observed before publication and an expected projection advanced after every observed-or-applied step. On reconciliation:

- current state equal to the expected projection means the step may proceed;
- current state already equal to the desired post-step projection means a prior GitHub success is recorded without repeating the mutation;
- current state different from both expected and desired projections is a human conflict.

This handles a crash after GitHub succeeds but before SQLite records the step.

Every Symphony triage comment contains `<!-- symphony:triage:{intent_id} -->`. The publication record stores the GitHub comment ID and authenticated publisher login. Updates fetch that comment ID and verify its author. Recovery from a create-before-record crash paginates comments and accepts only a matching intent marker authored by the authenticated publisher. A spoofed marker from any other author is ignored and remains part of the issue revision.

Preview mode creates a durable, idempotent preview-comment intent and reconciles it across restart using the same ownership contract. It keeps the intake label and changes no route label or state.

When configuration changes from preview to automatic, Symphony may promote the latest immutable preview artifact without rerunning the agent only when the current issue revision and route-mapping hash equal those recorded by the artifact. It creates a fresh automatic publication intent and conflict baseline. A changed issue revision or configuration revision creates a new triage attempt keyed by the new pair.

### HTTP and events

A1 adds two routes before the existing `/api/v1/{issue_identifier}` catch-all:

- `GET /api/v1/factory-runs/{run_id}` returns one run or `404` with the existing API error envelope.
- `GET /api/v1/factory-runs?issue={issue_identifier}` returns zero or one current run; a missing or invalid `issue` parameter returns `400`.

Run status is one of `active`, `waiting`, `completed`, `failed`, or `ineligible`. Stage status is one of `pending`, `running`, `completed`, `failed`, or `interrupted`. Publication status is one of `none`, `pending`, `applied`, `blocked`, or `conflict`.

Required response shape:

```json
{
  "run_id": "018f0f2c-1111-7000-8000-000000000001",
  "forge_host": "github.com",
  "repository": "example/widgets",
  "issue": {
    "id": "123",
    "identifier": "#123",
    "revision": "sha256-hex"
  },
  "status": "active",
  "current_stage": "triage",
  "created_at": "2026-07-16T17:00:00Z",
  "updated_at": "2026-07-16T17:01:00Z",
  "attempts": [
    {
      "stage_run_id": "018f0f2c-2222-7000-8000-000000000002",
      "attempt": 1,
      "status": "completed",
      "configuration_revision": "sha256-hex",
      "harness": "pi",
      "model": "anthropic/claude-sonnet-4-6",
      "started_at": "2026-07-16T17:00:01Z",
      "completed_at": "2026-07-16T17:00:31Z",
      "duration_ms": 30000,
      "usage": {
        "input_tokens": 1000,
        "output_tokens": 250,
        "total_tokens": 1250
      },
      "error": null
    }
  ],
  "artifact": {
    "artifact_id": "018f0f2c-3333-7000-8000-000000000003",
    "schema_version": 1,
    "route": "implement",
    "risk_class": "low",
    "rationale": "The issue defines one bounded documentation correction.",
    "evidence": [
      {
        "kind": "issue",
        "reference": "body",
        "summary": "The issue names the file and exact replacement."
      }
    ],
    "next_action": "Apply the specified correction.",
    "clarification_question": null,
    "reproduction": null,
    "received_at": "2026-07-16T17:00:30Z"
  },
  "publication": {
    "intent_id": "018f0f2c-4444-7000-8000-000000000004",
    "mode": "automatic",
    "status": "pending",
    "completed_steps": ["comment_pending", "route_label"],
    "route_label": "ready-for-agent",
    "project_state": "Todo",
    "retry_count": 0,
    "error": null
  }
}
```

`error`, when present, has required string fields `code`, `component`, and `remediation`, required boolean `retryable`, and optional string `publication_step`. Each string is limited to 2,000 UTF-8 bytes. Raw prompts, credentials, and unbounded model output are never returned.

The `model` field is a string or `null`. Pi records the configured effective model. Codex records `null` unless an app-server response supplies a verified effective model identity.

`GET /api/v1/factory-runs/metrics?stage=triage` returns a bounded aggregate over all retained A1 records with `total_attempts`, `completed_attempts`, `failed_attempts`, `ineligible_issues`, `route_counts`, `correction_count`, `correction_rate`, duration `average_ms`/`p50_ms`/`p95_ms`, and token totals grouped by harness and model. Null model identities group under the key `unknown`. A missing or non-`triage` stage returns `400`. Monetary cost remains unavailable until pricing attribution exists.

A1 adds `triage` to the existing event-envelope version. Event names are exactly `triage_started`, `triage_completed`, `triage_failed`, `triage_ineligible`, `triage_publication_started`, `triage_publication_blocked`, `triage_publication_conflict`, `triage_route_applied`, and `triage_route_corrected`.

```json
{
  "version": "v1",
  "sequence": 42,
  "timestamp": "2026-07-16T17:00:31Z",
  "kind": "triage",
  "severity": "info",
  "issue": "#123",
  "event": "triage_completed",
  "payload": {
    "run_id": "018f0f2c-1111-7000-8000-000000000001",
    "stage_run_id": "018f0f2c-2222-7000-8000-000000000002",
    "artifact_id": "018f0f2c-3333-7000-8000-000000000003",
    "publication_intent_id": "018f0f2c-4444-7000-8000-000000000004",
    "route": "implement",
    "status": "completed",
    "error_code": null
  }
}
```

All triage event payloads include `run_id`, `stage_run_id` when one exists, `status`, and `error_code`. Artifact, publication, and route fields are present when created. Live events continue through `EventHub`; the durable store retains the corresponding factory events.

## Completeness, issue revision, and idempotency

Every completeness-sensitive GitHub read uses pagination to exhaustion under `triage.max_intake_pages`: intake issues, Projects v2 membership, issue comments, and marked-comment recovery. Reaching the cap fails the complete intake, fingerprint, membership, or recovery operation. Symphony never reports a complete result from truncated data.

The issue revision is the lowercase hexadecimal SHA-256 digest of UTF-8 canonical JSON with fixed property order. It contains:

- exact title and body after CRLF-to-LF normalization, preserving other whitespace;
- non-managed label names normalized to lowercase and sorted lexicographically;
- assignee logins normalized to lowercase and sorted lexicographically;
- milestone number and exact title, using JSON `null` when absent;
- every issue comment except a verified Symphony comment, sorted by numeric GitHub comment ID and represented by comment ID, lowercase author login, exact body after CRLF-to-LF normalization, created time, and updated time.

Missing optional values serialize as JSON `null`; present empty strings remain empty strings. Arrays are present even when empty. A comment is excluded only when its persisted comment ID, intent marker, and authenticated publisher author all match. Marker text alone never excludes a comment.

The fingerprint excludes the intake label, configured route labels, Projects v2 state, verified Symphony triage comments, and update timestamps not attached to included content.

This prevents Symphony's own publication effects from causing retriage. A change to any included field creates a new issue revision. Configuration revision separately hashes the schema version, prompt content, one-turn execution settings, selected harness/model, and canonical route mapping.

A1 does not add a force-retriage command. A no-content-change reapplication of `needs-triage` reconciles the existing `(issue_revision, configuration_revision)` or remains a no-op.

## GitHub effect consistency

GitHub cannot apply comments, labels, and project state in one transaction. A1 provides recoverable consistency:

- the desired publication is committed locally before any GitHub effect;
- each external step is reconciled against its persisted expected and desired projections and recorded after observed or applied success;
- the intake label remains until all configured effects complete;
- a failed or restarted process resumes at the first unfinished step;
- a run is not reported route-applied until every step completes;
- permanent failures leave the issue visibly in intake and expose operator remediation.

Before each effect, the publisher reads current managed labels and Projects v2 state. A value different from both the persisted expected and desired projections stops publication and records a conflict rather than overwriting the human decision.

## Immediate implementation handoff

The default implement mapping applies `ready-for-agent` and moves the issue to `Todo`. The implementation candidate scheduler must reject every issue ID with a durable nonterminal automatic publication intent, independent of the currently loaded `triage.enabled`, intake label, or route mapping. It also rejects the intake label recorded by that intent. The scheduler discovers the issue only after publication becomes final and the recorded intake label is absent.

A1 does not invoke implementation directly. Tracker state plus absence of the intake label remains the handoff boundary, preserving existing dependency, assignment, concurrency, and retry rules.

Spec, needs-information, park, and human-owned routes have no active state in starter configuration and remain ineligible for implementation.

## Human correction measurement

After automatic routing, a correction reconciler queries the durable set of latest published artifacts and fetches those GitHub issues by ID independently of intake and implementation candidate queries. It compares current labels with the five route labels stored on that publication intent. A live mapping reload cannot reinterpret an older publication; changed mappings apply only to a new artifact/publication pair.

- The same route remains an agreement observation.
- One different configured route records a single `route_corrected` durable and live event, unique by artifact ID and observed corrected route.
- Missing or multiple route labels record a route-consistency diagnostic, not an agreement result.
- A new triage attempt resets comparison to its latest artifact.

Route correction is a measurable proxy for disagreement. A1 does not claim that absence of correction proves correctness.

## Error handling

### Intake and persistence

- A GitHub intake failure records a runtime error and leaves issues unchanged.
- The coordinator must persist the run and active attempt before agent launch.
- SQLite unavailable, corrupt, locked beyond `storage.busy_timeout_ms`, or migration failure blocks triage execution and publication.

### Agent execution

These conditions fail the stage and create no publication intent:

- process start or timeout failure;
- missing output file;
- malformed JSON;
- schema-version mismatch;
- unknown route or risk class;
- empty rationale or evidence;
- missing clarification question for `needs_information`;
- changed `HEAD`, submodule commit, staged files, unstaged files, or untracked source files;
- cancellation or unexpected shutdown.

The issue keeps its intake label. Retry follows bounded backoff and creates a new attempt number for the same revision after the prior attempt is terminal, up to `triage.max_attempts`. Exhaustion leaves a durable failed stage with remediation and requires a changed issue revision for another automatic attempt.

### Route publication

- Transient GitHub errors keep the publication pending and retry unfinished steps.
- Missing labels, invalid states, authorization failure, and repository mismatch block publication with exact remediation.
- Human mutation conflicts stop publication and record a conflict.
- The publisher never removes the intake label after a blocked or incomplete publication.

### Restart

- Symphony acquires the exclusive store lock and creates a new owner instance.
- Nonterminal attempts owned by a prior instance become interrupted after their lease is stale.
- Recorded process groups receive bounded termination only when PID, process-group ID, OS process-start token, and executable identity all match; otherwise Symphony skips signaling and relies on owner-token rejection and attempt cleanup. Late output cannot complete an interrupted attempt.
- Attempt-unique workspaces, isolated homes, and output paths are cleaned before retry.
- Durable preview and automatic publication intents resume from their expected projection.
- An interrupted stage may retry up to `triage.max_attempts`; it is never silently marked successful.
- Completed artifacts remain immutable.

## Security and trust boundaries

- Issue bodies, comments, repository content, model output, and tool output are untrusted inputs.
- The local runner and host account are trusted. Symphony clears the environment, isolates `HOME`, uses a runtime-specific allowlist, creates an independent clone with no push URL, and injects no GitHub, Git/SSH, Symphony-helper, or lifecycle-hook mutation capability. These are defense-in-depth controls rather than an OS sandbox guarantee.
- Only schema-validated artifacts reach the publisher.
- Only configured labels and states can be applied; model-provided label or state strings are ignored.
- GitHub credentials stay in the adapter/publisher process and are redacted from logs and API output.
- Evidence and errors use bounded lengths before persistence and display.
- The workspace is disposable and path-contained by existing workspace safety rules.
- Changed `HEAD`, submodules, index, worktree, or untracked source files fail the stage and cannot enter implementation.
- SQLite statements use bound parameters and migrations bundled with the binary.

## User-visible behavior

### Preview mode comment

The marked comment shows:

- proposed route;
- risk class;
- rationale;
- evidence summary;
- next action;
- clarification question when present;
- factory run and attempt IDs;
- statement that no labels or state were changed.

### Automatic mode comment

The first marked comment shows publication status `pending`, proposed route and next action, risk class, rationale and evidence summary, clarification question when present, and factory run, attempt, and intent IDs.

After route label, project state, and conflicting-label cleanup succeed, the publisher updates the same owned comment to `route effects: applied; publication: pending` and lists the applied label and state. Intake-label removal is the final routing mutation. A final idempotent update changes the owned comment to `publication: applied`; the durable intent, HTTP, events, and implementation handoff remain pending until that comment update succeeds.

Repeated publication updates the same publisher-owned comment ID.

### Ineligible project membership

An intake-labeled issue outside the configured GitHub Project receives one marked diagnostic comment naming the required project and remediation. It remains untriaged, retains `needs-triage`, and appears as ineligible through events and the factory-run query surface.

### Startup and doctor

Startup output identifies whether triage is disabled, preview, or automatic and prints the resolved SQLite path. Doctor reports separate checks for storage, exclusive lock, isolated harness authentication, prompt, intake label, route labels, Projects v2 membership access, and route states.

## Delivery slices

### Pull request 1: triage preview

**Shipped** in [#587](https://github.com/gannonh/kata-symphony/pull/587) (2026-07-18).

Delivered:

- storage and triage configuration;
- SQLite migrations and durable records;
- GitHub intake polling (Projects v2 membership by repository + number);
- repository-backed Pi/Codex runner and artifact validation;
- durable preview-comment intent and restart-safe reconciler (including orphaned-artifact recovery);
- lease renewal during long-running turns;
- isolated env/home with credential scrub after success;
- minimal HTTP factory-run read surface and live triage session visibility;
- starter prompt, doctor checks, docs, and focused tests.

User value: maintainers can evaluate repository-backed triage decisions safely and establish a baseline without changing routing.

### Pull request 2: automatic route publication

Delivers:

- automatic mode and preview-artifact promotion;
- configurable route labels and states;
- deterministic outbox publisher with expected-projection crash recovery;
- conflicting-label cleanup;
- human-mutation conflict detection;
- immediate implementation handoff;
- automatic-routing UAT.

User value: selected repositories automatically route issues and send implementation-ready work into the existing Symphony flow.

### Pull request 3: recovery and agreement measurement

Delivers:

- interrupted agent-process recovery and attempt cleanup;
- post-publication issue reconciliation by durable issue ID;
- route correction and consistency events;
- agreement metrics in the minimal read surface;
- restart and correction UAT.

User value: operators can run triage continuously, recover safely, and measure model usage and maintainer disagreement.

No pull request may merge with a hidden foundation-only outcome. Each slice must satisfy its stated user workflow independently.

Merge gates by slice:

- Pull request 1 must satisfy criteria 1 through 8, the preview-relevant portions of 14, 15, 17, and the preview and off-project portions of 18.
- Pull request 2 must satisfy criteria 9 through 13, the automatic-publication portions of 14, 15, 17, and the promotion and implementation-handoff portions of 18.
- Pull request 3 must satisfy the restart, retry, correction, and agreement portions of criteria 4, 14 through 18.

## Acceptance criteria

1. `WORKFLOW.md` can enable Projects v2 triage, select `preview` or `automatic`, configure `needs-triage`, set bounded attempts and intake pages, and map all five canonical outcomes to distinct labels and optional project states. Starter defaults use `ready-for-agent` plus `Todo`, `ready-to-spec`, `needs-info`, `wait-to-implement`, and `ready-for-human`.
2. A focused GitHub query paginates open `needs-triage` issues to exhaustion and intersects them with the configured Project without requiring an implementation-active state. Reaching `triage.max_intake_pages` fails the whole poll visibly and starts no attempts from partial results.
3. An intake-labeled issue outside the configured Project receives one idempotent diagnostic comment, retains `needs-triage`, emits `triage_ineligible`, and starts no agent attempt.
4. Before agent execution, Symphony acquires the exclusive store lock and persists one factory run and one nonterminal attempt for the normalized forge-host, repository, issue revision, and configuration revision. Restart preserves the records; multiple terminal failed attempts are allowed up to `triage.max_attempts`, with at most one successful artifact per `(issue_revision, configuration_revision)`.
5. A1 documents the local runner and host account as trusted. Symphony uses clone-only workspaces with no push URL and injects no GitHub token or CLI configuration, Git/SSH credential variables or sockets, Symphony helper environment, or lifecycle hooks. Child-process tests prove those values are absent from Symphony's injected environment and that the clone cannot push through a configured remote.
6. Pi and Codex each execute exactly one triage turn, enforce `triage.turn_timeout_ms`, disable continuation/escalation/steering/dynamic tools, and write through the harness-approved `SYMPHONY_STAGE_OUTPUT`. Pi uses `triage.model`, then `agent.model`, then harness default; Codex uses its app-server default and rejects `triage.model` during config validation. The process receives the exact schema-version-1 contract and produces valid JSON of at most 64 KiB. Unknown fields, invalid enum values, out-of-bound strings or arrays, duplicate evidence, and a missing or extraneous clarification question fail validation. Symphony records timestamps.
7. Completion requires unchanged `HEAD` and submodule commits plus no staged, unstaged, or untracked source entries. Tests prove that agent-created commits, staged changes, ordinary worktree changes, and untracked source files fail the attempt, while ignored build output does not.
8. In preview mode, a persisted preview-comment intent upserts exactly one publisher-owned marked proposal comment containing route, risk, rationale, evidence, next action, and any clarification question; retains `needs-triage`; changes no route label or project state; and reconciles idempotently after a crash or restart.
9. Changing from preview to automatic creates an automatic publication intent from the immutable preview artifact only when its issue revision and route-mapping hash still match current state. A changed revision or mapping creates a new attempt instead of publishing stale intent.
10. In automatic mode, a successful artifact and publication intent persist before GitHub effects. The publisher upserts a `pending` comment, applies exactly one route label, applies the configured Projects v2 state, removes conflicting managed route labels, updates the comment to `route effects: applied; publication: pending`, removes `needs-triage` as the final routing mutation, then updates the owned comment to `publication: applied`. HTTP, events, and implementation handoff remain pending until the final comment update succeeds.
11. Publication persists its observed baseline, expected managed projection, intent-specific comment marker, comment ID, and authenticated publisher. A crash after any GitHub success but before local step recording reconciles without duplication; spoofed markers are ignored; and a managed value different from both expected and desired projections records a human conflict and stops.
12. The implementation scheduler rejects every issue ID with a nonterminal automatic publication intent, independent of live configuration, and rejects the intent's recorded intake label. The default implement route becomes eligible only after publication is final; tests disable triage or change its intake label after every publication step and prove no worker dispatch occurs.
13. Needs information publishes the artifact's exact question. Spec, park, and human-owned starter routes have no active project state and remain ineligible for implementation.
14. Agent failure, schema failure, integrity failure, exhausted retries, project-membership failure, or GitHub publication failure produces a durable failed, ineligible, or blocked record. HTTP and events expose an error code, failing component or step, retryability, and remediation, each bounded to 2,000 UTF-8 bytes. An incomplete publication is never reported route-applied.
15. `GET /api/v1/factory-runs/{run_id}` and `GET /api/v1/factory-runs?issue={issue_identifier}` return the exact run schema and status enums defined by this spec, with nullable `model`; missing runs return `404`, and missing or invalid issue filters return `400`. `GET /api/v1/factory-runs/metrics?stage=triage` returns route, correction, latency, failure, ineligible, harness/model, and token aggregates, grouping null models under `unknown`, or `400` for another stage.
16. Started, completed, failed, publication, route-applied, ineligible, conflict, and route-corrected events are emitted live and durably. A correction reconciler fetches published issues by durable ID after intake removal, compares against the publication intent's recorded five-label mapping, and records a correction once per artifact and observed corrected route.
17. Automated tests cover configuration including Pi model precedence and Codex `triage.model` rejection, one-turn execution, timeout/output behavior, schema bounds, storage locking/migrations/recovery, complete pagination and cap failure for issues/project items/comments, project membership, diagnostic and triage comment ownership/spoofing/create-before-record ordering, preview recovery/promotion, every automatic route, crash windows around every GitHub effect, canonical issue revisions, non-injected capabilities, repository integrity, exact process-identity signaling, dynamic reload dispatch guards, recorded-mapping correction reconciliation, human conflicts, retry exhaustion, exact HTTP/event contracts, metrics, and implementation handoff.
18. Manual GitHub UAT uses two documented fixtures: a single-file documentation typo with an exact replacement expected to route `implement`, and an underspecified performance request expected to route `needs_information` with a non-empty question. Evidence shows preview, promotion to automatic, implementation eligibility only after intake removal, process restart, durable API records, one human route correction, and the off-project diagnostic.

## Testing strategy

### Unit tests

- Configuration defaults, environment resolution, validation, and route uniqueness.
- Route and risk parsing.
- Artifact schema and required-field validation.
- Issue revision fingerprint inclusion and exclusion rules.
- SQLite migrations, busy timeout, exclusive locking, constraints, owner leases, attempts, immutable artifacts, and expected-projection outbox transitions.
- Comment rendering and every field-size bound.
- Repository integrity checks for `HEAD`, submodules, staged, unstaged, untracked, and ignored files.

### Integration tests

- Mock GitHub Projects v2 intake issues, project items, and issue comments across multiple pages, with cap failure in every path and off-project issues.
- Publisher-owned preview comment upsert, spoofed-marker handling, create-before-record recovery, and preview-to-automatic promotion.
- Every automatic route mapping.
- Optional Projects v2 state updates for on-project issues.
- Managed-label conflict cleanup.
- Human mutation conflict.
- Transient and permanent failures plus the GitHub-success/local-recording crash window at every publication step.
- Process restart with an active attempt, durable preview intent, and automatic publication intent.
- Existing implementation scheduler exclusion during nonterminal publication across triage disable/intake-label reload, then discovery after final publication.
- Exact factory-run HTTP responses, metrics aggregates, and event envelopes.

### Runner tests

Use fake Pi and Codex commands to exercise:

- one-turn execution, Pi model precedence, Codex `triage.model` rejection, timeout, disabled escalation/dynamic tools, and output-root transport;
- valid artifact;
- malformed artifact;
- missing artifact;
- timeout and process exit;
- committed, staged, unstaged, untracked, and submodule source mutation;
- ignored build artifacts;
- cleared-environment and isolated-home capability assertions;
- process-group termination and ignored late output;
- token and timing capture.

### Manual UAT

Use a dedicated GitHub test repository or disposable issues in an approved UAT repository. Capture issue URLs, API responses, Symphony logs, and restart evidence. Clean up route labels, comments, issues, and workspaces after proof collection.

### Quality gate

Run from `apps/symphony` or through the monorepo scripts as appropriate:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

The Build phase must also run the repository's affected-package validation required by CI.

## Likely file map

New focused modules are expected for:

- durable factory-run storage and migrations;
- triage domain/coordinator;
- triage runner and artifact validation;
- deterministic GitHub route publication.

Existing areas likely affected:

- `apps/symphony/src/domain.rs`
- `apps/symphony/src/config.rs`
- `apps/symphony/src/workflow_store.rs`
- `apps/symphony/src/orchestrator.rs`
- `apps/symphony/src/github/adapter.rs`
- `apps/symphony/src/github/client.rs`
- `apps/symphony/src/event_stream.rs`
- `apps/symphony/src/http_server.rs`
- `apps/symphony/src/doctor.rs`
- `apps/symphony/src/starter_assets.rs`
- `apps/symphony/prompts/`
- `apps/symphony/docs/WORKFLOW-REFERENCE.md`
- `apps/symphony/tests/`

The Build phase should confirm exact module names after tracing current runner construction and HTTP route composition. It should keep persistence, triage coordination, artifact validation, and GitHub publication as separate interfaces even if file names differ.

## Risks and mitigations

### A1 becomes a general workflow engine

Keep one hard-bounded triage stage and canonical route enum. Reuse scheduling and runner primitives without introducing arbitrary stage graphs.

### SQLite becomes coupled to orchestration

Depend on a narrow run-store interface. Keep migrations and SQL inside the storage module.

### GitHub effects remain partially visible during transient failure

Keep `needs-triage` until every configured effect succeeds, record each completed outbox step, and report route-applied only after completion.

### Repository-backed triage mutates code

Use a disposable workspace with an isolated process profile, record initial commits, require unchanged commits and source status, terminate the process group, and delete the workspace and isolated home.

### Trusted local runner can access host resources

Document the same-UID trust boundary, use clone-only workspaces and non-injected credentials, and reserve hostile-runner isolation for the Docker/cloud execution slice.

### Triage retriggers itself

Use canonical SHA-256 issue and configuration revisions. Exclude managed route effects and only exclude comments verified by stored ID, intent marker, and publisher author.

### Human changes race publication

Persist and advance the expected managed projection around every effect. Stop and record a conflict only when current state differs from both expected and desired projections.

### Preview mode is treated as full automation

Render an explicit preview marker and keep all labels and states unchanged. Documentation and startup output identify the active mode.

### Correction rate is treated as complete accuracy

Describe correction as a disagreement proxy and retain sampled human review as the baseline method.

## Explicitly deferred work

- Linear adapter and vocabulary mapping.
- GitHub label-state configuration and triage.
- Automatic GitHub Project membership.
- Docker and SSH triage execution.
- GitHub webhook ingestion.
- Spec generation and approval.
- Confidence thresholds and automatic policy by risk class.
- Authenticated remote factory-run APIs.
- Full control-room history and visual stage board.
- Cross-repository and organization views.
- A force-retriage control.
- Durable shared memory and learning proposals.

## Build handoff

### Approved scope

Build A1 as three user-facing vertical pull requests: preview, automatic routing, then recovery and agreement measurement. Preserve the final acceptance criteria across the stack.

### Non-negotiable constraints

- GitHub Projects v2 first, with project membership required.
- Polling intake through `needs-triage`, paginated to exhaustion or failed visibly.
- SQLite durability before agent execution.
- Local clone-only repository-backed triage under a trusted-runner model, with no GitHub/helper mutation capability injected by Symphony.
- One-turn Pi/Codex execution with explicit model, timeout, output, and disabled-tool behavior.
- Structured artifact validation before privileged effects.
- Deterministic, idempotent publication with intake-label removal last.
- Durable nonterminal publication intents guard the existing implementation scheduler across configuration reload.
- Publisher-owned comments and expected projections protect idempotency and human edits.
- Existing implementation scheduler remains the implement handoff mechanism.
- Every pull request has a tracker-visible demo and durable evidence.

### Build sequence

1. Trace runner construction, workspace lifecycle, GitHub label/state operations, HTTP route composition, and workflow reload behavior.
2. Implement preview mode end to end, including durability, runner, comment, API, events, doctor, and UAT.
3. Review preview metrics and failure evidence before enabling automatic mode.
4. Implement automatic route publication and immediate handoff end to end.
5. Implement restart recovery and correction measurement end to end.
6. Run focused tests after each slice and the full quality gate before completion.
7. Update OKF roadmap/logs and Symphony reference documentation after each merged slice.

### Verification contract

Build completion requires:

- all eighteen acceptance criteria passing or explicitly blocked with evidence;
- focused unit and integration suites;
- full Rust format, Clippy, and test gates;
- real GitHub UAT evidence for preview, auto-route, clarification, correction, and restart;
- an adversarial code review with no unresolved blockers;
- a completion report listing commits, commands, evidence paths, and residual risks.

### Blocking conditions

Stop Build and request a decision if:

- Pi and Codex cannot share a safe structured-output contract without broad runner redesign;
- Codex cannot use its harness default while config validation reliably rejects unsupported `triage.model`;
- Pi or Codex cannot run with the specified cleared environment and without Symphony-injected GitHub/helper mutation capabilities;
- Projects v2 membership or state reads cannot support the required intake and expected-projection checks;
- SQLite placement or lifecycle conflicts with supported deployment modes;
- a user-facing slice would require a foundation-only pull request;
- an acceptance criterion requires expanding into A2 or Horizon B work.
