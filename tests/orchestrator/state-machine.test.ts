import { describe, expect, it } from 'vitest'
import type { Issue } from '../../src/domain/models.js'
import type {
  OrchestratorState,
  WorkerExitIntent,
} from '../../src/orchestrator/runtime/index.js'
import {
  applyCodexUpdate,
  claimRunningIssue,
  createInitialOrchestratorState,
  deriveWorkerExitIntent,
  recordCompletion,
  releaseIssue,
} from '../../src/orchestrator/runtime/index.js'

function createIssue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: 'issue-1',
    identifier: 'KAT-230',
    title: 'Implement runtime state machine',
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
    max_concurrent_agents: 5,
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

describe('state-machine helpers', () => {
  it('creates the initial orchestrator state from config snapshot', () => {
    const state = createInitialOrchestratorState({
      tracker: {
        kind: 'linear',
        endpoint: 'https://api.linear.app/graphql',
        api_key: 'token',
        project_slug: 'proj',
        active_states: ['Todo'],
        terminal_states: ['Done'],
      },
      polling: { interval_ms: 15_000 },
      workspace: { root: '/tmp/symphony' },
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 60_000,
      },
      agent: {
        max_concurrent_agents: 4,
        max_turns: 6,
        max_retry_backoff_ms: 120_000,
        max_concurrent_agents_by_state: {},
      },
      codex: {
        command: 'codex app-server',
        turn_timeout_ms: 60_000,
        read_timeout_ms: 1_000,
        stall_timeout_ms: 10_000,
      },
      max_concurrent_agents: 4,
    })

    expect(state.poll_interval_ms).toBe(15_000)
    expect(state.max_concurrent_agents).toBe(4)
    expect(state.running.size).toBe(0)
    expect(state.claimed.size).toBe(0)
    expect(state.retry_attempts.size).toBe(0)
    expect(state.completed.size).toBe(0)
    expect(state.codex_totals.total_tokens).toBe(0)
    expect(state.codex_rate_limits).toBeNull()
  })

  it('claims a running issue and clears prior retry bookkeeping', () => {
    const issue = createIssue()
    const workerPromise = Promise.resolve({
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: null,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'running',
      },
      session: null,
      outcome: {
        kind: 'normal' as const,
        reason_code: 'stopped_non_active_state' as const,
        turns_executed: 0,
        final_issue_state: null,
      },
    })
    const state = createState({
      retry_attempts: new Map([
        [
          issue.id,
          {
            issue_id: issue.id,
            identifier: issue.identifier,
            attempt: 1,
            due_at_ms: 5_000,
            timer_handle: null,
            error: 'retry me',
          },
        ],
      ]),
    })

    const next = claimRunningIssue(state, issue, {
      workerPromise,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
    })

    expect(next).not.toBe(state)
    expect(next.claimed.has(issue.id)).toBe(true)
    expect(next.running.get(issue.id)).toMatchObject({
      identifier: issue.identifier,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
    })
    expect(next.running.get(issue.id)?.workerPromise).toBe(workerPromise)
    expect(next.retry_attempts.has(issue.id)).toBe(false)
    expect(state.claimed.has(issue.id)).toBe(false)
  })

  it('seeds running entries from an existing live session snapshot', () => {
    const issue = createIssue()

    const next = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: 1,
      started_at: '2026-03-07T00:00:00Z',
      session: {
        session_id: 'thread-1-turn-1',
        thread_id: 'thread-1',
        turn_id: 'turn-1',
        codex_app_server_pid: '321',
        last_codex_event: 'turn/completed',
        last_codex_timestamp: '2026-03-07T00:00:01Z',
        last_codex_message: 'hello',
        codex_input_tokens: 1,
        codex_output_tokens: 2,
        codex_total_tokens: 3,
        last_reported_input_tokens: 1,
        last_reported_output_tokens: 2,
        last_reported_total_tokens: 3,
        turn_count: 1,
      },
    })

    expect(next.running.get(issue.id)).toMatchObject({
      session_id: 'thread-1-turn-1',
      codex_app_server_pid: '321',
      last_codex_event: 'turn/completed',
      last_codex_timestamp: '2026-03-07T00:00:01Z',
      last_codex_message: 'hello',
      codex_total_tokens: 3,
      last_reported_total_tokens: 3,
    })
  })

  it('applies codex updates to running entries and aggregate totals', () => {
    const issue = createIssue()
    const claimed = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
    })

    const updated = applyCodexUpdate(claimed, issue.id, {
      session: {
        session_id: 'thread-1-turn-1',
        codex_input_tokens: 4,
        codex_output_tokens: 5,
        codex_total_tokens: 9,
        last_reported_input_tokens: 4,
        last_reported_output_tokens: 5,
        last_reported_total_tokens: 9,
      },
      rate_limits: { requests_remaining: 10 },
    })

    expect(updated.running.get(issue.id)).toMatchObject({
      session_id: 'thread-1-turn-1',
      codex_total_tokens: 9,
      last_reported_total_tokens: 9,
    })
    expect(updated.codex_totals).toMatchObject({
      input_tokens: 4,
      output_tokens: 5,
      total_tokens: 9,
    })
    expect(updated.codex_rate_limits).toEqual({ requests_remaining: 10 })

    const updatedAgain = applyCodexUpdate(updated, issue.id, {
      session: {
        codex_input_tokens: 6,
        codex_output_tokens: 8,
        codex_total_tokens: 14,
        last_reported_input_tokens: 6,
        last_reported_output_tokens: 8,
        last_reported_total_tokens: 14,
      },
    })

    expect(updatedAgain.codex_totals).toMatchObject({
      input_tokens: 6,
      output_tokens: 8,
      total_tokens: 14,
    })
  })

  it('applies rate limits even when the issue is not currently running', () => {
    const state = createState()

    const updated = applyCodexUpdate(state, 'missing-issue', {
      rate_limits: { requests_remaining: 8 },
    })

    expect(updated.codex_rate_limits).toEqual({ requests_remaining: 8 })
    expect(updated.running.size).toBe(0)
  })

  it('keeps running entry values intact when codex update has no session payload', () => {
    const issue = createIssue()
    const claimed = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
    })

    const updated = applyCodexUpdate(claimed, issue.id, {
      rate_limits: { requests_remaining: 7 },
    })

    expect(updated.running.get(issue.id)).toMatchObject({
      session_id: null,
      codex_total_tokens: 0,
      last_reported_total_tokens: 0,
    })
    expect(updated.codex_rate_limits).toEqual({ requests_remaining: 7 })
  })

  it('preserves omitted live-session fields during partial codex updates', () => {
    const issue = createIssue()
    const claimed = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
      session: {
        session_id: 'thread-1-turn-1',
        thread_id: 'thread-1',
        turn_id: 'turn-1',
        codex_app_server_pid: '999',
        last_codex_event: 'turn/completed',
        last_codex_timestamp: '2026-03-07T00:00:01Z',
        last_codex_message: 'before',
        codex_input_tokens: 1,
        codex_output_tokens: 2,
        codex_total_tokens: 3,
        last_reported_input_tokens: 1,
        last_reported_output_tokens: 2,
        last_reported_total_tokens: 3,
        turn_count: 1,
      },
    })

    const updated = applyCodexUpdate(claimed, issue.id, {
      session: {
        last_codex_message: 'after',
        codex_total_tokens: 5,
        last_reported_total_tokens: 5,
      },
    })

    expect(updated.running.get(issue.id)).toMatchObject({
      session_id: 'thread-1-turn-1',
      codex_app_server_pid: '999',
      last_codex_event: 'turn/completed',
      last_codex_timestamp: '2026-03-07T00:00:01Z',
      last_codex_message: 'after',
      codex_input_tokens: 1,
      codex_output_tokens: 2,
      codex_total_tokens: 5,
      last_reported_input_tokens: 1,
      last_reported_output_tokens: 2,
      last_reported_total_tokens: 5,
    })
  })

  it('falls back to prior totals when a partial codex update carries null token counters', () => {
    const issue = createIssue()
    const claimed = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
      session: {
        session_id: 'thread-1-turn-1',
        thread_id: 'thread-1',
        turn_id: 'turn-1',
        codex_app_server_pid: '999',
        last_codex_event: 'turn/completed',
        last_codex_timestamp: '2026-03-07T00:00:01Z',
        last_codex_message: 'before',
        codex_input_tokens: 1,
        codex_output_tokens: 2,
        codex_total_tokens: 3,
        last_reported_input_tokens: 1,
        last_reported_output_tokens: 2,
        last_reported_total_tokens: 3,
        turn_count: 1,
      },
    })

    const updated = applyCodexUpdate(claimed, issue.id, {
      session: {
        codex_total_tokens: null as unknown as number,
        last_reported_total_tokens: null as unknown as number,
      },
    })

    expect(updated.running.get(issue.id)).toMatchObject({
      codex_total_tokens: 3,
      last_reported_total_tokens: 3,
    })
    expect(updated.codex_totals.total_tokens).toBe(0)
  })

  it('releases an issue by clearing claimed, running, and retry bookkeeping', () => {
    const issue = createIssue()
    const state = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: 2,
      started_at: '2026-03-07T00:00:00Z',
    })
    state.retry_attempts.set(issue.id, {
      issue_id: issue.id,
      identifier: issue.identifier,
      attempt: 2,
      due_at_ms: 10_000,
      timer_handle: null,
      error: 'retry me',
    })

    const released = releaseIssue(state, issue.id)

    expect(released.claimed.has(issue.id)).toBe(false)
    expect(released.running.has(issue.id)).toBe(false)
    expect(released.retry_attempts.has(issue.id)).toBe(false)
  })

  it('records completion as bookkeeping only', () => {
    const issue = createIssue()
    const state = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
    })

    const completed = recordCompletion(state, issue.id)

    expect(completed.completed.has(issue.id)).toBe(true)
    expect(completed.claimed.has(issue.id)).toBe(true)
    expect(completed.running.has(issue.id)).toBe(true)
  })

  it('turns worker exit outcomes into retry or release intents without scheduling timers', () => {
    const issue = createIssue()
    const runningState = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: 2,
      started_at: '2026-03-07T00:00:00Z',
    })

    const normalIntent = deriveWorkerExitIntent(runningState, issue.id, {
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: 2,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'succeeded',
      },
      session: null,
      outcome: {
        kind: 'normal',
        reason_code: 'stopped_max_turns_reached',
        turns_executed: 3,
        final_issue_state: 'Todo',
      },
    })
    const abnormalIntent = deriveWorkerExitIntent(runningState, issue.id, {
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: 2,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'failed',
      },
      session: null,
      outcome: {
        kind: 'abnormal',
        reason_code: 'agent_turn_error',
        turns_executed: 1,
        final_issue_state: 'Todo',
      },
    })
    const releaseIntent = deriveWorkerExitIntent(createState(), issue.id, {
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: null,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'failed',
      },
      session: null,
      outcome: {
        kind: 'abnormal',
        reason_code: 'workspace_error',
        turns_executed: 0,
        final_issue_state: null,
      },
    })

    expect(normalIntent).toEqual<WorkerExitIntent>({
      kind: 'retry',
      issue_id: issue.id,
      identifier: issue.identifier,
      attempt: 3,
      retry_kind: 'continuation',
      error: null,
    })
    expect(abnormalIntent).toEqual<WorkerExitIntent>({
      kind: 'retry',
      issue_id: issue.id,
      identifier: issue.identifier,
      attempt: 3,
      retry_kind: 'failure',
      error: 'worker exited: agent_turn_error',
    })
    expect(releaseIntent).toEqual<WorkerExitIntent>({
      kind: 'release',
      issue_id: issue.id,
      identifier: issue.identifier,
    })
  })

  it('starts failure retries at attempt one when the running entry has no prior retry attempt', () => {
    const issue = createIssue()
    const runningState = claimRunningIssue(createState(), issue, {
      workerPromise: null,
      retry_attempt: null,
      started_at: '2026-03-07T00:00:00Z',
    })

    expect(
      deriveWorkerExitIntent(runningState, issue.id, {
        attempt: {
          issue_id: issue.id,
          issue_identifier: issue.identifier,
          attempt: null,
          workspace_path: '/tmp/symphony/KAT-230',
          started_at: '2026-03-07T00:00:00Z',
          status: 'failed',
        },
        session: null,
        outcome: {
          kind: 'abnormal',
          reason_code: 'workspace_error',
          turns_executed: 0,
          final_issue_state: null,
        },
      }),
    ).toEqual<WorkerExitIntent>({
      kind: 'retry',
      issue_id: issue.id,
      identifier: issue.identifier,
      attempt: 1,
      retry_kind: 'failure',
      error: 'worker exited: workspace_error',
    })
  })
})
