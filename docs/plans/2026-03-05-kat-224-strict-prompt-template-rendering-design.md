# KAT-224 Strict Prompt Template Rendering Design

## Context

- Ticket: `KAT-224`
- Goal: Implement strict prompt construction contract from `SPEC.md` Section 5.4 and Section 12.
- Project/Epic context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M1 Foundations & Contract Loading`
  - Linear docs: `Project Spec`, `Symphony v1 Execution Plan (Dependency DAG)`
- Dependency status:
  - Blocker `KAT-221` is `Done`
  - This ticket blocks `KAT-225`, `KAT-228`, `KAT-229`
- Issue-level attachments/documents:
  - None attached directly to `KAT-224`; contract source is `SPEC.md` + project docs.

## References Reviewed

- `SPEC.md` Section 5.4 (Prompt Template Contract)
- `SPEC.md` Section 5.5 (workflow/template error classes and dispatch gating)
- `SPEC.md` Section 12 (prompt construction, retry/continuation, failure semantics)
- `SPEC.md` Section 17.1 and 18.1 conformance bullets for strict prompt rendering
- `WORKFLOW.md` current template body and variable usage (`issue.*`, `attempt`)
- `ARCHITECTURE.md` layer map and runtime flow
- `src/domain/models.ts` (`Issue`, `WorkflowDefinition`, `RunAttempt` contracts)
- `src/execution/contracts.ts` (`AgentRunner.runAttempt(issue, attempt)` input seam)

## Problem Statement

Current scaffolding defines domain contracts but does not yet implement prompt rendering behavior. `KAT-224` must establish a strict, typed rendering contract so execution code can safely build prompts and surface failures predictably.

The implementation must enforce:

1. Strict template semantics for variables and filters.
2. Only supported template inputs (`issue`, `attempt`).
3. Explicit empty-body fallback behavior.
4. Continuation/retry semantics for `attempt` in first run and subsequent runs.
5. Typed prompt errors compatible with orchestrator retry/error handling.

## Assumptions

1. Rendering runs in the execution layer (worker attempt path) and is pure/deterministic given `(workflow.prompt_template, issue, attempt)`.
2. `attempt = null` indicates first attempt; positive integers indicate continuation/retry paths, consistent with existing domain contracts.
3. Unknown variable and unknown filter errors should be classified as `template_render_error` and include structured metadata.
4. Workflow read/YAML parse failures remain outside this ticket (handled by workflow/config validation tickets), but this design preserves their typed names for integration.

## Options Considered

1. Liquid-compatible engine with strict mode enabled (recommended)
   - Use a Liquid-compatible library configured to fail on undefined variables and undefined filters.
   - Pros: direct conformance to spec language, low custom parser risk, supports nested issue objects/collections.
   - Cons: introduces one runtime dependency and requires careful error normalization.

2. Hand-rolled template interpolation (`{{ issue.x }}` parser)
   - Build a minimal parser supporting variable interpolation and simple loops/conditionals as needed.
   - Pros: zero external dependency, full control of error messages.
   - Cons: high implementation and correctness risk; likely under-specifies template features and diverges from Liquid semantics.

3. Two-pass validation + permissive render engine
   - Pre-scan template AST for unsupported variables/filters, then render with a permissive engine.
   - Pros: custom diagnostics possible.
   - Cons: duplicated semantics and drift risk between validator and renderer; more complexity than strict-mode engine.

## Selected Approach

Adopt **Option 1**: a Liquid-compatible renderer in strict mode with a Symphony-owned wrapper that normalizes all parse/render failures into typed prompt errors.

This yields the best conformance/complexity ratio and cleanly supports retry/continuation prompt variants via the `attempt` variable.

## Proposed Contract

### 1. Prompt Builder API

Introduce execution-layer prompt contract (module name illustrative):

- `buildPrompt(input): PromptBuildResult`
- Input:
  - `template: string`
  - `issue: Issue`
  - `attempt: number | null`
- Output:
  - success: `{ prompt: string }`
  - failure: typed `PromptBuildError`

### 2. Typed Error Surface

Define typed prompt errors aligned with Sections 5.4/5.5/12.4:

- `template_parse_error`
  - Invalid template syntax.
- `template_render_error`
  - Unknown variable/filter or invalid interpolation during render.

Error object fields:

- `kind` (`template_parse_error` | `template_render_error`)
- `message` (safe summary)
- `template_excerpt` (small sanitized snippet, optional)
- `cause` (raw engine error, optional internal detail)

Runtime behavior:

- Prompt build failure aborts the current run attempt immediately.
- Worker exits with failure classification so orchestrator retry policy applies.

### 3. Template Inputs and Normalization

Only these top-level template variables are provided:

- `issue`: normalized domain issue object
- `attempt`: `null` for first run, integer for continuation/retry

Compatibility rules:

- Convert issue keys to string-compatible object keys.
- Preserve arrays/maps (`labels`, `blocked_by`) for template loops.
- No implicit extra globals are injected.

### 4. Empty-Body Fallback

If `workflow.prompt_template.trim()` is empty:

- Use explicit default prompt: `You are working on an issue from Linear.`
- Continue rendering path with the same variable context for consistency.

Non-goal:

- Do not silently fallback for workflow read/parse/front matter errors; those remain typed configuration errors and should block dispatch elsewhere.

### 5. Retry/Continuation Semantics

`attempt` handling:

- First dispatch: `attempt = null`
- Continuation after normal worker exit: `attempt = 1`, then increment per continuation/retry schedule
- Abnormal retry paths reuse orchestrator retry-attempt integer

Prompt templates can branch behavior by attempt state:

- first-run instructions when `attempt` is null
- continuation instructions when `attempt >= 1`

### 6. Integration Points

1. Worker attempt flow receives `(issue, attempt)` from orchestrator.
2. Worker calls prompt builder with `workflow.prompt_template`.
3. On success: pass rendered prompt into agent turn startup.
4. On failure: return typed failure to orchestrator without launching turn.

## Test Design

## Unit Tests (Prompt Builder)

1. Renders issue fields and `attempt` for first attempt (`null`) and retry (`>=1`).
2. Fails with `template_render_error` on unknown variable.
3. Fails with `template_render_error` on unknown filter.
4. Fails with `template_parse_error` on malformed template syntax.
5. Uses default fallback prompt when template body is empty/whitespace.
6. Preserves iterable structures (`labels`, `blocked_by`) for loops.

## Integration Tests (Worker Attempt Path)

1. Successful prompt build is passed to agent runner turn initiation.
2. Prompt build failure short-circuits attempt and reports typed error.
3. Retry/continuation attempts pass expected `attempt` value into rendering context.

## Conformance Mapping

- Section 5.4: strict unknown variable/filter failures, `issue`/`attempt` variables, empty-body fallback.
- Section 12.2/12.3/12.4: strict rendering rules, continuation semantics, immediate attempt failure on render errors.
- Section 17.1/18.1: strict prompt rendering and `issue`/`attempt` conformance behavior.

## Scope Boundaries

### In Scope

- Strict template parse/render implementation and wrappers
- Typed prompt error model for parse/render failures
- Empty-template fallback handling
- `attempt`-aware rendering behavior for first and continuation/retry runs
- Tests for contract behavior

### Out of Scope

- Workflow file IO/front matter parsing and watcher/reload plumbing
- Orchestrator retry algorithm changes
- Agent protocol transport implementation details

## Risks and Mitigations

1. Risk: Engine errors leak noisy or sensitive internals.
   - Mitigation: normalize to typed/safe error messages and cap template excerpts.
2. Risk: Ambiguity between continuation and retry attempt numbering.
   - Mitigation: standardize attempt semantics in execution contracts and integration tests.
3. Risk: Future template features drift from strict rules.
   - Mitigation: centralize rendering through one prompt builder module and enforce with tests.

## Implementation Notes

Implemented `KAT-224` strict prompt rendering contract with typed results/errors and strict Liquid behavior.

- Added prompt contracts and exports:
  - `src/execution/prompt/contracts.ts`
  - `src/execution/prompt/index.ts`
  - `src/execution/contracts.ts`
- Added strict prompt builder with explicit empty-template fallback:
  - `src/execution/prompt/build-prompt.ts`
- Added focused contract and behavior tests:
  - `tests/contracts/execution-prompt-contracts.test.ts`
  - `tests/execution/prompt/build-prompt.test.ts`
  - `tests/execution/prompt/build-prompt-errors.test.ts`
- Added runtime templating dependency:
  - `package.json` (`liquidjs`)
  - `pnpm-lock.yaml`

Verification evidence:

- `pnpm run lint` (pass)
- `pnpm run typecheck` (pass)
- `pnpm test` (pass)
- `make check` (initial fail on docs sync before this section; rerun required)

## Handoff

This design defines the strict rendering contract and error semantics needed for `KAT-224` and unblocks downstream worker pipeline and protocol tickets (`KAT-228`, `KAT-229`) plus dispatch preflight integration (`KAT-225`).
