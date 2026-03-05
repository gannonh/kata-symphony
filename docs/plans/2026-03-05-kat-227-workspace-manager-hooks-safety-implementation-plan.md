# KAT-227 Workspace Manager Hooks Safety Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement a deterministic, root-contained workspace manager with lifecycle hook timeout/failure semantics and terminal cleanup primitives matching `SPEC.md` Sections 9, 17.2, and 18.1.

**Architecture:** Add a dedicated `src/execution/workspace/` module split into path safety, hook execution, cleanup, and manager orchestration. Keep `ensureWorkspace(issueIdentifier)` stable while adding backward-compatible workspace lifecycle methods used by startup/reconciliation and attempt execution. Use dependency-injected helpers for filesystem and process execution so timeout/failure branches are unit-testable without flaky shell integration.

**Tech Stack:** TypeScript (Node 22), Vitest, `node:fs/promises`, `node:path`, `node:child_process`.

---

**Skill refs for execution:** `@test-driven-development`, `@verification-before-completion`, `@executing-plans`

## Task 1: Add Workspace Error and Hook Contract Surface

**Files:**
- Create: `src/execution/workspace/errors.ts`
- Modify: `src/execution/contracts.ts`
- Modify: `src/execution/index.ts` (if added in this task; otherwise skip)
- Modify: `tests/contracts/layer-contracts.test.ts`
- Create: `tests/execution/workspace/workspace-errors-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import type { WorkspaceManager } from '../../../src/execution/contracts.js'
import { WorkspaceExecutionError } from '../../../src/execution/workspace/errors.js'

describe('workspace contracts', () => {
  it('exposes typed workspace execution error codes', () => {
    const err = new WorkspaceExecutionError(
      'workspace_path_outside_root',
      'outside root',
      { workspace_path: '/tmp/outside', workspace_root: '/tmp/root', fatal: true },
    )

    expect(err.code).toBe('workspace_path_outside_root')
    expect(err.context.fatal).toBe(true)
  })

  it('supports additive workspace manager lifecycle methods', () => {
    const manager = {} as WorkspaceManager
    expect(typeof manager.ensureWorkspace).toBe('function')
    expect(typeof manager.runBeforeRun).toBe('function')
    expect(typeof manager.runAfterRun).toBe('function')
    expect(typeof manager.removeWorkspace).toBe('function')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/workspace/workspace-errors-contracts.test.ts tests/contracts/layer-contracts.test.ts`
Expected: FAIL because workspace error class and additive manager methods do not exist yet.

**Step 3: Write minimal implementation**

```ts
// src/execution/workspace/errors.ts
export type WorkspaceErrorCode =
  | 'workspace_path_outside_root'
  | 'workspace_path_not_directory'
  | 'workspace_fs_error'
  | 'workspace_hook_failed'
  | 'workspace_hook_timeout'

export class WorkspaceExecutionError extends Error {
  readonly code: WorkspaceErrorCode
  readonly context: Record<string, unknown>
  constructor(code: WorkspaceErrorCode, message: string, context: Record<string, unknown> = {}) {
    super(message)
    this.name = 'WorkspaceExecutionError'
    this.code = code
    this.context = context
  }
}
```

```ts
// src/execution/contracts.ts (excerpt)
export interface WorkspaceManager {
  ensureWorkspace(issueIdentifier: string): Promise<Workspace>
  runBeforeRun(workspace: Workspace): Promise<void>
  runAfterRun(workspace: Workspace): Promise<void>
  removeWorkspace(issueIdentifier: string): Promise<{ removed: boolean; path: string }>
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/workspace/workspace-errors-contracts.test.ts tests/contracts/layer-contracts.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/workspace/errors.ts src/execution/contracts.ts tests/execution/workspace/workspace-errors-contracts.test.ts tests/contracts/layer-contracts.test.ts
git commit -m "feat(workspace): add typed workspace error and lifecycle contract surface"
```

## Task 2: Implement Deterministic Workspace Paths with Root Containment

**Files:**
- Create: `src/execution/workspace/paths.ts`
- Modify: `src/domain/normalization.ts` (only if edge-case sanitizer fixes are needed)
- Create: `tests/execution/workspace/workspace-paths.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import {
  createWorkspacePathForIssue,
  assertWorkspaceInsideRoot,
} from '../../../src/execution/workspace/paths.js'

describe('workspace path safety', () => {
  it('creates deterministic workspace paths from sanitized issue identifiers', () => {
    const first = createWorkspacePathForIssue('/tmp/ws', 'KAT-227/fix*scope')
    const second = createWorkspacePathForIssue('/tmp/ws', 'KAT-227/fix*scope')

    expect(first.workspace_key).toBe('KAT-227_fix_scope')
    expect(first.path).toBe(second.path)
  })

  it('rejects workspace paths that escape root containment', () => {
    expect(() => assertWorkspaceInsideRoot('/tmp/ws', '/tmp/other/KAT-1')).toThrowError(
      /workspace_path_outside_root/,
    )
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/workspace/workspace-paths.test.ts`
Expected: FAIL because workspace path helper module does not exist.

**Step 3: Write minimal implementation**

```ts
// src/execution/workspace/paths.ts
import path from 'node:path'
import { sanitizeWorkspaceKey } from '../../domain/normalization.js'
import { WorkspaceExecutionError } from './errors.js'

export function createWorkspacePathForIssue(workspaceRoot: string, issueIdentifier: string) {
  const workspace_root = path.resolve(workspaceRoot)
  const workspace_key = sanitizeWorkspaceKey(issueIdentifier)
  const candidate = path.resolve(workspace_root, workspace_key)
  assertWorkspaceInsideRoot(workspace_root, candidate)
  return { workspace_root, workspace_key, path: candidate }
}

export function assertWorkspaceInsideRoot(workspaceRootAbs: string, workspacePathAbs: string): void {
  const rel = path.relative(workspaceRootAbs, workspacePathAbs)
  if (rel.startsWith('..') || path.isAbsolute(rel)) {
    throw new WorkspaceExecutionError(
      'workspace_path_outside_root',
      'workspace path is outside configured workspace root',
      { workspace_root: workspaceRootAbs, workspace_path: workspacePathAbs, fatal: true },
    )
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/workspace/workspace-paths.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/workspace/paths.ts tests/execution/workspace/workspace-paths.test.ts src/domain/normalization.ts
git commit -m "feat(workspace): add deterministic path derivation and root containment checks"
```

## Task 3: Implement Hook Runner with Timeout and Typed Failure Policy

**Files:**
- Create: `src/execution/workspace/hooks.ts`
- Create: `tests/execution/workspace/workspace-hooks.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'
import { runWorkspaceHook } from '../../../src/execution/workspace/hooks.js'

describe('workspace hook runner', () => {
  it('throws fatal timeout for before_run', async () => {
    const runCommand = vi.fn().mockResolvedValue({ timed_out: true, exit_code: null, stderr: 'timeout' })

    await expect(
      runWorkspaceHook({
        hook: 'before_run',
        script: 'echo hi',
        timeout_ms: 1000,
        cwd: '/tmp/ws/KAT-227',
        runCommand,
      }),
    ).rejects.toMatchObject({ code: 'workspace_hook_timeout' })
  })

  it('returns ignored failure for after_run', async () => {
    const runCommand = vi.fn().mockResolvedValue({ timed_out: false, exit_code: 42, stderr: 'bad' })
    const result = await runWorkspaceHook({
      hook: 'after_run',
      script: 'exit 42',
      timeout_ms: 1000,
      cwd: '/tmp/ws/KAT-227',
      runCommand,
    })

    expect(result.outcome).toBe('ignored_failure')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/workspace/workspace-hooks.test.ts`
Expected: FAIL because hook runner does not exist.

**Step 3: Write minimal implementation**

```ts
// src/execution/workspace/hooks.ts (excerpt)
import { WorkspaceExecutionError } from './errors.js'

export type WorkspaceHookName = 'after_create' | 'before_run' | 'after_run' | 'before_remove'
export type HookRunResult = { outcome: 'ok' | 'ignored_failure' }

const FATAL_HOOKS = new Set<WorkspaceHookName>(['after_create', 'before_run'])

export async function runWorkspaceHook(input: {
  hook: WorkspaceHookName
  script: string | null
  timeout_ms: number
  cwd: string
  runCommand: (args: { script: string; cwd: string; timeout_ms: number }) => Promise<{ timed_out: boolean; exit_code: number | null; stderr: string }>
}): Promise<HookRunResult> {
  if (!input.script || input.script.trim().length === 0) return { outcome: 'ok' }
  const result = await input.runCommand({ script: input.script, cwd: input.cwd, timeout_ms: input.timeout_ms })
  const fatal = FATAL_HOOKS.has(input.hook)
  if (result.timed_out) {
    if (fatal) throw new WorkspaceExecutionError('workspace_hook_timeout', `${input.hook} timed out`, { hook: input.hook, fatal, workspace_path: input.cwd })
    return { outcome: 'ignored_failure' }
  }
  if (result.exit_code !== 0) {
    if (fatal) throw new WorkspaceExecutionError('workspace_hook_failed', `${input.hook} failed`, { hook: input.hook, fatal, workspace_path: input.cwd })
    return { outcome: 'ignored_failure' }
  }
  return { outcome: 'ok' }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/workspace/workspace-hooks.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/workspace/hooks.ts tests/execution/workspace/workspace-hooks.test.ts
git commit -m "feat(workspace): add hook runner timeout policy with fatal/nonfatal semantics"
```

## Task 4: Implement Workspace Manager Ensure/Create/Reuse + Hook Invocation

**Files:**
- Create: `src/execution/workspace/manager.ts`
- Create: `src/execution/workspace/index.ts`
- Create: `tests/execution/workspace/workspace-manager.test.ts`
- Modify: `src/execution/contracts.ts` (imports/exports only if needed)

**Step 1: Write the failing test**

```ts
import { mkdtemp, writeFile, rm } from 'node:fs/promises'
import path from 'node:path'
import os from 'node:os'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWorkspaceManager } from '../../../src/execution/workspace/index.js'

describe('workspace manager ensureWorkspace', () => {
  const dirs: string[] = []
  afterEach(async () => { await Promise.all(dirs.map((d) => rm(d, { recursive: true, force: true }))) })

  it('creates missing workspace and marks created_now true', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: { after_create: null, before_run: null, after_run: null, before_remove: null, timeout_ms: 1000 },
    })

    const ws = await manager.ensureWorkspace('KAT-227/fix*scope')
    expect(ws.created_now).toBe(true)
    expect(ws.path).toContain('KAT-227_fix_scope')
  })

  it('fails when workspace target exists as non-directory', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    await writeFile(path.join(root, 'KAT-227'), 'not a directory')
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: { after_create: null, before_run: null, after_run: null, before_remove: null, timeout_ms: 1000 },
    })

    await expect(manager.ensureWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_path_not_directory',
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/workspace/workspace-manager.test.ts`
Expected: FAIL because manager implementation does not exist.

**Step 3: Write minimal implementation**

```ts
// src/execution/workspace/manager.ts (excerpt)
import { mkdir, stat } from 'node:fs/promises'
import type { Workspace } from '../../domain/models.js'
import { createWorkspacePathForIssue } from './paths.js'
import { runWorkspaceHook } from './hooks.js'
import { WorkspaceExecutionError } from './errors.js'

export function createWorkspaceManager(input: {
  workspaceRoot: string
  hooks: { after_create: string | null; before_run: string | null; after_run: string | null; before_remove: string | null; timeout_ms: number }
  runCommand?: (args: { script: string; cwd: string; timeout_ms: number }) => Promise<{ timed_out: boolean; exit_code: number | null; stderr: string }>
}) {
  // return object implementing WorkspaceManager
}

async function ensureDirectory(pathAbs: string): Promise<boolean> {
  try {
    const existing = await stat(pathAbs)
    if (!existing.isDirectory()) {
      throw new WorkspaceExecutionError('workspace_path_not_directory', 'workspace path exists and is not a directory', { workspace_path: pathAbs, fatal: true })
    }
    return false
  } catch {
    await mkdir(pathAbs, { recursive: true })
    return true
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/workspace/workspace-manager.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/workspace/manager.ts src/execution/workspace/index.ts tests/execution/workspace/workspace-manager.test.ts
git commit -m "feat(workspace): implement deterministic ensureWorkspace create/reuse behavior"
```

## Task 5: Add `before_run` / `after_run` / `before_remove` Methods and Cleanup Primitive

**Files:**
- Modify: `src/execution/workspace/manager.ts`
- Create: `tests/execution/workspace/workspace-cleanup.test.ts`

**Step 1: Write the failing test**

```ts
import { mkdtemp, mkdir, rm } from 'node:fs/promises'
import path from 'node:path'
import os from 'node:os'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWorkspaceManager } from '../../../src/execution/workspace/index.js'

describe('workspace lifecycle helpers', () => {
  const dirs: string[] = []
  afterEach(async () => { await Promise.all(dirs.map((d) => rm(d, { recursive: true, force: true }))) })

  it('before_run propagates timeout/failure as fatal', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const runCommand = vi.fn().mockResolvedValue({ timed_out: true, exit_code: null, stderr: 'timeout' })
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: { after_create: null, before_run: 'echo x', after_run: null, before_remove: null, timeout_ms: 10 },
      runCommand,
    })

    const ws = await manager.ensureWorkspace('KAT-227')
    await expect(manager.runBeforeRun(ws)).rejects.toMatchObject({ code: 'workspace_hook_timeout' })
  })

  it('removeWorkspace ignores before_remove failures and still deletes', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    await mkdir(path.join(root, 'KAT-227'), { recursive: true })
    const runCommand = vi.fn().mockResolvedValue({ timed_out: false, exit_code: 99, stderr: 'fail' })
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: { after_create: null, before_run: null, after_run: null, before_remove: 'exit 99', timeout_ms: 10 },
      runCommand,
    })

    const result = await manager.removeWorkspace('KAT-227')
    expect(result.removed).toBe(true)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/workspace/workspace-cleanup.test.ts`
Expected: FAIL because lifecycle helper methods are incomplete.

**Step 3: Write minimal implementation**

```ts
// src/execution/workspace/manager.ts (method excerpts)
async runBeforeRun(workspace) {
  await runWorkspaceHook({ hook: 'before_run', script: hooks.before_run, timeout_ms: hooks.timeout_ms, cwd: workspace.path, runCommand })
}

async runAfterRun(workspace) {
  await runWorkspaceHook({ hook: 'after_run', script: hooks.after_run, timeout_ms: hooks.timeout_ms, cwd: workspace.path, runCommand })
}

async removeWorkspace(issueIdentifier: string) {
  const resolved = createWorkspacePathForIssue(workspaceRoot, issueIdentifier)
  // run before_remove as best effort
  await runWorkspaceHook({ hook: 'before_remove', script: hooks.before_remove, timeout_ms: hooks.timeout_ms, cwd: resolved.path, runCommand })
  // delete directory if present
  return { removed: true, path: resolved.path }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/workspace/workspace-cleanup.test.ts tests/execution/workspace/workspace-manager.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/workspace/manager.ts tests/execution/workspace/workspace-cleanup.test.ts
git commit -m "feat(workspace): add lifecycle hook runners and terminal cleanup primitive"
```

## Task 6: Wire Bootstrap to Real Workspace Manager and Validate End-to-End Contract

**Files:**
- Modify: `src/bootstrap/service.ts`
- Modify: `tests/bootstrap/service-wiring.test.ts`
- Modify: `tests/bootstrap/service-internals.test.ts`
- Modify: `tests/contracts/runtime-modules.test.ts`

**Step 1: Write the failing test**

```ts
it('returns absolute workspace paths rooted under configured workspace root', async () => {
  const service = createService()
  const snapshot = service.config.getSnapshot()

  const ws = await service.workspace.ensureWorkspace('KAT-227/fix*scope')
  expect(ws.path.startsWith('/')).toBe(true)
  expect(ws.path.startsWith(snapshot.workspace.root)).toBe(true)
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/bootstrap/service-wiring.test.ts tests/bootstrap/service-internals.test.ts`
Expected: FAIL because bootstrap still uses `/tmp/symphony/${workspaceKey}` stub and lacks lifecycle methods.

**Step 3: Write minimal implementation**

```ts
// src/bootstrap/service.ts (excerpt)
import { createWorkspaceManager } from '../execution/workspace/index.js'

const workspace = createWorkspaceManager({
  workspaceRoot: config.getSnapshot().workspace.root,
  hooks: config.getSnapshot().hooks,
  logger,
})
```

Also update bootstrap tests to assert:
- `ensureWorkspace` absolute/root-contained behavior
- lifecycle helper methods exist and resolve with no hooks configured

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/bootstrap/service-wiring.test.ts tests/bootstrap/service-internals.test.ts tests/contracts/runtime-modules.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/bootstrap/service.ts tests/bootstrap/service-wiring.test.ts tests/bootstrap/service-internals.test.ts tests/contracts/runtime-modules.test.ts
git commit -m "feat(bootstrap): wire real workspace manager into service graph"
```

## Task 7: Conformance Verification and Documentation Sync

**Files:**
- Modify: `docs/design-docs/index.md` (if additional note needed)
- Modify: `docs/generated/kat-227-verification.md` (create if missing)
- Modify: `PLANS.md` (if your process tracks active execution)

**Step 1: Write the failing verification checklist file**

```md
# KAT-227 Verification

- [ ] Section 9 deterministic path/create/reuse
- [ ] Section 9.4 hook timeout/failure semantics
- [ ] Section 9.5 root containment invariant
- [ ] Section 17.2 workspace manager/safety matrix
```

**Step 2: Run targeted and full test suites**

Run: `pnpm vitest run tests/execution/workspace tests/bootstrap/service-wiring.test.ts`
Expected: PASS.

Run: `pnpm run test`
Expected: PASS.

Run: `pnpm run lint && pnpm run typecheck`
Expected: PASS.

Run: `make check`
Expected: PASS.

**Step 3: Capture evidence in docs**

Record command outputs and conformance mapping in `docs/generated/kat-227-verification.md`.

**Step 4: Final repo sanity**

Run: `git status --short`
Expected: only intended `KAT-227` files are modified.

**Step 5: Commit**

```bash
git add docs/generated/kat-227-verification.md docs/design-docs/index.md PLANS.md
git commit -m "docs(verification): capture kat-227 conformance evidence"
```
