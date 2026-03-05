# KAT-221 Service Skeleton and Core Domain Model Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement spec-aligned core domain contracts and a runnable dependency-wired service skeleton that compiles/runs without orchestration logic enabled.

**Architecture:** Keep Section 4 entities as spec-native TypeScript contracts in a shared domain module, then layer interface contracts for `config`, `tracker`, `execution`, `orchestrator`, and `observability` with strict dependency direction. Build a bootstrap assembly path (`createService`/`startService`) that wires no-op implementations and preserves a clean seam for KAT-222/KAT-223/KAT-224.

**Tech Stack:** Node 22, TypeScript, Vitest, pnpm, ESLint, tsx

---

Related skills during execution: `@executing-plans`, `@test-driven-development`, `@verification-before-completion`.

### Task 1: Define failing domain contract tests for Section 4 entities

**Files:**
- Create: `tests/domain/core-domain-models.test.ts`
- Test: `tests/domain/core-domain-models.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import type {
  Issue,
  WorkflowDefinition,
  Workspace,
  RunAttempt,
  LiveSession,
  RetryEntry,
  OrchestratorRuntimeState,
} from '../../src/domain/models'

const issueFixture: Issue = {
  id: 'issue-1',
  identifier: 'KAT-221',
  title: 'Bootstrap service skeleton',
  description: 'spec mapping',
  priority: 1,
  state: 'Todo',
  branch_name: 'feature/kat-221',
  url: 'https://linear.app/kata-sh/issue/KAT-221',
  labels: ['area:symphony'],
  blocked_by: [{ id: 'issue-0', identifier: 'KAT-255', state: 'Done' }],
  created_at: '2026-03-05T00:00:00Z',
  updated_at: '2026-03-05T00:00:00Z',
}

describe('core domain model contracts', () => {
  it('supports Section 4 Issue + WorkflowDefinition shape', () => {
    const workflow: WorkflowDefinition = {
      config: { polling: { interval_ms: 30000 } },
      prompt_template: 'Issue: {{ issue.identifier }}',
    }

    expect(issueFixture.identifier).toBe('KAT-221')
    expect(workflow.prompt_template).toContain('{{ issue.identifier }}')
  })

  it('supports Section 4 runtime entities', () => {
    const workspace: Workspace = {
      path: '/tmp/symphony/KAT-221',
      workspace_key: 'KAT-221',
      created_now: true,
    }

    const attempt: RunAttempt = {
      issue_id: 'issue-1',
      issue_identifier: 'KAT-221',
      attempt: null,
      workspace_path: workspace.path,
      started_at: '2026-03-05T00:00:00Z',
      status: 'running',
    }

    const session: LiveSession = {
      session_id: 'thread-1-turn-1',
      thread_id: 'thread-1',
      turn_id: 'turn-1',
      codex_app_server_pid: null,
      last_codex_event: null,
      last_codex_timestamp: null,
      last_codex_message: null,
      codex_input_tokens: 0,
      codex_output_tokens: 0,
      codex_total_tokens: 0,
      last_reported_input_tokens: 0,
      last_reported_output_tokens: 0,
      last_reported_total_tokens: 0,
      turn_count: 0,
    }

    const retry: RetryEntry = {
      issue_id: 'issue-1',
      identifier: 'KAT-221',
      attempt: 1,
      due_at_ms: 0,
      timer_handle: null,
      error: null,
    }

    const runtime: OrchestratorRuntimeState = {
      poll_interval_ms: 30000,
      max_concurrent_agents: 5,
      running: new Map(),
      claimed: new Set(),
      retry_attempts: new Map(),
      completed: new Set(),
      codex_totals: { input_tokens: 0, output_tokens: 0, total_tokens: 0, seconds_running: 0 },
      codex_rate_limits: null,
    }

    expect(attempt.issue_identifier).toBe('KAT-221')
    expect(session.turn_count).toBe(0)
    expect(retry.attempt).toBe(1)
    expect(runtime.max_concurrent_agents).toBe(5)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/domain/core-domain-models.test.ts`
Expected: FAIL with module-not-found for `src/domain/models`.

**Step 3: Commit**

```bash
git add tests/domain/core-domain-models.test.ts
git commit -m "test: add failing section-4 domain contract tests"
```

### Task 2: Implement domain models and exports

**Files:**
- Create: `src/domain/models.ts`
- Create: `src/domain/index.ts`
- Modify: `tests/domain/core-domain-models.test.ts`
- Test: `tests/domain/core-domain-models.test.ts`

**Step 1: Write minimal implementation**

```ts
// src/domain/models.ts
export interface BlockerRef {
  id: string | null
  identifier: string | null
  state: string | null
}

export interface Issue {
  id: string
  identifier: string
  title: string
  description: string | null
  priority: number | null
  state: string
  branch_name: string | null
  url: string | null
  labels: string[]
  blocked_by: BlockerRef[]
  created_at: string | null
  updated_at: string | null
}

export interface WorkflowDefinition {
  config: Record<string, unknown>
  prompt_template: string
}

export interface Workspace {
  path: string
  workspace_key: string
  created_now: boolean
}

export interface RunAttempt {
  issue_id: string
  issue_identifier: string
  attempt: number | null
  workspace_path: string
  started_at: string
  status: string
  error?: string
}

export interface LiveSession {
  session_id: string
  thread_id: string
  turn_id: string
  codex_app_server_pid: string | null
  last_codex_event: string | null
  last_codex_timestamp: string | null
  last_codex_message: string | null
  codex_input_tokens: number
  codex_output_tokens: number
  codex_total_tokens: number
  last_reported_input_tokens: number
  last_reported_output_tokens: number
  last_reported_total_tokens: number
  turn_count: number
}

export interface RetryEntry {
  issue_id: string
  identifier: string
  attempt: number
  due_at_ms: number
  timer_handle: unknown
  error: string | null
}

export interface CodexTotals {
  input_tokens: number
  output_tokens: number
  total_tokens: number
  seconds_running: number
}

export interface OrchestratorRuntimeState {
  poll_interval_ms: number
  max_concurrent_agents: number
  running: Map<string, unknown>
  claimed: Set<string>
  retry_attempts: Map<string, RetryEntry>
  completed: Set<string>
  codex_totals: CodexTotals
  codex_rate_limits: unknown
}
```

**Step 2: Run test to verify it passes**

Run: `pnpm vitest tests/domain/core-domain-models.test.ts`
Expected: PASS.

**Step 3: Run typecheck**

Run: `pnpm run typecheck`
Expected: PASS.

**Step 4: Commit**

```bash
git add src/domain/models.ts src/domain/index.ts tests/domain/core-domain-models.test.ts
git commit -m "feat: add core domain model contracts for section 4"
```

### Task 3: Add failing normalization rule tests

**Files:**
- Create: `tests/domain/normalization.test.ts`
- Test: `tests/domain/normalization.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import {
  normalizeIssueState,
  sanitizeWorkspaceKey,
  makeSessionId,
} from '../../src/domain/normalization'

describe('domain normalization rules', () => {
  it('sanitizes workspace key per allowlist', () => {
    expect(sanitizeWorkspaceKey('KAT-221/fix*scope')).toBe('KAT-221_fix_scope')
  })

  it('normalizes state by trim + lowercase', () => {
    expect(normalizeIssueState('  In Progress  ')).toBe('in progress')
  })

  it('builds session id as <thread_id>-<turn_id>', () => {
    expect(makeSessionId('thread-1', 'turn-3')).toBe('thread-1-turn-3')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/domain/normalization.test.ts`
Expected: FAIL with module-not-found for `src/domain/normalization`.

**Step 3: Commit**

```bash
git add tests/domain/normalization.test.ts
git commit -m "test: add failing domain normalization rule tests"
```

### Task 4: Implement normalization helpers

**Files:**
- Create: `src/domain/normalization.ts`
- Modify: `src/domain/index.ts`
- Test: `tests/domain/normalization.test.ts`

**Step 1: Write minimal implementation**

```ts
// src/domain/normalization.ts
const WORKSPACE_KEY_INVALID = /[^A-Za-z0-9._-]/g

export function sanitizeWorkspaceKey(identifier: string): string {
  return identifier.replace(WORKSPACE_KEY_INVALID, '_')
}

export function normalizeIssueState(state: string): string {
  return state.trim().toLowerCase()
}

export function makeSessionId(threadId: string, turnId: string): string {
  return `${threadId}-${turnId}`
}
```

**Step 2: Run tests to verify they pass**

Run: `pnpm vitest tests/domain/normalization.test.ts tests/domain/core-domain-models.test.ts`
Expected: PASS.

**Step 3: Commit**

```bash
git add src/domain/normalization.ts src/domain/index.ts tests/domain/normalization.test.ts
git commit -m "feat: add domain normalization helpers"
```

### Task 5: Add failing layer contract tests (Section 3 boundaries)

**Files:**
- Create: `tests/contracts/layer-contracts.test.ts`
- Test: `tests/contracts/layer-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

import { createStaticConfigProvider } from '../../src/config/contracts'
import type { TrackerClient } from '../../src/tracker/contracts'
import type { WorkspaceManager, AgentRunner } from '../../src/execution/contracts'
import type { Logger } from '../../src/observability/contracts'
import { createNoopOrchestrator } from '../../src/orchestrator/contracts'

describe('layer contract surface', () => {
  it('exports required layer contracts', () => {
    expect(typeof createStaticConfigProvider).toBe('function')
    expect(typeof createNoopOrchestrator).toBe('function')

    const _tracker: TrackerClient | null = null
    const _workspace: WorkspaceManager | null = null
    const _agent: AgentRunner | null = null
    const _logger: Logger | null = null

    expect(_tracker).toBeNull()
    expect(_workspace).toBeNull()
    expect(_agent).toBeNull()
    expect(_logger).toBeNull()
  })

  it('keeps domain model free of layer imports', () => {
    const source = readFileSync('src/domain/models.ts', 'utf8')
    expect(source).not.toMatch(/from '..\/config/)
    expect(source).not.toMatch(/from '..\/tracker/)
    expect(source).not.toMatch(/from '..\/orchestrator/)
    expect(source).not.toMatch(/from '..\/execution/)
    expect(source).not.toMatch(/from '..\/observability/)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/contracts/layer-contracts.test.ts`
Expected: FAIL with missing module exports for layer contracts.

**Step 3: Commit**

```bash
git add tests/contracts/layer-contracts.test.ts
git commit -m "test: add failing layer contract boundary tests"
```

### Task 6: Implement module contracts for config/tracker/execution/orchestrator/observability

**Files:**
- Create: `src/config/contracts.ts`
- Create: `src/tracker/contracts.ts`
- Create: `src/execution/contracts.ts`
- Create: `src/orchestrator/contracts.ts`
- Create: `src/observability/contracts.ts`
- Modify: `tests/contracts/layer-contracts.test.ts`
- Test: `tests/contracts/layer-contracts.test.ts`

**Step 1: Write minimal implementation**

```ts
// src/config/contracts.ts
export interface ConfigSnapshot {
  poll_interval_ms: number
  max_concurrent_agents: number
}

export interface ConfigProvider {
  getSnapshot(): ConfigSnapshot
}

export function createStaticConfigProvider(snapshot: ConfigSnapshot): ConfigProvider {
  return {
    getSnapshot: () => snapshot,
  }
}
```

```ts
// src/tracker/contracts.ts
import type { Issue } from '../domain/models'

export interface TrackerClient {
  fetchCandidates(): Promise<Issue[]>
  fetchIssueStatesByIds(issueIds: string[]): Promise<Issue[]>
  fetchTerminalIssues(): Promise<Issue[]>
}
```

```ts
// src/execution/contracts.ts
import type { Issue, LiveSession, RunAttempt, Workspace } from '../domain/models'

export interface WorkspaceManager {
  ensureWorkspace(issueIdentifier: string): Promise<Workspace>
}

export interface AgentRunner {
  runAttempt(issue: Issue, attempt: number | null): Promise<{ attempt: RunAttempt; session: LiveSession | null }>
}
```

```ts
// src/observability/contracts.ts
export interface Logger {
  info(message: string, context?: Record<string, unknown>): void
  error(message: string, context?: Record<string, unknown>): void
}
```

```ts
// src/orchestrator/contracts.ts
import type { ConfigProvider } from '../config/contracts'
import type { AgentRunner, WorkspaceManager } from '../execution/contracts'
import type { Logger } from '../observability/contracts'
import type { TrackerClient } from '../tracker/contracts'

export interface Orchestrator {
  start(): Promise<void>
  stop(): Promise<void>
}

export interface OrchestratorDeps {
  config: ConfigProvider
  tracker: TrackerClient
  workspace: WorkspaceManager
  agentRunner: AgentRunner
  logger: Logger
}

export function createNoopOrchestrator(_: OrchestratorDeps): Orchestrator {
  return {
    async start() {},
    async stop() {},
  }
}
```

**Step 2: Run test to verify it passes**

Run: `pnpm vitest tests/contracts/layer-contracts.test.ts`
Expected: PASS.

**Step 3: Run typecheck**

Run: `pnpm run typecheck`
Expected: PASS.

**Step 4: Commit**

```bash
git add src/config/contracts.ts src/tracker/contracts.ts src/execution/contracts.ts src/orchestrator/contracts.ts src/observability/contracts.ts tests/contracts/layer-contracts.test.ts
git commit -m "feat: add section-3 layer contracts and no-op orchestrator"
```

### Task 7: Add failing startup wiring test for bootstrap shell

**Files:**
- Create: `tests/bootstrap/service-wiring.test.ts`
- Test: `tests/bootstrap/service-wiring.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { createService, startService } from '../../src/bootstrap/service'

describe('service bootstrap wiring', () => {
  it('creates dependency graph and starts without orchestration loop', async () => {
    const service = createService()

    expect(service.config.getSnapshot().poll_interval_ms).toBeGreaterThan(0)
    await expect(startService(service)).resolves.toBeUndefined()
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/bootstrap/service-wiring.test.ts`
Expected: FAIL with module-not-found for `src/bootstrap/service`.

**Step 3: Commit**

```bash
git add tests/bootstrap/service-wiring.test.ts
git commit -m "test: add failing bootstrap service wiring test"
```

### Task 8: Implement bootstrap assembly path and main entrypoint wiring

**Files:**
- Create: `src/bootstrap/service.ts`
- Modify: `src/main.ts`
- Modify: `tests/bootstrap/startup.test.ts`
- Test: `tests/bootstrap/service-wiring.test.ts`
- Test: `tests/bootstrap/startup.test.ts`

**Step 1: Write minimal implementation**

```ts
// src/bootstrap/service.ts
import { createStaticConfigProvider } from '../config/contracts'
import { createNoopOrchestrator } from '../orchestrator/contracts'
import type { Logger } from '../observability/contracts'

const logger: Logger = {
  info(message, context) {
    console.log(message, context ?? {})
  },
  error(message, context) {
    console.error(message, context ?? {})
  },
}

export function createService() {
  const config = createStaticConfigProvider({
    poll_interval_ms: 30000,
    max_concurrent_agents: 5,
  })

  const tracker = {
    async fetchCandidates() {
      return []
    },
    async fetchIssueStatesByIds() {
      return []
    },
    async fetchTerminalIssues() {
      return []
    },
  }

  const workspace = {
    async ensureWorkspace(issueIdentifier: string) {
      return {
        path: `/tmp/symphony/${issueIdentifier}`,
        workspace_key: issueIdentifier,
        created_now: false,
      }
    },
  }

  const agentRunner = {
    async runAttempt() {
      throw new Error('agent runner not enabled in bootstrap mode')
    },
  }

  const orchestrator = createNoopOrchestrator({ config, tracker, workspace, agentRunner, logger })

  return { config, tracker, workspace, agentRunner, logger, orchestrator }
}

export async function startService(service = createService()): Promise<void> {
  await service.orchestrator.start()
  service.logger.info('Symphony bootstrap ok', {
    mode: 'bootstrap',
    orchestration_enabled: false,
  })
}
```

```ts
// src/main.ts
import { startService } from './bootstrap/service'

void startService()
```

**Step 2: Run tests to verify they pass**

Run: `pnpm vitest tests/bootstrap/service-wiring.test.ts tests/bootstrap/startup.test.ts`
Expected: PASS.

**Step 3: Run startup smoke**

Run: `pnpm start`
Expected: output includes `Symphony bootstrap ok`, exit `0`.

**Step 4: Commit**

```bash
git add src/bootstrap/service.ts src/main.ts tests/bootstrap/service-wiring.test.ts tests/bootstrap/startup.test.ts
git commit -m "feat: wire bootstrap service assembly and startup path"
```

### Task 9: Update architecture/docs and capture verification evidence

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `PLANS.md`
- Create: `docs/generated/kat-221-verification.md`

**Step 1: Add docs updates**

- `ARCHITECTURE.md`: add concrete TypeScript module path mapping for each Section 3 layer.
- `PLANS.md`: mark KAT-221 as active/in progress with brief scope.
- `docs/generated/kat-221-verification.md`: add acceptance criteria checklist mapped to executed commands.

**Step 2: Run full checks**

Run: `make check`
Expected: PASS.

**Step 3: Run explicit toolchain checks**

Run: `pnpm run lint && pnpm run typecheck && pnpm test`
Expected: PASS.

**Step 4: Commit**

```bash
git add ARCHITECTURE.md PLANS.md docs/generated/kat-221-verification.md
git commit -m "docs: record kat-221 architecture updates and verification evidence"
```

### Task 10: Final readiness pass and PR handoff note

**Files:**
- Create: `docs/generated/kat-221-pr-summary.md`

**Step 1: Draft PR summary**

Include:
- acceptance criteria mapping
- module boundary summary
- startup wiring evidence
- risks/follow-ups for KAT-222/KAT-223/KAT-224

**Step 2: Final gate run**

Run: `make check`
Expected: PASS.

**Step 3: Commit**

```bash
git add docs/generated/kat-221-pr-summary.md
git commit -m "docs: add kat-221 pr handoff summary"
```
