# KAT-225 Dispatch Preflight Validation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement startup and per-tick dispatch preflight validation that enforces `SPEC.md` Section `6.3`, fails startup cleanly when invalid, and skips dispatch (while preserving reconciliation) when per-tick validation fails.

**Architecture:** Add an orchestrator-owned preflight validator that returns a discriminated, machine-branchable result with stable error codes. Integrate the validator into startup (`startService`) and provide a tick gate helper consumed by orchestrator polling flow so dispatch can be conditionally skipped without blocking reconciliation. Centralize redacted operator logging to avoid secret leakage.

**Tech Stack:** TypeScript (Node 22), Vitest, existing `src/workflow` loader contract, existing config snapshot provider.

---

**Skill refs for execution:** `@test-driven-development`, `@verification-before-completion`

### Task 1: Add preflight contracts and public exports

**Files:**
- Create: `src/orchestrator/preflight/contracts.ts`
- Create: `src/orchestrator/preflight/index.ts`
- Modify: `src/orchestrator/contracts.ts`
- Test: `tests/orchestrator/preflight-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import type { DispatchPreflightErrorCode } from '../../src/orchestrator/preflight/index.js'
import { isDispatchPreflightFailure } from '../../src/orchestrator/preflight/index.js'

describe('dispatch preflight contracts', () => {
  it('exposes stable error codes and narrowing helper', () => {
    const codes: DispatchPreflightErrorCode[] = [
      'workflow_invalid',
      'tracker_kind_missing',
      'tracker_kind_unsupported',
      'tracker_api_key_missing',
      'tracker_project_slug_missing',
      'codex_command_missing',
    ]

    expect(codes).toHaveLength(6)
    expect(isDispatchPreflightFailure({ ok: false, errors: [] })).toBe(true)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/orchestrator/preflight-contracts.test.ts`
Expected: FAIL with missing module/export errors.

**Step 3: Write minimal implementation**

```ts
// src/orchestrator/preflight/contracts.ts
export type DispatchPreflightErrorCode =
  | 'workflow_invalid'
  | 'tracker_kind_missing'
  | 'tracker_kind_unsupported'
  | 'tracker_api_key_missing'
  | 'tracker_project_slug_missing'
  | 'codex_command_missing'

export interface DispatchPreflightError {
  code: DispatchPreflightErrorCode
  message: string
  source: 'workflow' | 'config'
  field?: string
}

export type DispatchPreflightResult =
  | { ok: true }
  | { ok: false; errors: DispatchPreflightError[] }

export function isDispatchPreflightFailure(
  result: DispatchPreflightResult,
): result is { ok: false; errors: DispatchPreflightError[] } {
  return result.ok === false
}
```

```ts
// src/orchestrator/preflight/index.ts
export type {
  DispatchPreflightError,
  DispatchPreflightErrorCode,
  DispatchPreflightResult,
} from './contracts.js'
export { isDispatchPreflightFailure } from './contracts.js'
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/orchestrator/preflight-contracts.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/orchestrator/preflight-contracts.test.ts src/orchestrator/preflight/contracts.ts src/orchestrator/preflight/index.ts src/orchestrator/contracts.ts
git commit -m "feat(orchestrator): add dispatch preflight contracts"
```

### Task 2: Implement preflight validator with workflow + config checks

**Files:**
- Create: `src/orchestrator/preflight/validate-dispatch-preflight.ts`
- Modify: `src/orchestrator/preflight/index.ts`
- Test: `tests/orchestrator/validate-dispatch-preflight.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { validateDispatchPreflight } from '../../src/orchestrator/preflight/index.js'

describe('validateDispatchPreflight', () => {
  it('maps workflow loader failure to workflow_invalid', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => {
        const e = new Error('bad yaml') as Error & { code?: string }
        e.code = 'workflow_parse_error'
        throw e
      },
      getSnapshot: () => ({
        tracker: { kind: 'linear', api_key: 'k', project_slug: 'proj' },
        codex: { command: 'codex app-server' },
      }) as never,
    })

    expect(result).toMatchObject({
      ok: false,
      errors: [{ code: 'workflow_invalid', source: 'workflow' }],
    })
  })

  it('returns all config failures in deterministic order', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => ({ config: {}, prompt_template: '' }),
      getSnapshot: () => ({
        tracker: { kind: '' as never, api_key: ' ', project_slug: ' ' },
        codex: { command: '   ' },
      }) as never,
    })

    expect(result).toMatchObject({
      ok: false,
      errors: [
        { code: 'tracker_kind_missing' },
        { code: 'tracker_api_key_missing' },
        { code: 'tracker_project_slug_missing' },
        { code: 'codex_command_missing' },
      ],
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/orchestrator/validate-dispatch-preflight.test.ts`
Expected: FAIL because `validateDispatchPreflight` does not exist.

**Step 3: Write minimal implementation**

```ts
// src/orchestrator/preflight/validate-dispatch-preflight.ts
import type { ConfigSnapshot } from '../../config/contracts.js'
import type { LoadWorkflowDefinition } from '../../workflow/contracts.js'
import type { DispatchPreflightError, DispatchPreflightResult } from './contracts.js'

export interface ValidateDispatchPreflightOptions {
  loadWorkflow: LoadWorkflowDefinition
  getSnapshot: () => ConfigSnapshot
}

function missingOrBlank(value: unknown): boolean {
  return typeof value !== 'string' || value.trim().length === 0
}

export async function validateDispatchPreflight(
  options: ValidateDispatchPreflightOptions,
): Promise<DispatchPreflightResult> {
  const errors: DispatchPreflightError[] = []

  try {
    await options.loadWorkflow()
  } catch {
    errors.push({
      code: 'workflow_invalid',
      message: 'Workflow file cannot be loaded or parsed',
      source: 'workflow',
      field: 'workflow',
    })
  }

  const snapshot = options.getSnapshot()
  const trackerKind = snapshot.tracker?.kind
  const trackerApiKey = snapshot.tracker?.api_key
  const trackerProjectSlug = snapshot.tracker?.project_slug
  const codexCommand = snapshot.codex?.command

  if (missingOrBlank(trackerKind)) {
    errors.push({
      code: 'tracker_kind_missing',
      message: 'tracker.kind is required',
      source: 'config',
      field: 'tracker.kind',
    })
  } else if (trackerKind !== 'linear') {
    errors.push({
      code: 'tracker_kind_unsupported',
      message: `Unsupported tracker.kind: ${trackerKind}`,
      source: 'config',
      field: 'tracker.kind',
    })
  }

  if (missingOrBlank(trackerApiKey)) {
    errors.push({
      code: 'tracker_api_key_missing',
      message: 'tracker.api_key is required after resolution',
      source: 'config',
      field: 'tracker.api_key',
    })
  }

  if (trackerKind === 'linear' && missingOrBlank(trackerProjectSlug)) {
    errors.push({
      code: 'tracker_project_slug_missing',
      message: 'tracker.project_slug is required for tracker.kind=linear',
      source: 'config',
      field: 'tracker.project_slug',
    })
  }

  if (missingOrBlank(codexCommand)) {
    errors.push({
      code: 'codex_command_missing',
      message: 'codex.command is required',
      source: 'config',
      field: 'codex.command',
    })
  }

  return errors.length === 0 ? { ok: true } : { ok: false, errors }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/orchestrator/validate-dispatch-preflight.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/orchestrator/validate-dispatch-preflight.test.ts src/orchestrator/preflight/validate-dispatch-preflight.ts src/orchestrator/preflight/index.ts
git commit -m "feat(orchestrator): implement typed dispatch preflight validator"
```

### Task 3: Add redacted operator-visible preflight logging helper

**Files:**
- Create: `src/orchestrator/preflight/log-preflight-failure.ts`
- Modify: `src/orchestrator/preflight/index.ts`
- Test: `tests/orchestrator/log-preflight-failure.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'
import { logPreflightFailure } from '../../src/orchestrator/preflight/index.js'

describe('logPreflightFailure', () => {
  it('logs error codes and phase without leaking secrets', () => {
    const error = vi.fn()

    logPreflightFailure(
      { error } as never,
      'startup',
      [
        { code: 'tracker_api_key_missing', message: 'missing', source: 'config', field: 'tracker.api_key' },
      ],
      { trackerApiKey: 'super-secret-token' },
    )

    expect(error).toHaveBeenCalledWith(
      'Dispatch preflight validation failed',
      expect.objectContaining({
        phase: 'startup',
        error_codes: ['tracker_api_key_missing'],
      }),
    )

    expect(JSON.stringify(error.mock.calls)).not.toContain('super-secret-token')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/orchestrator/log-preflight-failure.test.ts`
Expected: FAIL because helper is missing.

**Step 3: Write minimal implementation**

```ts
// src/orchestrator/preflight/log-preflight-failure.ts
import type { Logger } from '../../observability/contracts.js'
import type { DispatchPreflightError } from './contracts.js'

export function logPreflightFailure(
  logger: Logger,
  phase: 'startup' | 'tick',
  errors: DispatchPreflightError[],
  context: { workflowPath?: string } = {},
): void {
  logger.error('Dispatch preflight validation failed', {
    phase,
    error_codes: errors.map((entry) => entry.code),
    errors: errors.map((entry) => ({
      code: entry.code,
      field: entry.field,
      source: entry.source,
      message: entry.message,
    })),
    workflow_path: context.workflowPath,
  })
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/orchestrator/log-preflight-failure.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/orchestrator/log-preflight-failure.test.ts src/orchestrator/preflight/log-preflight-failure.ts src/orchestrator/preflight/index.ts
git commit -m "feat(orchestrator): add redacted preflight failure logging"
```

### Task 4: Integrate startup preflight gate in service bootstrap

**Files:**
- Modify: `src/bootstrap/service.ts`
- Test: `tests/bootstrap/service-startup-preflight.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'
import { startService } from '../../src/bootstrap/service.js'

describe('startup preflight gate', () => {
  it('fails startup cleanly when preflight is invalid', async () => {
    const service = {
      orchestrator: { start: vi.fn() },
      logger: { info: vi.fn(), error: vi.fn() },
    } as never

    await expect(
      startService(service, {
        runStartupPreflight: async () => ({
          ok: false,
          errors: [
            { code: 'workflow_invalid', message: 'bad', source: 'workflow' },
          ],
        }),
      }),
    ).rejects.toMatchObject({
      code: 'dispatch_preflight_failed',
    })

    expect(service.orchestrator.start).not.toHaveBeenCalled()
    expect(service.logger.error).toHaveBeenCalled()
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/bootstrap/service-startup-preflight.test.ts`
Expected: FAIL because `startService` does not support preflight gate options.

**Step 3: Write minimal implementation**

```ts
// add typed startup error in src/bootstrap/service.ts
export class StartupPreflightError extends Error {
  readonly code = 'dispatch_preflight_failed'
  constructor(readonly errors: DispatchPreflightError[]) {
    super('Dispatch preflight validation failed at startup')
    this.name = 'StartupPreflightError'
  }
}

// in startService before orchestrator.start()
const preflight = await runStartupPreflight()
if (!preflight.ok) {
  logPreflightFailure(service.logger, 'startup', preflight.errors)
  throw new StartupPreflightError(preflight.errors)
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/bootstrap/service-startup-preflight.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/bootstrap/service-startup-preflight.test.ts src/bootstrap/service.ts
git commit -m "feat(bootstrap): gate startup on dispatch preflight validation"
```

### Task 5: Add per-tick preflight gate helper that preserves reconciliation

**Files:**
- Create: `src/orchestrator/preflight/run-tick-preflight-gate.ts`
- Modify: `src/orchestrator/preflight/index.ts`
- Test: `tests/orchestrator/run-tick-preflight-gate.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'
import { runTickPreflightGate } from '../../src/orchestrator/preflight/index.js'

describe('runTickPreflightGate', () => {
  it('runs reconciliation and skips dispatch when preflight fails', async () => {
    const reconcile = vi.fn(async () => {})
    const logFailure = vi.fn()

    const result = await runTickPreflightGate({
      reconcile,
      validate: async () => ({
        ok: false,
        errors: [{ code: 'codex_command_missing', message: 'missing', source: 'config' }],
      }),
      logFailure,
    })

    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(logFailure).toHaveBeenCalledTimes(1)
    expect(result).toEqual({ dispatchAllowed: false })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/orchestrator/run-tick-preflight-gate.test.ts`
Expected: FAIL because helper is missing.

**Step 3: Write minimal implementation**

```ts
// src/orchestrator/preflight/run-tick-preflight-gate.ts
import type { DispatchPreflightResult } from './contracts.js'

export interface RunTickPreflightGateOptions {
  reconcile: () => Promise<void>
  validate: () => Promise<DispatchPreflightResult>
  logFailure: (errors: { code: string }[]) => void
}

export async function runTickPreflightGate(
  options: RunTickPreflightGateOptions,
): Promise<{ dispatchAllowed: boolean }> {
  await options.reconcile()
  const validation = await options.validate()

  if (!validation.ok) {
    options.logFailure(validation.errors)
    return { dispatchAllowed: false }
  }

  return { dispatchAllowed: true }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/orchestrator/run-tick-preflight-gate.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/orchestrator/run-tick-preflight-gate.test.ts src/orchestrator/preflight/run-tick-preflight-gate.ts src/orchestrator/preflight/index.ts
git commit -m "feat(orchestrator): add per-tick preflight dispatch gate helper"
```

### Task 6: Add startup and tick integration regression coverage + full verification

**Files:**
- Modify: `tests/bootstrap/main-entry.test.ts`
- Modify: `tests/bootstrap/service-wiring.test.ts`
- Modify: `tests/bootstrap/startup.test.ts`
- Modify: `tests/contracts/runtime-modules.test.ts`

**Step 1: Write the failing tests**

```ts
it('sets exitCode=1 when startup preflight fails', async () => {
  const onError = vi.fn()
  await runMain(
    async () => {
      throw new StartupPreflightError([
        { code: 'workflow_invalid', message: 'bad', source: 'workflow' },
      ])
    },
    onError,
  )

  expect(process.exitCode).toBe(1)
  expect(onError).toHaveBeenCalledWith('Symphony startup failed', expect.any(Error))
})
```

```ts
it('exports preflight module from orchestrator layer', async () => {
  const mod = await import('../../src/orchestrator/preflight/index.js')
  expect(mod.validateDispatchPreflight).toBeTypeOf('function')
  expect(mod.runTickPreflightGate).toBeTypeOf('function')
})
```

**Step 2: Run tests to verify failures**

Run: `pnpm vitest run tests/bootstrap/main-entry.test.ts tests/bootstrap/service-wiring.test.ts tests/contracts/runtime-modules.test.ts`
Expected: FAIL until exports/wiring are complete.

**Step 3: Write minimal implementation/wiring fixes**

```ts
// src/orchestrator/preflight/index.ts
export { validateDispatchPreflight } from './validate-dispatch-preflight.js'
export { runTickPreflightGate } from './run-tick-preflight-gate.js'
export { logPreflightFailure } from './log-preflight-failure.js'
```

```ts
// tests/bootstrap/startup.test.ts
// ensure env/workflow setup reflects startup preflight behavior expectations
```

**Step 4: Run full verification suite**

Run:

- `pnpm run lint`
- `pnpm run typecheck`
- `pnpm test`
- `make check`

Expected: all commands PASS.

**Step 5: Commit**

```bash
git add tests/bootstrap/main-entry.test.ts tests/bootstrap/service-wiring.test.ts tests/bootstrap/startup.test.ts tests/contracts/runtime-modules.test.ts src/orchestrator/preflight/index.ts
git commit -m "test(orchestrator): cover startup and tick preflight integration semantics"
```

## Execution Notes

- Keep commit scope aligned to each task above (frequent small commits).
- Do not modify frozen cross-ticket contracts:
  - `src/tracker/contracts.ts`
  - `src/execution/contracts.ts`
  - `src/domain/models.ts`
- If signature changes appear necessary, stop and log a follow-up integration delta for `KAT-229`/`KAT-230`.
