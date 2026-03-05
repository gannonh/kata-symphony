# KAT-223 Typed Config Layer with Defaults, Env Resolution, and Reload Design

## Context

- Ticket: `KAT-223`
- Branch: `feature/kat-223-plan-build-typed-config-layer-with-defaults-env-resolution`
- Goal: Implement config behavior for `SPEC.md` Sections `5.3` and `6` with conformance to Section `6.4` defaults/coercion.
- Parent/dependency context reviewed:
  - `KAT-221` (`Done`) as direct blocker cleared for this work.
  - `KAT-255` (`Done`) as upstream scaffold parent in the milestone chain.
  - Linear project milestone: `M1 Foundations & Contract Loading`.

## References Reviewed

- `SPEC.md`:
  - Section `5.3` (front matter schema)
  - Section `6.1` (source precedence and resolution semantics)
  - Section `6.2` (dynamic reload semantics)
  - Section `6.3` (dispatch preflight validation touchpoints)
  - Section `6.4` (config cheat sheet defaults/coercion)
  - Section `17.1` (core conformance tests for config/reload)
- `ARCHITECTURE.md` (typed config coercion layer intent)
- `WORKFLOW.md` (current runtime contract shape)
- Linear docs:
  - `Project Spec` (`c02d5eb5a8d1`)
  - `Symphony v1 Execution Plan (Dependency DAG)` (`5b235d1e8099`)
- Current code baseline:
  - `src/config/contracts.ts` static two-field provider
  - bootstrap/orchestrator contracts consuming config snapshot

## Requirements Summary

1. Typed getters must cover `tracker`, `polling`, `workspace`, `hooks`, `agent`, and `codex`.
2. `$VAR` resolution and path expansion must match Sections `5.3` and `6.1`.
3. Reload must re-apply configuration for **future** dispatch/retry/reconciliation/hook/agent work.
4. Invalid reload must preserve last-known-good effective config and keep service alive.

## Assumptions

1. This ticket delivers the typed config layer and reload behavior, not full orchestrator policy logic.
2. Workflow parsing/discovery from `WORKFLOW.md` is available from adjacent contract-loading work; this ticket consumes parsed workflow config and owns typed coercion + effective config state.
3. Existing in-flight runs are not force-restarted on config change (spec-compliant); only future operations adopt new config.

## Options Considered

1. Immutable effective-config snapshots with atomic swap on reload (selected)
   - Build a normalized `EffectiveConfig` object; swap it only after full validation/coercion succeeds.
   - Pros: clean last-known-good fallback, simple concurrency model, deterministic behavior.
   - Cons: requires clear snapshot boundaries and slightly more up-front type definitions.

2. Lazy per-getter coercion against raw workflow map
   - Coerce each field at read time from raw config.
   - Pros: lower initial implementation effort.
   - Cons: inconsistent behavior across call sites, harder fallback semantics, repeated coercion logic.

3. Mutable config object updated field-by-field during reload
   - Apply changed keys in place, track per-key errors.
   - Pros: fine-grained updates.
   - Cons: partial update risk, complex rollback behavior, higher correctness risk under failures.

## Selected Approach

Use an immutable `EffectiveConfig` model and perform **validate+coerce first, atomic swap second**.  
If reload fails, emit an operator-visible error and keep prior snapshot unchanged.

## Proposed Design

### 1. Typed Config Surface

Expand `src/config/contracts.ts` from a minimal snapshot to domain-specific typed views:

- `TrackerConfig`
- `PollingConfig`
- `WorkspaceConfig`
- `HooksConfig`
- `AgentConfig`
- `CodexConfig`
- `EffectiveConfig` (aggregate)

`ConfigProvider` exposes:

1. typed getters:
   - `getTrackerConfig()`
   - `getPollingConfig()`
   - `getWorkspaceConfig()`
   - `getHooksConfig()`
   - `getAgentConfig()`
   - `getCodexConfig()`
2. `getSnapshot()` returning a defensive copy of `EffectiveConfig`.
3. `reload()` returning `{ applied: boolean; error?: ConfigError }`.

### 2. Coercion and Defaulting Rules

Implement one coercion pipeline that maps raw front matter into `EffectiveConfig`:

1. start from raw front matter maps under keys in Section `5.3`.
2. normalize scalar/list/map inputs per field.
3. resolve env indirection for supported fields (`$VAR` semantics).
4. apply defaults for missing/invalid optional values per Section `6.4`.
5. enforce required-field validation used by preflight gates (`tracker.kind`, `tracker.api_key` post-resolution, `tracker.project_slug`, non-empty `codex.command`).

Key defaults and constraints (minimum set):

- `tracker.endpoint`: `https://api.linear.app/graphql` (for `kind=linear`)
- `tracker.active_states`: `["Todo", "In Progress"]`
- `tracker.terminal_states`: `["Closed", "Cancelled", "Canceled", "Duplicate", "Done"]`
- `polling.interval_ms`: `30000`
- `workspace.root`: `<system-temp>/symphony_workspaces`
- `hooks.timeout_ms`: `60000` (fallback on non-positive)
- `agent.max_concurrent_agents`: `10`
- `agent.max_turns`: `20`
- `agent.max_retry_backoff_ms`: `300000`
- `agent.max_concurrent_agents_by_state`: normalize state keys (`trim+lowercase`), ignore invalid values
- `codex.command`: `codex app-server`
- `codex.turn_timeout_ms`: `3600000`
- `codex.read_timeout_ms`: `5000`
- `codex.stall_timeout_ms`: `300000` (`<=0` disables stall detection)

### 3. `$VAR` and Path Expansion Semantics

Resolution behavior:

1. If configured value is string token form `$NAME`, resolve from `process.env.NAME`.
2. For `tracker.api_key`, empty resolution is treated as missing.
3. For path-intended fields (not arbitrary commands/URIs), apply:
   - `~` home expansion
   - `$VAR` expansion for env-backed path values
4. Preserve bare strings without path separators for relative roots (spec allows this, though discouraged).
5. Do not rewrite `codex.command` or tracker endpoint URIs as path values.

### 4. Dynamic Reload with Last-Known-Good

Maintain in-memory state:

- `currentEffectiveConfig`
- `currentPromptTemplate`
- `lastKnownGood` (same object reference as current after successful load)

Reload algorithm:

1. detect workflow file update (watcher + defensive re-read path).
2. parse workflow and build candidate `EffectiveConfig`.
3. if candidate valid:
   - atomically swap `currentEffectiveConfig/currentPromptTemplate`
   - update orchestrator runtime knobs used for future work.
4. if candidate invalid:
   - keep existing config/prompt unchanged
   - log operator-visible structured error with typed reason.

Runtime application boundary:

- New config is consumed on future:
  - dispatch eligibility/sorting inputs dependent on config
  - retry scheduling/backoff calculations
  - reconciliation state rules (`active_states`/`terminal_states`)
  - workspace hook execution (`hooks.*`)
  - agent launch settings (`codex.*`, `agent.*`)
- In-flight agent attempts are not restarted automatically.

### 5. Integration Points

1. Bootstrap startup:
   - initial load must pass startup validation or fail fast.
2. Orchestrator:
   - read config via provider on each relevant operation boundary.
3. Preflight:
   - use typed provider validation result before dispatch tick.
4. Observability:
   - emit reload success/failure events with non-secret metadata.

## Error Handling Model

- Startup load failure: fail startup with typed config/workflow error.
- Runtime reload failure: service remains running with previous effective config.
- Validation failures on dispatch preflight: skip dispatch tick, continue reconciliation.
- Secret handling: never log resolved token values.

## Testing Strategy

### Unit Tests

1. Default application for all Section `6.4` fields.
2. Numeric/string coercion for integer fields.
3. List coercion from comma-separated and array forms.
4. `$VAR` resolution, including empty env handling for API key.
5. Path expansion behavior (`~`, env path values, relative bare root preservation).
6. `max_concurrent_agents_by_state` normalization and invalid-entry filtering.

### Reload/Behavior Tests

1. Valid reload swaps effective config and is visible on subsequent getter calls.
2. Invalid reload preserves prior effective config and prompt.
3. Reloaded values affect future dispatch/retry/reconciliation/hook/agent boundaries.
4. In-flight attempt remains unchanged when reload occurs mid-run.

### Preflight/Failure Tests

1. Missing required tracker fields after resolution triggers validation failure.
2. Empty/whitespace `codex.command` fails validation.
3. Reload failure emits operator-visible error while process remains alive.

## Verification Commands

1. `pnpm run lint`
2. `pnpm run typecheck`
3. `pnpm test`
4. `make check`

## Scope Boundaries

### In Scope

- Typed config contracts and getters for all Section `5.3` domains
- Section `6.4` defaults/coercion behavior
- `$VAR` and path expansion semantics for supported fields
- Dynamic reload + last-known-good fallback behavior

### Out of Scope

- Full tracker adapter implementation details (`KAT-226`)
- Workspace manager execution semantics (`KAT-227`)
- App-server protocol runner details (`KAT-228`)
- Complete orchestrator scheduling implementation beyond config integration points

## Risks and Mitigations

1. Risk: Spec drift in defaults/coercion.
   - Mitigation: encode Section `6.4` as explicit test matrix with one assertion per field.
2. Risk: Partial reload side effects.
   - Mitigation: atomic swap only after full candidate validation passes.
3. Risk: Secret leakage in error logs.
   - Mitigation: redact/omit resolved secret values in all config error surfaces.

## Handoff

This design is ready to transition into implementation planning (`writing-plans`) for `KAT-223`.

## Implementation Alignment Notes (2026-03-05)

The implemented solution preserves the design intent (typed normalized effective config + last-known-good fallback), with two deliberate shape adjustments:

1. `ConfigProvider` remains snapshot-based (`getSnapshot`) instead of adding six individual section getter methods.
   - Snapshot is fully typed (`EffectiveConfig` shape) and deep-cloned on read.
   - Callers access section-specific config via `snapshot.tracker`, `snapshot.polling`, `snapshot.workspace`, `snapshot.hooks`, `snapshot.agent`, and `snapshot.codex`.

2. Reload behavior is provided through a dedicated `createReloadableConfigProvider(...)` implementation in `src/config/reloadable-provider.ts`.
   - Static bootstrap wiring continues to use `createStaticConfigProvider(...)`.
   - Reload entrypoint keeps prior effective snapshot when candidate config validation fails.
