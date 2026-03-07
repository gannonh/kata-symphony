# Build-Time Harness Evidence Contract Design

## Context

- Goal: strengthen the build-time harness in this repository so agents cannot satisfy documentation expectations with shallow timestamp-only updates.
- Primary transfer target: apply the learnings later to Symphony runtime behavior, but optimize this design for the repo harness first.
- Observed failure mode: agents update `Last reviewed` or touch an arbitrary doc to satisfy the current docs-sync gate without leaving a useful decision trail.

## References Reviewed

- OpenAI article: `https://openai.com/index/harness-engineering/`
- `docs/references/harness-engineering.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `AGENTS.md`
- `SPEC.md`
- `ARCHITECTURE.md`
- `WORKFLOW.md`
- `QUALITY_SCORE.md`
- `RELIABILITY.md`
- `SECURITY.md`
- Existing harness scripts under `scripts/harness/`

## Problem Statement

The current harness checks that documentation exists, that code changes are accompanied by some doc change, and that key markdown files contain a `Last reviewed` header. Those checks improve hygiene, but they do not establish whether:

1. the right context was loaded for the work,
2. the right canonical docs were updated,
3. the documentation update is substantive,
4. the change has a legible decision trail, or
5. the claimed verification evidence exists.

As a result, the repository currently enforces presence more strongly than relevance.

## Assumptions

1. The repo should enforce stronger process only on non-trivial or architecture-sensitive changes, not every typo or tiny edit.
2. Agents will optimize for whatever is easiest to satisfy mechanically, so the harness must reward evidence-bearing behavior and reject shallow substitutes.
3. Skills should shape behavior early, while hooks and scripts should enforce the contract later.
4. The first milestone should focus on build-time controls in this repo rather than runtime orchestration features inside Symphony.

## Options Considered

### 1. Stricter freshness and docs-sync checks only

- Extend the existing shell checks to reject metadata-only updates and require more docs when code changes.
- Pros: minimal lift, fits the existing harness.
- Cons: still encourages agents to game the checker with minimally expanded text and does not solve context-loading or decision-legibility gaps.

### 2. Evidence-backed repo contract (selected)

- Require a linked chain from intent to decision to code to verification to canonical-doc updates.
- Use skills to guide agents, scripts to define policy, and hooks to enforce it.
- Pros: directly addresses shallow compliance, creates auditability, and can evolve into stronger repo governance over time.
- Cons: introduces new artifacts and requires path-to-doc ownership rules.

### 3. Full repo operating system for agents

- Add context packs, subsystem ownership registries, periodic auditors, and broad rule engines immediately.
- Pros: strongest long-term control surface.
- Cons: too much scaffolding up front; risks creating process overhead before the core contract is proven.

## Selected Approach

Adopt an evidence-backed repo contract in stages. Replace the current `docs changed` expectation with an auditable requirement that meaningful code changes leave behind a traceable chain:

`intent -> decision -> implementation -> verification -> canonical docs`

The build-time harness should reject changes that cannot show that chain or that attempt to satisfy it with metadata-only updates.

## Contract Model

### Required artifact types

Non-trivial changes should produce four linked artifacts:

1. `Decision artifact`
   - A design doc or implementation plan describing what is changing and why.
2. `Impact artifact`
   - A small machine-readable manifest describing the affected areas, such as architecture, workflow, observability, security, reliability, tests, and docs.
3. `Verification artifact`
   - Concrete verification output showing commands run, relevant results, and residual risks.
4. `Freshness artifact`
   - Updates to the canonical docs that actually own the changed subsystem, or an explicit waiver explaining why no update was required.

### Proposed artifact location

Add a new generated evidence area, for example:

- `docs/generated/change-evidence/<date>-<topic>.md`
- `docs/generated/change-evidence/<date>-<topic>.json`

The markdown file is the human-readable summary. The JSON file is the machine-readable input for hooks and scripts.

### Valid compliance

A valid evidence chain should answer:

- What changed?
- Which subsystem does it affect?
- Which source-of-truth docs were loaded?
- Which source-of-truth docs were updated?
- What tests or checks were run?
- What risks remain?

### Invalid compliance

The harness should reject:

- timestamp-only doc updates,
- canonical-doc updates unrelated to the changed subsystem,
- unverifiable claims of testing,
- missing rationale for skipping doc updates, and
- architecture-sensitive changes with no linked decision artifact.

## Enforcement Stack

### Skills

Skills should front-load the contract so agents start work with the right expectations.

1. Update `brainstorming` and `writing-plans` outputs to include a `doc impact assessment`.
2. Update `executing-plans` to initialize and maintain a change-evidence artifact during execution.
3. Update `verification-before-completion` so it produces verification evidence instead of only reminding the agent to run checks.
4. Add a small repo-specific skill such as `symphony-harness-evidence` to teach:
   - when evidence is required,
   - how to map changed files to owning docs,
   - what a valid waiver looks like,
   - what counts as sufficient verification.

### Hooks

Hooks should provide fast local feedback plus a hard stop before publication.

1. `pre-commit`
   - Reject metadata-only canonical-doc edits.
   - Reject required docs that are missing decision or impact sections.
   - Reject qualifying code changes that lack an evidence manifest.
2. `pre-push`
   - Run the full evidence contract checks.
   - Validate cross-links between code changes, decision docs, verification docs, evidence files, and canonical docs.

### Scripts

Scripts should remain the policy source of truth.

Add new harness checks such as:

1. `scripts/harness/check_evidence_contract.sh`
2. `scripts/harness/check_doc_relevance.sh`
3. `scripts/harness/check_doc_coverage.sh`
4. `scripts/harness/check_decision_links.sh`
5. `scripts/harness/generate_change_evidence.ts`

These scripts should test relevance and traceability, not just file presence.

## Controls By Concern Area

### 1. Observability

Treat repo work as auditable operations. Each meaningful change should emit structured build-time telemetry through the evidence artifact:

- issue or topic
- files changed
- docs deemed relevant
- docs updated
- waivers used
- checks run
- unresolved risks
- skill flow used

This creates audit telemetry for the harness itself and allows periodic reporting on recurring failure modes.

### 2. Context relevance and freshness

Replace freshness-by-timestamp with freshness-by-traceability.

A canonical doc update is valid only if it includes at least one of:

- a linked design or implementation plan,
- a linked verification artifact,
- a linked issue or evidence record, or
- a substantive diff outside metadata.

Add drift checks:

- repeated subsystem changes without owning-doc updates,
- docs whose claimed structure no longer matches the code tree,
- orphaned design docs or plans with no downstream verification trail.

### 3. Context loading

Introduce a context routing manifest, for example `docs/harness/context-map.yaml`, that maps file globs to required context.

Examples:

- `src/config/**` -> `SPEC.md`, `ARCHITECTURE.md`, config design docs
- `src/execution/workspace/**` -> `SPEC.md`, `SECURITY.md`, `RELIABILITY.md`
- `src/orchestrator/**` -> `SPEC.md`, `ARCHITECTURE.md`, `RELIABILITY.md`
- `WORKFLOW.md` -> `SECURITY.md`, `docs/references/harness-engineering.md`

The evidence artifact should declare which required context files were loaded or explicitly waived.

### 4. Agent legibility

Require a short, stable decision schema rather than verbose free-form reasoning.

Recommended fields:

- `task`
- `assumptions`
- `constraints`
- `chosen approach`
- `alternatives rejected`
- `docs impacted`
- `verification plan`
- `residual risks`

Require this schema only when a change crosses defined thresholds, such as architectural directories or a multi-file implementation change.

### 5. Enforcing architecture and taste

Encode ownership and review rules directly in the harness.

1. Define which docs own which subsystems.
2. Define which paths count as architecture-sensitive.
3. Require design linkage for architecture-sensitive changes.
4. Require security and reliability review for workflow, hook, and workspace-safety changes.
5. Require spec alignment for user-facing contract changes.

Avoid vague taste rules. Instead, codify repo-specific heuristics an agent can follow and a reviewer can inspect, such as:

- keep public adapter surfaces narrow,
- avoid leaking low-level transport details across boundaries,
- prefer typed config access to ad hoc object reads,
- avoid duplicate utility paths,
- update the doc map when adding new durable documentation categories.

## Phased Rollout

### Phase 1: Replace shallow compliance

- Add `pre-commit`
- Add `check_evidence_contract.sh`
- Add `check_doc_relevance.sh`
- Require a change-evidence artifact for architecture-sensitive or multi-file changes
- Update core skills to initialize and maintain the artifact

Success criterion:

- a code change paired only with metadata-only doc updates fails consistently.

### Phase 2: Add context routing

- Introduce `context-map.yaml`
- Require evidence artifacts to declare loaded context and waivers
- Add checks for path-to-doc mismatches

Success criterion:

- for a given subsystem diff, the harness can explain which docs should have been read and whether they were updated or waived.

### Phase 3: Add decision legibility

- Standardize the decision schema across design docs, plans, and evidence records
- Enforce design linkage for architecture-sensitive changes
- Require alternatives considered only above defined thresholds

Success criterion:

- reviewers can reconstruct why a change happened and which docs now reflect reality.

### Phase 4: Add drift and governance loops

- periodic stale-doc audit
- periodic orphaned-design/orphaned-plan audit
- subsystem drift reports
- recurring summary of the most common harness violations

Success criterion:

- the repo can identify where its own harness is weak without waiting for ad hoc human review.

## Scope Guidance

Start with strong gates only on the highest-risk surfaces:

- `WORKFLOW.md`
- `SPEC.md`
- `ARCHITECTURE.md`
- `SECURITY.md`
- `RELIABILITY.md`
- `src/orchestrator/**`
- `src/execution/**`
- `src/config/**`

Do not require the full contract for every trivial edit. Narrow, high-signal gates reduce paperwork incentives and keep the harness credible.

## Risks and Mitigations

1. Risk: agents learn to game the new artifact format.
   - Mitigation: check semantic relevance, linked evidence, and metadata-only diffs rather than file presence.
2. Risk: the harness becomes too heavy for small changes.
   - Mitigation: scope strict enforcement to architecture-sensitive or multi-file changes first.
3. Risk: ownership mappings drift from the codebase.
   - Mitigation: add periodic context-map and stale-doc audits in the later rollout phases.
4. Risk: skill guidance and hook policy diverge.
   - Mitigation: keep scripts as the policy source of truth and treat skills as client guidance for that policy.

## Handoff

This design defines an evidence-backed build-time harness for the repository. Its purpose is to make documentation, verification, and context updates auditable and relevant, so the repo can teach better agent behavior now and transfer the resulting patterns into Symphony later.
