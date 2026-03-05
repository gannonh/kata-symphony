# KAT-223 Typed Config Layer Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a typed config layer that matches `SPEC.md` Sections `5.3` and `6`, including defaults/coercion, `$VAR` and path resolution, and dynamic reload with last-known-good fallback.

**Architecture:** Implement an immutable `EffectiveConfig` snapshot model in `src/config`, with a single normalization pipeline (`raw workflow config -> coerced/validated typed config -> atomic swap`). `ConfigProvider` becomes reload-aware and exposes typed getters for `tracker`, `polling`, `workspace`, `hooks`, `agent`, and `codex`. Reload failures preserve the previous effective snapshot and emit typed errors.

**Tech Stack:** TypeScript (Node 22), Vitest, existing Symphony module contracts.

---

Execution notes: apply `@test-driven-development` on every task, use `@systematic-debugging` if a test behaves unexpectedly, and run `@verification-before-completion` before claiming completion.

### Task 1: Expand Typed Config Contracts

**Files:**
- Modify: `src/config/contracts.ts`
- Create: `src/config/types.ts`
- Create: `tests/config/config-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import type { EffectiveConfig } from '../../src/config/types.js'

describe('config contracts', () => {
  it('exposes typed config sections', async () => {
    const mod = await import('../../src/config/contracts.js')
    expect(mod).toHaveProperty('createStaticConfigProvider')

    const snapshot = mod.createStaticConfigProvider({} as EffectiveConfig).getSnapshot()
    expect(snapshot).toHaveProperty('tracker')
    expect(snapshot).toHaveProperty('polling')
    expect(snapshot).toHaveProperty('workspace')
    expect(snapshot).toHaveProperty('hooks')
    expect(snapshot).toHaveProperty('agent')
    expect(snapshot).toHaveProperty('codex')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm test -- tests/config/config-contracts.test.ts`  
Expected: FAIL with missing typed sections or type mismatches.

**Step 3: Write minimal implementation**

```ts
// src/config/types.ts
export interface TrackerConfig { kind: string; endpoint: string; api_key: string; project_slug: string; active_states: string[]; terminal_states: string[] }
export interface PollingConfig { interval_ms: number }
export interface WorkspaceConfig { root: string }
export interface HooksConfig { after_create: string | null; before_run: string | null; after_run: string | null; before_remove: string | null; timeout_ms: number }
export interface AgentConfig { max_concurrent_agents: number; max_turns: number; max_retry_backoff_ms: number; max_concurrent_agents_by_state: Record<string, number> }
export interface CodexConfig { command: string; approval_policy?: string; thread_sandbox?: string; turn_sandbox_policy?: string; turn_timeout_ms: number; read_timeout_ms: number; stall_timeout_ms: number }
export interface EffectiveConfig { tracker: TrackerConfig; polling: PollingConfig; workspace: WorkspaceConfig; hooks: HooksConfig; agent: AgentConfig; codex: CodexConfig }
```

**Step 4: Run test to verify it passes**

Run: `pnpm test -- tests/config/config-contracts.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/config/contracts.ts src/config/types.ts tests/config/config-contracts.test.ts
git commit -m "feat(config): expand typed config contract surface"
```

### Task 2: Add Defaults and Coercion Helpers

**Files:**
- Create: `src/config/defaults.ts`
- Create: `src/config/coerce.ts`
- Create: `tests/config/defaults-coercion.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { coerceInteger, coerceStringList, normalizeStateMap } from '../../src/config/coerce.js'
import { DEFAULTS } from '../../src/config/defaults.js'

describe('defaults and coercion', () => {
  it('applies spec defaults', () => {
    expect(DEFAULTS.polling.interval_ms).toBe(30000)
    expect(DEFAULTS.agent.max_turns).toBe(20)
  })

  it('coerces string and number forms', () => {
    expect(coerceInteger('42', 1)).toBe(42)
    expect(coerceInteger('bad', 9)).toBe(9)
    expect(coerceStringList('Todo, In Progress')).toEqual(['Todo', 'In Progress'])
    expect(normalizeStateMap({ ' In Progress ': '3', Done: 0 })).toEqual({ 'in progress': 3 })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm test -- tests/config/defaults-coercion.test.ts`  
Expected: FAIL with missing module exports.

**Step 3: Write minimal implementation**

```ts
// src/config/coerce.ts
export function coerceInteger(value: unknown, fallback: number): number {
  const n = typeof value === 'number' ? value : Number(value)
  return Number.isInteger(n) ? n : fallback
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm test -- tests/config/defaults-coercion.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/config/defaults.ts src/config/coerce.ts tests/config/defaults-coercion.test.ts
git commit -m "feat(config): add defaults and coercion helpers"
```

### Task 3: Implement `$VAR` and Path Resolution Semantics

**Files:**
- Create: `src/config/resolve.ts`
- Create: `tests/config/env-path-resolution.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { resolveEnvToken, resolvePathValue } from '../../src/config/resolve.js'

describe('env and path resolution', () => {
  it('resolves $VAR token values', () => {
    const env = { LINEAR_API_KEY: 'abc123' }
    expect(resolveEnvToken('$LINEAR_API_KEY', env)).toBe('abc123')
    expect(resolveEnvToken('$MISSING', env)).toBe('')
  })

  it('expands ~ and env-backed path values only for path fields', () => {
    const env = { SYMPHONY_WORKSPACE_ROOT: '/tmp/ws', HOME: '/Users/test' }
    expect(resolvePathValue('$SYMPHONY_WORKSPACE_ROOT', env)).toBe('/tmp/ws')
    expect(resolvePathValue('~/symphony', env)).toBe('/Users/test/symphony')
    expect(resolvePathValue('relativeRoot', env)).toBe('relativeRoot')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm test -- tests/config/env-path-resolution.test.ts`  
Expected: FAIL with missing resolver functions.

**Step 3: Write minimal implementation**

```ts
const TOKEN_RE = /^\$([A-Z0-9_]+)$/
export function resolveEnvToken(value: string, env: NodeJS.ProcessEnv): string {
  const m = value.match(TOKEN_RE)
  return m ? (env[m[1]] ?? '') : value
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm test -- tests/config/env-path-resolution.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/config/resolve.ts tests/config/env-path-resolution.test.ts
git commit -m "feat(config): implement env token and path resolution"
```

### Task 4: Build Effective Config Factory + Validation

**Files:**
- Create: `src/config/errors.ts`
- Create: `src/config/build-effective-config.ts`
- Create: `tests/config/effective-config.test.ts`
- Test fixture usage: `src/domain/models.ts` (`WorkflowDefinition`)

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { buildEffectiveConfig } from '../../src/config/build-effective-config.js'

describe('effective config builder', () => {
  it('maps workflow front matter to typed config with defaults', () => {
    const config = buildEffectiveConfig({ tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' } }, { LINEAR_API_KEY: 'token' })
    expect(config.tracker.endpoint).toBe('https://api.linear.app/graphql')
    expect(config.polling.interval_ms).toBe(30000)
  })

  it('throws typed error for missing required values', () => {
    expect(() => buildEffectiveConfig({ tracker: { kind: 'linear' } }, {})).toThrow(/tracker.api_key/)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm test -- tests/config/effective-config.test.ts`  
Expected: FAIL with missing builder/error classes.

**Step 3: Write minimal implementation**

```ts
export class ConfigValidationError extends Error {
  constructor(public code: string, message: string) { super(message) }
}

export function buildEffectiveConfig(raw: Record<string, unknown>, env: NodeJS.ProcessEnv): EffectiveConfig {
  // compose defaults + coercion + resolution + required validation
  return effective
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm test -- tests/config/effective-config.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/config/errors.ts src/config/build-effective-config.ts tests/config/effective-config.test.ts
git commit -m "feat(config): add effective config builder and validation"
```

### Task 5: Implement Reloadable Provider with Last-Known-Good Fallback

**Files:**
- Create: `src/config/reloadable-provider.ts`
- Create: `tests/config/reloadable-provider.test.ts`
- Modify: `src/config/contracts.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { createReloadableConfigProvider } from '../../src/config/reloadable-provider.js'

describe('reloadable config provider', () => {
  it('keeps last known good snapshot when reload input is invalid', async () => {
    const provider = createReloadableConfigProvider(validWorkflow, env)
    const before = provider.getSnapshot()

    const result = await provider.reload(invalidWorkflow)
    expect(result.applied).toBe(false)
    expect(provider.getSnapshot()).toEqual(before)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm test -- tests/config/reloadable-provider.test.ts`  
Expected: FAIL with missing provider implementation.

**Step 3: Write minimal implementation**

```ts
let current = buildEffectiveConfig(initial.config, env)
return {
  getSnapshot: () => structuredClone(current),
  async reload(nextWorkflow) {
    try {
      current = buildEffectiveConfig(nextWorkflow.config, env)
      return { applied: true as const }
    } catch (error) {
      return { applied: false as const, error }
    }
  },
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm test -- tests/config/reloadable-provider.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/config/reloadable-provider.ts src/config/contracts.ts tests/config/reloadable-provider.test.ts
git commit -m "feat(config): add reloadable provider with lkg fallback"
```

### Task 6: Wire Config Provider into Bootstrap and Add Integration Assertions

**Files:**
- Modify: `src/bootstrap/service.ts`
- Modify: `tests/bootstrap/service-wiring.test.ts`
- Create: `tests/config/reload-boundary.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { createService } from '../../src/bootstrap/service.js'

describe('bootstrap config integration', () => {
  it('exposes typed snapshot sections', () => {
    const service = createService()
    const snapshot = service.config.getSnapshot()
    expect(snapshot.tracker.kind).toBe('linear')
    expect(snapshot.agent.max_retry_backoff_ms).toBeGreaterThan(0)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm test -- tests/bootstrap/service-wiring.test.ts tests/config/reload-boundary.test.ts`  
Expected: FAIL with outdated bootstrap config shape.

**Step 3: Write minimal implementation**

```ts
const config = createStaticConfigProvider(buildEffectiveConfig(workflow.config, process.env))
```

**Step 4: Run test to verify it passes**

Run: `pnpm test -- tests/bootstrap/service-wiring.test.ts tests/config/reload-boundary.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add src/bootstrap/service.ts tests/bootstrap/service-wiring.test.ts tests/config/reload-boundary.test.ts
git commit -m "feat(bootstrap): wire typed config snapshot into service bootstrap"
```

### Task 7: Full Verification and Documentation Sync

**Files:**
- Modify: `docs/plans/2026-03-05-kat-223-typed-config-layer-design.md` (only if implementation decisions diverge)
- Modify: `PLANS.md` (status line for KAT-223 when ready)
- Create: `docs/generated/kat-223-verification.md`

**Step 1: Write the failing verification artifact stub**

```md
# KAT-223 Verification
- [ ] lint
- [ ] typecheck
- [ ] test
- [ ] make check
```

**Step 2: Run verification commands**

Run:
- `pnpm run lint`
- `pnpm run typecheck`
- `pnpm test`
- `make check`

Expected: all PASS.

**Step 3: Fill verification artifact with real outputs**

```md
- lint: PASS
- typecheck: PASS
- test: PASS (N tests)
- make check: PASS
```

**Step 4: Re-run targeted config tests for regression confidence**

Run: `pnpm test -- tests/config tests/bootstrap/service-wiring.test.ts`  
Expected: PASS.

**Step 5: Commit**

```bash
git add docs/generated/kat-223-verification.md PLANS.md docs/plans/2026-03-05-kat-223-typed-config-layer-design.md
git commit -m "docs: add kat-223 verification evidence and plan sync"
```

## Final Acceptance Checklist (KAT-223)

- Defaults/coercion match Section `6.4` (asserted by `tests/config/defaults-coercion.test.ts` and `tests/config/effective-config.test.ts`)
- Typed getters exist for tracker/polling/workspace/hooks/agent/codex
- Reload applies to future dispatch/retry/reconciliation/hook/agent boundaries
- Invalid reload does not crash process and keeps prior effective config
- Harness gate remains green (`make check`)
