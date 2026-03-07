import { describe, expect, it } from 'vitest'
import type { Issue } from '../../src/domain/models.js'
import type {
  OrchestratorClaimState,
  OrchestratorState,
  RunningEntry,
} from '../../src/orchestrator/contracts.js'
import { deriveClaimState } from '../../src/orchestrator/contracts.js'

const issueFixture: Issue = {
  id: 'issue-1',
  identifier: 'KAT-230',
  title: 'Implement orchestrator runtime contracts',
  description: 'Add concrete runtime types for the orchestrator layer.',
  priority: 2,
  state: 'Todo',
  branch_name: 'feature/kat-230-runtime-contracts',
  url: 'https://linear.app/kata-sh/issue/KAT-230',
  labels: ['area:orchestrator'],
  blocked_by: [],
  created_at: '2026-03-07T00:00:00Z',
  updated_at: '2026-03-07T00:00:00Z',
}

function createRunningEntry(issue: Issue): RunningEntry {
  return {
    issue,
    identifier: issue.identifier,
    workerPromise: Promise.resolve({
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
        kind: 'normal',
        reason_code: 'stopped_non_active_state',
        turns_executed: 0,
        final_issue_state: null,
      },
    }),
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

describe('orchestrator runtime contracts', () => {
  it('supports concrete running entries and derived claim state', async () => {
    const runningEntry = createRunningEntry(issueFixture)
    const state = createState({
      running: new Map([[issueFixture.id, runningEntry]]),
      claimed: new Set([issueFixture.id]),
    })

    const claimState: OrchestratorClaimState = deriveClaimState(
      issueFixture.id,
      state,
    )

    expect(runningEntry.identifier).toBe('KAT-230')
    expect(state.running.get(issueFixture.id)).toBe(runningEntry)
    expect(claimState).toBe('running')
    expect(deriveClaimState('issue-2', state)).toBe('unclaimed')
  })

  it('derives retry_queued from retry ownership', () => {
    const state = createState({
      retry_attempts: new Map([
        [
          issueFixture.id,
          {
            issue_id: issueFixture.id,
            identifier: issueFixture.identifier,
            attempt: 1,
            due_at_ms: 1_234,
            timer_handle: null,
            error: 'retry me',
          },
        ],
      ]),
    })

    expect(deriveClaimState(issueFixture.id, state)).toBe('retry_queued')
  })

  it('derives claimed from reservation-only membership', () => {
    const state = createState({
      claimed: new Set([issueFixture.id]),
    })

    expect(deriveClaimState(issueFixture.id, state)).toBe('claimed')
  })

  it('derives released from completed bookkeeping when no live ownership remains', () => {
    const state = createState({
      completed: new Set([issueFixture.id]),
    })

    expect(deriveClaimState(issueFixture.id, state)).toBe('released')
  })

  it('prefers running over retry, claimed, and completed when memberships overlap', () => {
    const runningEntry = createRunningEntry(issueFixture)
    const state = createState({
      running: new Map([[issueFixture.id, runningEntry]]),
      claimed: new Set([issueFixture.id]),
      retry_attempts: new Map([
        [
          issueFixture.id,
          {
            issue_id: issueFixture.id,
            identifier: issueFixture.identifier,
            attempt: 2,
            due_at_ms: 2_468,
            timer_handle: null,
            error: null,
          },
        ],
      ]),
      completed: new Set([issueFixture.id]),
    })

    expect(deriveClaimState(issueFixture.id, state)).toBe('running')
  })

  it('prefers retry ownership over completed bookkeeping when memberships overlap', () => {
    const state = createState({
      claimed: new Set([issueFixture.id]),
      retry_attempts: new Map([
        [
          issueFixture.id,
          {
            issue_id: issueFixture.id,
            identifier: issueFixture.identifier,
            attempt: 2,
            due_at_ms: 2_468,
            timer_handle: null,
            error: null,
          },
        ],
      ]),
      completed: new Set([issueFixture.id]),
    })

    expect(deriveClaimState(issueFixture.id, state)).toBe('retry_queued')
  })

  it('prefers claimed reservation over completed bookkeeping when memberships overlap', () => {
    const state = createState({
      claimed: new Set([issueFixture.id]),
      completed: new Set([issueFixture.id]),
    })

    expect(deriveClaimState(issueFixture.id, state)).toBe('claimed')
  })
})
