import { describe, expect, it, vi } from 'vitest'
import type { Issue } from '../../src/domain/models.js'
import type { DispatchPreflightError } from '../../src/orchestrator/preflight/contracts.js'
import type {
  DispatchSelectionOptions,
  OrchestratorState,
  RunningEntry,
} from '../../src/orchestrator/runtime/index.js'
import {
  claimRunningIssue,
  runPollTick,
} from '../../src/orchestrator/runtime/index.js'

function createIssue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: 'issue-1',
    identifier: 'KAT-230',
    title: 'Run poll tick',
    description: null,
    priority: 2,
    state: 'Todo',
    branch_name: null,
    url: null,
    labels: [],
    blocked_by: [],
    created_at: '2026-03-07T00:00:00Z',
    updated_at: '2026-03-07T00:00:00Z',
    ...overrides,
  }
}

function createState(overrides: Partial<OrchestratorState> = {}): OrchestratorState {
  return {
    poll_interval_ms: 30_000,
    max_concurrent_agents: 2,
    running: new Map(),
    claimed: new Set(),
    retry_attempts: new Map(),
    completed: new Set(),
    codex_totals: {
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      seconds_running: 0,
    },
    codex_rate_limits: null,
    ...overrides,
  }
}

function createRunningEntry(issue: Issue): RunningEntry {
  return {
    issue,
    identifier: issue.identifier,
    workerPromise: null,
    retry_attempt: null,
    started_at: '2026-03-07T00:00:00Z',
    session_id: null,
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
  }
}

const selection: DispatchSelectionOptions = {
  activeStates: ['Todo', 'In Progress'],
  terminalStates: ['Done', 'Cancelled'],
  perStateLimits: {},
}

describe('runPollTick', () => {
  it('reconciles before validate, validates before fetch, and dispatches in sorted order', async () => {
    const state = createState()
    const later = createIssue({
      id: 'issue-2',
      identifier: 'KAT-300',
      priority: 2,
      created_at: '2026-03-07T02:00:00Z',
    })
    const earlier = createIssue({
      id: 'issue-3',
      identifier: 'KAT-100',
      priority: 1,
      created_at: '2026-03-07T01:00:00Z',
    })

    const reconcile = vi.fn(async (current: OrchestratorState) => current)
    const validate = vi.fn(async () => ({ ok: true as const }))
    const fetchCandidates = vi.fn(async () => [later, earlier])
    const dispatchIssue = vi.fn(async (current: OrchestratorState, issue: Issue) =>
      claimRunningIssue(current, issue, {
        workerPromise: null,
        retry_attempt: null,
        started_at: '2026-03-07T00:00:00Z',
      }),
    )

    const result = await runPollTick({
      state,
      selection,
      reconcile,
      validate,
      logFailure: vi.fn(),
      fetchCandidates,
      dispatchIssue,
    })

    expect(reconcile.mock.invocationCallOrder[0]).toBeLessThan(
      validate.mock.invocationCallOrder[0] as number,
    )
    expect(validate.mock.invocationCallOrder[0] as number).toBeLessThan(
      fetchCandidates.mock.invocationCallOrder[0] as number,
    )
    expect(dispatchIssue).toHaveBeenNthCalledWith(1, expect.anything(), earlier, null)
    expect(dispatchIssue).toHaveBeenNthCalledWith(2, expect.anything(), later, null)
    expect(result.running.size).toBe(2)
  })

  it('skips fetch and dispatch when validation fails', async () => {
    const reconciledState = createState({
      completed: new Set(['issue-9']),
    })
    const errors: DispatchPreflightError[] = [
      {
        code: 'workflow_invalid',
        source: 'workflow',
        field: 'workflow',
        message: 'bad workflow',
      },
    ]
    const reconcile = vi.fn(async () => reconciledState)
    const validate = vi.fn(async () => ({
      ok: false as const,
      errors,
    }))
    const fetchCandidates = vi.fn(async () => [])
    const dispatchIssue = vi.fn()

    const result = await runPollTick({
      state: createState(),
      selection,
      reconcile,
      validate,
      logFailure: vi.fn(),
      fetchCandidates,
      dispatchIssue,
    })

    expect(fetchCandidates).not.toHaveBeenCalled()
    expect(dispatchIssue).not.toHaveBeenCalled()
    expect(result).toBe(reconciledState)
  })

  it('returns reconciled state unchanged when fetching candidates fails', async () => {
    const reconciledState = createState({
      completed: new Set(['issue-9']),
    })
    const fetchCandidates = vi.fn(async () => {
      throw new Error('tracker down')
    })

    const result = await runPollTick({
      state: createState(),
      selection,
      reconcile: vi.fn(async () => reconciledState),
      validate: vi.fn(async () => ({ ok: true as const })),
      logFailure: vi.fn(),
      fetchCandidates,
      dispatchIssue: vi.fn(),
    })

    expect(result).toBe(reconciledState)
  })

  it('stops dispatching once slots are exhausted', async () => {
    const state = createState({
      max_concurrent_agents: 1,
    })
    const first = createIssue({
      id: 'issue-2',
      identifier: 'KAT-100',
      priority: 1,
    })
    const second = createIssue({
      id: 'issue-3',
      identifier: 'KAT-200',
      priority: 2,
    })
    const dispatchIssue = vi.fn(async (current: OrchestratorState, issue: Issue) =>
      claimRunningIssue(current, issue, {
        workerPromise: null,
        retry_attempt: null,
        started_at: '2026-03-07T00:00:00Z',
      }),
    )

    const result = await runPollTick({
      state,
      selection,
      reconcile: vi.fn(async (current) => current),
      validate: vi.fn(async () => ({ ok: true as const })),
      logFailure: vi.fn(),
      fetchCandidates: vi.fn(async () => [first, second]),
      dispatchIssue,
    })

    expect(dispatchIssue).toHaveBeenCalledTimes(1)
    expect(dispatchIssue).toHaveBeenCalledWith(expect.anything(), first, null)
    expect(result.running.has(first.id)).toBe(true)
    expect(result.running.has(second.id)).toBe(false)
  })

  it('skips already claimed or running issues before dispatching the next eligible issue', async () => {
    const claimedIssue = createIssue({
      id: 'issue-2',
      identifier: 'KAT-150',
      priority: 1,
    })
    const runningIssue = createIssue({
      id: 'issue-3',
      identifier: 'KAT-160',
      priority: 2,
      state: 'In Progress',
    })
    const eligibleIssue = createIssue({
      id: 'issue-4',
      identifier: 'KAT-170',
      priority: 3,
    })
    const state = createState({
      claimed: new Set([claimedIssue.id]),
      running: new Map([[runningIssue.id, createRunningEntry(runningIssue)]]),
    })
    const dispatchIssue = vi.fn(async (current: OrchestratorState, issue: Issue) =>
      claimRunningIssue(current, issue, {
        workerPromise: null,
        retry_attempt: null,
        started_at: '2026-03-07T00:00:00Z',
      }),
    )

    const result = await runPollTick({
      state,
      selection,
      reconcile: vi.fn(async (current) => current),
      validate: vi.fn(async () => ({ ok: true as const })),
      logFailure: vi.fn(),
      fetchCandidates: vi.fn(async () => [claimedIssue, runningIssue, eligibleIssue]),
      dispatchIssue,
    })

    expect(dispatchIssue).toHaveBeenCalledTimes(1)
    expect(dispatchIssue).toHaveBeenCalledWith(expect.anything(), eligibleIssue, null)
    expect(result.running.has(eligibleIssue.id)).toBe(true)
  })
})
