# KAT-227 Workspace Manager with Hooks and Safety Invariants Design

## Context

- Ticket: `KAT-227`
- Branch: `feature/kat-227-plan-build-workspace-manager-with-hooks-and-safety`
- Goal: implement workspace manager behavior from `SPEC.md` Sections `9`, `17.2`, and `18.1`.
- Project/epic context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M2 Parallel Subsystems`
  - Upstream blocker: `KAT-223` (`Done`)
  - Parallel-worktree contract comment on `KAT-227` (ownership + boundaries)
- Issue docs/attachments:
  - Linear issue docs: none attached
  - Issue attachments/design mocks: none attached

## References Reviewed

- `SPEC.md` Section `8.6` (startup terminal workspace cleanup)
- `SPEC.md` Section `9` (workspace layout/reuse/hooks/safety)
- `SPEC.md` Section `10.7` (agent runner contract includes workspace create/reuse)
- `SPEC.md` Section `15.2` and `15.4` (filesystem safety + hook script safety)
- `SPEC.md` Section `17.2` and `18.1` (conformance requirements)
- Linear document: `Project Spec` (`c02d5eb5a8d1`)
- Linear document: `Symphony v1 Execution Plan (Dependency DAG)` (`5b235d1e8099`)
- Existing code seams:
  - `src/execution/contracts.ts` (`WorkspaceManager.ensureWorkspace`)
  - `src/bootstrap/service.ts` (current workspace stub)
  - `src/domain/normalization.ts` (`sanitizeWorkspaceKey`)
  - `src/config/types.ts` (`workspace.root`, `hooks.*`)

## Problem Statement

Current runtime wiring has only a placeholder workspace implementation. `KAT-227` must deliver a deterministic, safe, and testable workspace manager that:

1. Computes per-issue workspace paths under configured root.
2. Enforces path containment and sanitized workspace keys.
3. Runs workspace lifecycle hooks with timeout and spec-defined failure semantics.
4. Exposes cleanup primitives needed by startup/reconciliation flows without implementing tracker policy or orchestrator state transitions.

## Clarified Scope and Assumptions

1. Ownership is limited to workspace module + workspace/hook safety tests.
2. No tracker adapter logic (`KAT-226`) and no codex protocol internals (`KAT-228`) will be changed.
3. Cleanup behavior in this ticket is **primitive-level** (`remove workspace safely`), while startup/reconciliation orchestration remains for downstream integration work (`KAT-233` / orchestrator tickets).
4. Existing `WorkspaceManager.ensureWorkspace(issueIdentifier)` contract remains and any interface additions are additive/backward-compatible.

## Approaches Considered

1. Monolithic manager class (pathing + hooks + cleanup all in one file)
   - Pros: fewer files, quick to wire.
   - Cons: harder to unit test failure branches; high coupling between filesystem and process execution.
2. Split services: `workspace-paths`, `hook-runner`, `workspace-manager` orchestrator facade (**recommended**)
   - Pros: deterministic unit tests for path math, isolated hook timeout behavior, clearer ownership boundaries for parallel tickets.
   - Cons: slightly more boilerplate and dependency wiring.
3. Functional-only helpers with no stateful manager object
   - Pros: very small primitives.
   - Cons: weak contract boundary; hook semantics and logging policy can drift across call sites.

## Selected Approach

Use **Approach 2** with a small facade implementing `WorkspaceManager` and delegating to:

- path/safety utility for deterministic root-contained paths
- hook runner with timeout and typed failures
- cleanup utility for safe remove flows

This keeps execution policy explicit and testable while preserving current contracts.

## Proposed Design

### 1. Module Layout

Create `src/execution/workspace/`:

- `manager.ts`: primary `WorkspaceManager` implementation
- `paths.ts`: workspace key/path derivation + root containment checks
- `hooks.ts`: hook execution with timeout and failure normalization
- `cleanup.ts`: safe workspace delete primitives + `before_remove` behavior
- `errors.ts`: typed workspace/hook error classes and codes
- `index.ts`: exports

### 2. Contract Surface (Additive)

Keep existing contract:

- `ensureWorkspace(issueIdentifier): Promise<Workspace>`

Additive methods for integration points:

- `runBeforeRun(workspace: Workspace): Promise<void>` (fatal on error/timeout)
- `runAfterRun(workspace: Workspace): Promise<void>` (best effort; logs and resolves)
- `removeWorkspace(issueIdentifier: string): Promise<{ removed: boolean; path: string }>` (best effort `before_remove`; delete proceeds)

Notes:

- Additive methods preserve backward compatibility for existing call sites.
- `ensureWorkspace` remains the required baseline contract from the ticket comment.

### 3. Path Determinism and Safety

Algorithm:

1. `workspace_key = sanitizeWorkspaceKey(issueIdentifier)`
2. `workspace_root_abs = resolve(workspace.root)` (absolute normalized)
3. `workspace_path_abs = resolve(workspace_root_abs, workspace_key)`
4. Enforce containment with path-relative check (`relative(root, candidate)` must not escape root)
5. Ensure directory existence and determine `created_now`

Safety guarantees:

- Reject out-of-root candidates with typed error (`workspace_path_outside_root`).
- Reject non-directory collisions at target path with typed error (`workspace_path_not_directory`).
- Returned `Workspace.path` is absolute and safe to use as `cwd`.

### 4. Hook Execution Semantics

Hook runner contract:

- Executes scripts via `sh -lc <script>` with `cwd = workspace.path`
- Applies `hooks.timeout_ms` with kill-on-timeout behavior
- Captures exit status/stdout/stderr summary for logging (truncated)

Failure policy mapping:

- `after_create`: throw typed error (`fatal=true`)
- `before_run`: throw typed error (`fatal=true`)
- `after_run`: log and ignore (`fatal=false`)
- `before_remove`: log and ignore (`fatal=false`)

Typed error shape:

- `code` (`workspace_hook_failed` | `workspace_hook_timeout` | `workspace_path_outside_root` | `workspace_path_not_directory` | `workspace_fs_error`)
- `hook` (optional)
- `workspace_path`
- `fatal`
- `cause` (internal detail)

### 5. Cleanup Primitives

`removeWorkspace(issueIdentifier)` behavior:

1. Derive safe workspace path from identifier (same deterministic pathing rules)
2. If directory missing, return `{ removed: false }`
3. Run `before_remove` hook best-effort (timeout/failure logged, no throw)
4. Delete directory recursively
5. Return `{ removed: true, path }`

This primitive supports:

- startup terminal sweep (`SPEC` `8.6`)
- active-run transition cleanup on terminal state (orchestrator reconciliation path)

### 6. Logging Contract

Emit structured logs with stable context:

- `issue_identifier`
- `workspace_key`
- `workspace_path`
- `hook`
- `timeout_ms`
- `result` (`created`, `reused`, `deleted`, `hook_failed_ignored`, etc.)

Do not log secrets or full raw hook output.

## Test Strategy

## Unit Tests

1. `ensureWorkspace` creates missing directory and sets `created_now=true`.
2. `ensureWorkspace` reuses existing directory and sets `created_now=false`.
3. Workspace key sanitization with invalid characters, empty string, and dot-segment edge cases.
4. Out-of-root path rejection returns typed containment error.
5. Non-directory collision at workspace path returns typed error.
6. `after_create` runs only on first creation and is fatal on failure/timeout.
7. `before_run` failure/timeout aborts attempt path.
8. `after_run` and `before_remove` failures/timeouts are logged and ignored.
9. `removeWorkspace` deletes existing workspace and is deterministic by identifier.
10. `removeWorkspace` missing path is a no-op result, not a throw.

## Integration-Oriented Tests

1. Bootstrap/service wiring uses real workspace manager (not `/tmp/symphony/*` stub).
2. Returned workspace path is absolute and under configured `workspace.root`.
3. Hook timeout from config is honored.
4. Cleanup primitive output is usable for startup/reconciliation callers.

## Conformance Mapping

- Section `9.1`/`9.2`: deterministic path + create/reuse semantics
- Section `9.4`: timeout + per-hook failure policy
- Section `9.5`/`15.2`: root containment + safe cwd invariant
- Section `17.2`: workspace/hook/cleanup behavior matrix coverage
- Section `18.1`: required workspace manager + lifecycle hooks + cleanup prerequisites

## Scope Boundaries

### In Scope

- Workspace manager module under execution layer
- Hook execution engine with timeout + typed failures
- Cleanup primitives for terminal workspace removal
- Tests for workspace and hook safety invariants

### Out of Scope

- Tracker API behavior and query shapes (`KAT-226`)
- Dispatch preflight policy behavior (`KAT-225`)
- Codex app-server handshake/stream parsing (`KAT-228`)
- Orchestrator terminal cleanup policy wiring itself (beyond providing callable workspace primitives)

## Open Questions for Implementation Plan

1. Should hook stdout/stderr capture be capped by bytes or lines in logs?
2. Should non-directory collision policy be hard-fail only, or configurable replace behavior?
3. Should `removeWorkspace` expose hook outcome metadata for observability/reporting?
