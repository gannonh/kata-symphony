# KAT-229 Worker Attempt Pipeline Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the Section 16.5 worker attempt pipeline so Symphony can create/reuse a workspace, build first-turn and continuation prompts, stream agent turns on one live thread, refresh tracker state between turns, and return deterministic normal vs abnormal exit reasons.

**Architecture:** Add a dedicated execution-layer `worker-attempt` module that composes the existing workspace manager, prompt builder, tracker adapter, and app-server protocol code. Refactor the current `agent-runner` into a reusable session client that can start one session, run multiple turns on the same thread, expose session metadata/events, and stop cleanly so the worker-attempt coordinator owns the Section 16.5 loop.

**Tech Stack:** TypeScript (NodeNext), Vitest, existing execution/tracker/workspace modules, ESLint, repository harness via `make check`.

---

**Skill refs for execution:** `@test-driven-development`, `@verification-before-completion`, `@executing-plans`

### Task 1: Add Worker Attempt Contract Surface

**Files:**
- Create: `src/execution/worker-attempt/contracts.ts`
- Create: `src/execution/worker-attempt/index.ts`
- Modify: `src/execution/contracts.ts`
- Modify: `tests/contracts/layer-contracts.test.ts`
- Modify: `tests/contracts/runtime-modules.test.ts`
- Create: `tests/execution/worker-attempt/worker-attempt-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'

import type { WorkerAttemptRunner } from '../../../src/execution/contracts.js'
import {
  WORKER_ATTEMPT_OUTCOME_KINDS,
  WORKER_ATTEMPT_REASON_CODES,
} from '../../../src/execution/worker-attempt/contracts.js'

describe('worker attempt contracts', () => {
  it('exposes stable outcome kinds and reason codes', () => {
    expect(WORKER_ATTEMPT_OUTCOME_KINDS).toEqual(['normal', 'abnormal'])
    expect(WORKER_ATTEMPT_REASON_CODES).toEqual(
      expect.arrayContaining([
        'stopped_non_active_state',
        'stopped_max_turns_reached',
        'workspace_error',
        'before_run_hook_error',
        'agent_session_startup_error',
        'prompt_error',
        'agent_turn_error',
        'issue_state_refresh_error',
      ]),
    )
  })

  it('adds a worker-attempt runner contract to the execution layer', () => {
    const runner = {} as WorkerAttemptRunner
    expect(typeof runner.run).toBe('function')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/worker-attempt/worker-attempt-contracts.test.ts tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts`
Expected: FAIL because the worker-attempt contract module and execution export do not exist yet.

**Step 3: Write minimal implementation**

```ts
// src/execution/worker-attempt/contracts.ts
import type { Issue, LiveSession, RunAttempt } from '../../domain/models.js'

export const WORKER_ATTEMPT_OUTCOME_KINDS = ['normal', 'abnormal'] as const
export const WORKER_ATTEMPT_REASON_CODES = [
  'stopped_non_active_state',
  'stopped_max_turns_reached',
  'workspace_error',
  'before_run_hook_error',
  'agent_session_startup_error',
  'prompt_error',
  'agent_turn_error',
  'issue_state_refresh_error',
] as const

export type WorkerAttemptOutcomeKind = (typeof WORKER_ATTEMPT_OUTCOME_KINDS)[number]
export type WorkerAttemptReasonCode = (typeof WORKER_ATTEMPT_REASON_CODES)[number]

export interface WorkerAttemptOutcome {
  kind: WorkerAttemptOutcomeKind
  reason_code: WorkerAttemptReasonCode
  turns_executed: number
  final_issue_state: string | null
}

export interface WorkerAttemptResult {
  attempt: RunAttempt
  session: LiveSession | null
  outcome: WorkerAttemptOutcome
}

export interface WorkerAttemptRunner {
  run(issue: Issue, attempt: number | null): Promise<WorkerAttemptResult>
}
```

```ts
// src/execution/contracts.ts (excerpt)
export type {
  WorkerAttemptOutcome,
  WorkerAttemptResult,
  WorkerAttemptRunner,
} from './worker-attempt/contracts.js'

export {
  WORKER_ATTEMPT_OUTCOME_KINDS,
  WORKER_ATTEMPT_REASON_CODES,
} from './worker-attempt/contracts.js'
```

```ts
// src/execution/worker-attempt/index.ts
export * from './contracts.js'
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/worker-attempt/worker-attempt-contracts.test.ts tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/contracts.ts src/execution/worker-attempt/contracts.ts src/execution/worker-attempt/index.ts tests/execution/worker-attempt/worker-attempt-contracts.test.ts tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts
git commit -m "feat(execution): add worker attempt contract surface"
```

### Task 2: Refactor Agent Runner Into Reusable Session Lifecycle Primitives

**Files:**
- Create: `src/execution/agent-runner/session-client.ts`
- Modify: `src/execution/agent-runner/index.ts`
- Modify: `src/execution/agent-runner/runner.ts`
- Modify: `src/execution/agent-runner/protocol-client.ts`
- Modify: `src/execution/agent-runner/session-reducer.ts`
- Modify: `src/execution/agent-runner/errors.ts`
- Modify: `tests/execution/agent-runner/protocol-client.test.ts`
- Modify: `tests/execution/agent-runner/session-reducer.test.ts`
- Create: `tests/execution/agent-runner/session-client.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'

import { createAgentSessionClient } from '../../../src/execution/agent-runner/session-client.js'

describe('agent session client', () => {
  it('starts one session and runs multiple turns on the same thread', async () => {
    const client = createAgentSessionClient({
      codex: {
        command: 'echo noop',
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp/ws',
      spawnChild: vi.fn(),
    })

    expect(typeof client.startSession).toBe('function')
    expect(typeof client.runTurn).toBe('function')
    expect(typeof client.stopSession).toBe('function')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/agent-runner/session-client.test.ts tests/execution/agent-runner/protocol-client.test.ts tests/execution/agent-runner/session-reducer.test.ts`
Expected: FAIL because session-client lifecycle API does not exist and the reducer/protocol code only models one-shot startup.

**Step 3: Write minimal implementation**

```ts
// src/execution/agent-runner/session-client.ts (shape only)
export interface AgentSessionStart {
  threadId: string
  turnId: string
  sessionId: string
}

export interface AgentSessionClient {
  startSession(input: { title: string; prompt: string }): Promise<AgentSessionStart>
  runTurn(input: { threadId: string; title: string; prompt: string }): Promise<AgentSessionStart>
  stopSession(): Promise<void>
  getLatestSession(): LiveSession | null
}
```

```ts
// src/execution/agent-runner/protocol-client.ts (excerpt)
return {
  async initializeSession(...) { ... },
  async startThread(...) { ... },
  async startTurn(...) { ... },
}
```

```ts
// src/execution/agent-runner/session-reducer.ts (excerpt)
return {
  acceptMessage(message) { ... },
  resetForNextTurn() { ... },
  waitForTurnCompletion(timeoutMs) { ... },
  toLiveSession(session, pid, turnCount) { ... },
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/agent-runner/session-client.test.ts tests/execution/agent-runner/protocol-client.test.ts tests/execution/agent-runner/session-reducer.test.ts`
Expected: PASS with a reusable session lifecycle API that supports repeated `turn/start` calls on one thread.

**Step 5: Commit**

```bash
git add src/execution/agent-runner/index.ts src/execution/agent-runner/runner.ts src/execution/agent-runner/protocol-client.ts src/execution/agent-runner/session-reducer.ts src/execution/agent-runner/session-client.ts src/execution/agent-runner/errors.ts tests/execution/agent-runner/protocol-client.test.ts tests/execution/agent-runner/session-reducer.test.ts tests/execution/agent-runner/session-client.test.ts
git commit -m "feat(agent-runner): expose reusable session lifecycle for multi-turn attempts"
```

### Task 3: Add First-Turn and Continuation Prompt Helpers

**Files:**
- Create: `src/execution/worker-attempt/build-turn-prompt.ts`
- Modify: `src/execution/worker-attempt/index.ts`
- Create: `tests/execution/worker-attempt/build-turn-prompt.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'

import { buildTurnPrompt } from '../../../src/execution/worker-attempt/build-turn-prompt.js'

const issue = {
  id: '1',
  identifier: 'KAT-229',
  title: 'Worker attempt pipeline',
  description: null,
  priority: 1,
  state: 'In Progress',
  branch_name: null,
  url: null,
  labels: ['area:symphony'],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('buildTurnPrompt', () => {
  it('uses the workflow template for the first turn', async () => {
    const result = await buildTurnPrompt({
      template: 'Issue {{ issue.identifier }} attempt={{ attempt }}',
      issue,
      attempt: null,
      turnNumber: 1,
      maxTurns: 3,
    })

    expect(result).toEqual({ ok: true, prompt: 'Issue KAT-229 attempt=' })
  })

  it('uses short continuation guidance for later turns without repeating the workflow body', async () => {
    const result = await buildTurnPrompt({
      template: 'DO NOT REUSE THIS BODY',
      issue,
      attempt: 1,
      turnNumber: 2,
      maxTurns: 3,
    })

    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.prompt).toContain('Continue working on KAT-229')
      expect(result.prompt).not.toContain('DO NOT REUSE THIS BODY')
    }
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/worker-attempt/build-turn-prompt.test.ts`
Expected: FAIL because the turn prompt helper does not exist.

**Step 3: Write minimal implementation**

```ts
// src/execution/worker-attempt/build-turn-prompt.ts
import { createPromptBuilder } from '../prompt/index.js'

const CONTINUATION_TEMPLATE =
  'Continue working on {{ issue.identifier }} (turn {{ turn_number }} of {{ max_turns }}). Do not repeat prior setup; continue from the current thread state.'

export async function buildTurnPrompt(input: {
  template: string
  issue: Issue
  attempt: number | null
  turnNumber: number
  maxTurns: number
}) {
  const builder = createPromptBuilder()
  if (input.turnNumber === 1) {
    return builder.build({
      template: input.template,
      issue: input.issue,
      attempt: input.attempt,
    })
  }

  return builder.build({
    template: CONTINUATION_TEMPLATE
      .replace('{{ turn_number }}', String(input.turnNumber))
      .replace('{{ max_turns }}', String(input.maxTurns)),
    issue: input.issue,
    attempt: input.attempt,
  })
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/worker-attempt/build-turn-prompt.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/worker-attempt/build-turn-prompt.ts src/execution/worker-attempt/index.ts tests/execution/worker-attempt/build-turn-prompt.test.ts
git commit -m "feat(worker-attempt): add first-turn and continuation prompt policy"
```

### Task 4: Implement Single-Turn Worker Attempt Flow With Guaranteed `after_run`

**Files:**
- Create: `src/execution/worker-attempt/run-worker-attempt.ts`
- Modify: `src/execution/worker-attempt/index.ts`
- Create: `tests/execution/worker-attempt/run-worker-attempt.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'

import { createWorkerAttemptRunner } from '../../../src/execution/worker-attempt/run-worker-attempt.js'

const issue = {
  id: 'issue-1',
  identifier: 'KAT-229',
  title: 'Worker pipeline',
  description: null,
  priority: 1,
  state: 'In Progress',
  branch_name: null,
  url: null,
  labels: [],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('worker attempt runner single-turn flow', () => {
  it('creates workspace, runs before_run, executes one turn, and always runs after_run', async () => {
    const ensureWorkspace = vi.fn().mockResolvedValue({
      path: '/tmp/ws/KAT-229',
      workspace_key: 'KAT-229',
      created_now: false,
    })
    const runBeforeRun = vi.fn().mockResolvedValue(undefined)
    const runAfterRun = vi.fn().mockResolvedValue(undefined)
    const startSession = vi.fn().mockResolvedValue({
      threadId: 'thread-1',
      turnId: 'turn-1',
      sessionId: 'thread-1-turn-1',
    })
    const runTurn = vi.fn().mockResolvedValue({
      threadId: 'thread-1',
      turnId: 'turn-1',
      sessionId: 'thread-1-turn-1',
    })
    const stopSession = vi.fn().mockResolvedValue(undefined)

    const runner = createWorkerAttemptRunner({
      workspace: { ensureWorkspace, runBeforeRun, runAfterRun },
      tracker: { fetchIssuesByIds: vi.fn().mockResolvedValue([{ ...issue, state: 'Review' }]) },
      sessionClientFactory: () => ({
        startSession,
        runTurn,
        stopSession,
        getLatestSession: () => null,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
      onCodexEvent: vi.fn(),
    })

    const result = await runner.run(issue, null)
    expect(result.outcome).toMatchObject({
      kind: 'normal',
      reason_code: 'stopped_non_active_state',
    })
    expect(runAfterRun).toHaveBeenCalledTimes(1)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/worker-attempt/run-worker-attempt.test.ts`
Expected: FAIL because the worker-attempt runner does not exist.

**Step 3: Write minimal implementation**

```ts
// src/execution/worker-attempt/run-worker-attempt.ts (single-turn baseline)
export function createWorkerAttemptRunner(deps: WorkerAttemptRunnerDeps): WorkerAttemptRunner {
  return {
    async run(issue, attempt) {
      let workspace: Workspace | null = null
      let session: LiveSession | null = null
      let finalOutcome: WorkerAttemptOutcome = {
        kind: 'abnormal',
        reason_code: 'workspace_error',
        turns_executed: 0,
        final_issue_state: null,
      }

      try {
        workspace = await deps.workspace.ensureWorkspace(issue.identifier)
        await deps.workspace.runBeforeRun(workspace)
        const client = deps.sessionClientFactory(workspace.path)
        const prompt = await buildTurnPrompt({ ... })
        if (!prompt.ok) { ...prompt_error... }
        await client.startSession({ title: `${issue.identifier}: ${issue.title}`, prompt: prompt.prompt })
        const turn = await client.runTurn({ threadId: 'thread-1', title: `${issue.identifier}: ${issue.title}`, prompt: prompt.prompt })
        const refreshed = await deps.tracker.fetchIssuesByIds([issue.id])
        finalOutcome = { kind: 'normal', reason_code: 'stopped_non_active_state', turns_executed: 1, final_issue_state: refreshed[0]?.state ?? issue.state }
        session = client.getLatestSession()
        await client.stopSession()
        return { attempt: ..., session, outcome: finalOutcome }
      } finally {
        if (workspace) {
          await deps.workspace.runAfterRun(workspace).catch(() => {})
        }
      }
    },
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/worker-attempt/run-worker-attempt.test.ts`
Expected: PASS for the single-turn happy-path and `after_run` finally guarantee.

**Step 5: Commit**

```bash
git add src/execution/worker-attempt/run-worker-attempt.ts src/execution/worker-attempt/index.ts tests/execution/worker-attempt/run-worker-attempt.test.ts
git commit -m "feat(worker-attempt): add single-turn worker attempt flow with after_run guarantee"
```

### Task 5: Add Continuation Loop, Refresh/Error Mapping, and Event Streaming

**Files:**
- Modify: `src/execution/worker-attempt/run-worker-attempt.ts`
- Modify: `src/execution/agent-runner/session-client.ts`
- Modify: `tests/execution/worker-attempt/run-worker-attempt.test.ts`
- Modify: `tests/execution/agent-runner/session-client.test.ts`
- Modify: `tests/execution/agent-runner/agent-runner.test.ts`

**Step 1: Write the failing test**

```ts
it('reuses the same thread for continuation turns until the issue becomes non-active', async () => {
  const runTurn = vi
    .fn()
    .mockResolvedValueOnce({
      threadId: 'thread-1',
      turnId: 'turn-1',
      sessionId: 'thread-1-turn-1',
    })
    .mockResolvedValueOnce({
      threadId: 'thread-1',
      turnId: 'turn-2',
      sessionId: 'thread-1-turn-2',
    })

  const fetchIssuesByIds = vi
    .fn()
    .mockResolvedValueOnce([{ ...issue, state: 'In Progress' }])
    .mockResolvedValueOnce([{ ...issue, state: 'Review' }])

  const onCodexEvent = vi.fn()

  const runner = createWorkerAttemptRunner({
    ...baseDeps,
    tracker: { fetchIssuesByIds },
    onCodexEvent,
    sessionClientFactory: () => ({
      startSession: vi.fn().mockResolvedValue({
        threadId: 'thread-1',
        turnId: 'turn-1',
        sessionId: 'thread-1-turn-1',
      }),
      runTurn,
      stopSession: vi.fn().mockResolvedValue(undefined),
      getLatestSession: () => ({
        session_id: 'thread-1-turn-2',
        thread_id: 'thread-1',
        turn_id: 'turn-2',
        codex_app_server_pid: '123',
        last_codex_event: 'turn/completed',
        last_codex_timestamp: null,
        last_codex_message: null,
        codex_input_tokens: 0,
        codex_output_tokens: 0,
        codex_total_tokens: 0,
        last_reported_input_tokens: 0,
        last_reported_output_tokens: 0,
        last_reported_total_tokens: 0,
        turn_count: 2,
      }),
    }),
  })

  const result = await runner.run(issue, 1)
  expect(runTurn).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({ threadId: 'thread-1' }),
  )
  expect(result.outcome).toMatchObject({
    kind: 'normal',
    reason_code: 'stopped_non_active_state',
    turns_executed: 2,
  })
  expect(onCodexEvent).toHaveBeenCalled()
})

it('maps tracker refresh failure to abnormal issue_state_refresh_error and still runs after_run', async () => {
  ...
})

it('stops at maxTurns with normal stopped_max_turns_reached outcome', async () => {
  ...
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/worker-attempt/run-worker-attempt.test.ts tests/execution/agent-runner/session-client.test.ts tests/execution/agent-runner/agent-runner.test.ts`
Expected: FAIL because the current implementation only covers a one-turn baseline.

**Step 3: Write minimal implementation**

```ts
// src/execution/worker-attempt/run-worker-attempt.ts (loop excerpt)
let turnNumber = 1
let threadId: string | null = null

while (turnNumber <= deps.maxTurns) {
  const prompt = await buildTurnPrompt({
    template: deps.workflowTemplate,
    issue: currentIssue,
    attempt,
    turnNumber,
    maxTurns: deps.maxTurns,
  })

  if (!prompt.ok) {
    return abnormal('prompt_error', turnNumber - 1, currentIssue.state)
  }

  const turnStart =
    turnNumber === 1
      ? await client.startSession({ title, prompt: prompt.prompt })
      : await client.runTurn({ threadId: threadId as string, title, prompt: prompt.prompt })

  threadId = turnStart.threadId
  deps.onCodexEvent?.({
    issue_id: issue.id,
    issue_identifier: issue.identifier,
    event: 'turn_started',
    timestamp: new Date().toISOString(),
    session: client.getLatestSession(),
  })

  const refreshed = await deps.tracker.fetchIssuesByIds([issue.id]).catch(() => null)
  if (refreshed === null) {
    return abnormal('issue_state_refresh_error', turnNumber, currentIssue.state)
  }

  currentIssue = refreshed[0] ?? currentIssue
  if (!activeStateSet.has(currentIssue.state.trim().toLowerCase())) {
    return normal('stopped_non_active_state', turnNumber, currentIssue.state)
  }

  if (turnNumber >= deps.maxTurns) {
    return normal('stopped_max_turns_reached', turnNumber, currentIssue.state)
  }

  turnNumber += 1
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/worker-attempt/run-worker-attempt.test.ts tests/execution/agent-runner/session-client.test.ts tests/execution/agent-runner/agent-runner.test.ts`
Expected: PASS with continuation reuse, deterministic stop reasons, and refresh failure mapping.

**Step 5: Commit**

```bash
git add src/execution/worker-attempt/run-worker-attempt.ts src/execution/agent-runner/session-client.ts tests/execution/worker-attempt/run-worker-attempt.test.ts tests/execution/agent-runner/session-client.test.ts tests/execution/agent-runner/agent-runner.test.ts
git commit -m "feat(worker-attempt): add multi-turn continuation loop and outcome mapping"
```

### Task 6: Wire Worker Attempt Into Execution Exports and Verify End-to-End

**Files:**
- Modify: `src/execution/contracts.ts`
- Modify: `src/bootstrap/service.ts`
- Modify: `tests/contracts/layer-contracts.test.ts`
- Modify: `tests/bootstrap/service-wiring.test.ts`
- Modify: `docs/design-docs/2026-03-05-kat-229-worker-attempt-pipeline-design.md`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'

describe('service wiring', () => {
  it('builds a concrete worker attempt runner in bootstrap wiring', async () => {
    const { createService } = await import('../../src/bootstrap/service.js')
    const service = createService()

    expect(service).toHaveProperty('workerAttemptRunner')
    expect(typeof service.workerAttemptRunner.run).toBe('function')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/bootstrap/service-wiring.test.ts tests/contracts/layer-contracts.test.ts`
Expected: FAIL because bootstrap wiring and contract tests do not yet know about the concrete worker-attempt runner.

**Step 3: Write minimal implementation**

```ts
// src/bootstrap/service.ts (excerpt)
import { createWorkerAttemptRunner } from '../execution/worker-attempt/run-worker-attempt.js'
import { createPromptBuilder } from '../execution/prompt/index.js'
import { createLinearTrackerClient } from '../tracker/index.js'

const workerAttemptRunner = createWorkerAttemptRunner({
  workspace,
  tracker,
  workflowTemplate: loadWorkflowDefinition(...).prompt_template,
  activeStates: snapshot.tracker.active_states,
  maxTurns: snapshot.agent.max_turns,
  sessionClientFactory: (workspacePath) =>
    createAgentSessionClient({
      codex: snapshot.codex,
      workspacePath,
    }),
  onCodexEvent(event) {
    logger.info('worker_attempt_codex_event', event)
  },
})

return { ..., workerAttemptRunner }
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/bootstrap/service-wiring.test.ts tests/contracts/layer-contracts.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/execution/contracts.ts src/bootstrap/service.ts tests/contracts/layer-contracts.test.ts tests/bootstrap/service-wiring.test.ts docs/design-docs/2026-03-05-kat-229-worker-attempt-pipeline-design.md
git commit -m "feat(execution): wire worker attempt runner into bootstrap contracts"
```

### Task 7: Full Verification

**Files:**
- Verify only: `src/execution/worker-attempt/*`
- Verify only: `src/execution/agent-runner/*`
- Verify only: `src/bootstrap/service.ts`
- Verify only: `tests/execution/worker-attempt/*`
- Verify only: `tests/execution/agent-runner/*`

**Step 1: Run targeted tests**

Run: `pnpm vitest run tests/execution/worker-attempt tests/execution/agent-runner tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts tests/bootstrap/service-wiring.test.ts`
Expected: PASS.

**Step 2: Run repo verification**

Run: `make check`
Expected: PASS with lint, doc sync, and test gates green.

**Step 3: Inspect diff before handoff**

Run: `git status --short`
Expected: only KAT-229 implementation files plus any pre-existing unrelated user changes.

**Step 4: Commit verification-only follow-up if needed**

```bash
git add <only-if-needed>
git commit -m "test(execution): finalize worker attempt pipeline verification"
```

