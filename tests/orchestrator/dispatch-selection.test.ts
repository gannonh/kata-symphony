import { describe, expect, it } from 'vitest'
import type { Issue } from '../../src/domain/models.js'
import type {
  OrchestratorState,
  RunningEntry,
} from '../../src/orchestrator/runtime/index.js'
import {
  shouldDispatch,
  sortCandidatesForDispatch,
} from '../../src/orchestrator/runtime/index.js'

function createIssue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: 'issue-1',
    identifier: 'KAT-230',
    title: 'Implement dispatch selection helpers',
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

function createState(overrides: Partial<OrchestratorState> = {}): OrchestratorState {
  return {
    poll_interval_ms: 30_000,
    max_concurrent_agents: 3,
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

const selectionOptions = {
  activeStates: ['Todo', 'In Progress'],
  terminalStates: ['Done', 'Cancelled'],
  perStateLimits: {},
}

describe('sortCandidatesForDispatch', () => {
  it('sorts by priority, oldest created_at first, and identifier tie-breaker', () => {
    const issues = [
      createIssue({
        id: 'issue-2',
        identifier: 'KAT-300',
        priority: null,
        created_at: '2026-03-07T03:00:00Z',
      }),
      createIssue({
        id: 'issue-3',
        identifier: 'KAT-200',
        priority: 1,
        created_at: '2026-03-07T02:00:00Z',
      }),
      createIssue({
        id: 'issue-4',
        identifier: 'KAT-100',
        priority: 1,
        created_at: '2026-03-07T02:00:00Z',
      }),
      createIssue({
        id: 'issue-5',
        identifier: 'KAT-150',
        priority: 2,
        created_at: '2026-03-07T01:00:00Z',
      }),
    ]

    expect(sortCandidatesForDispatch(issues).map((issue) => issue.identifier)).toEqual([
      'KAT-100',
      'KAT-200',
      'KAT-150',
      'KAT-300',
    ])
  })
})

describe('shouldDispatch', () => {
  it('returns false when required structural fields are missing', () => {
    const issue = createIssue({
      title: '',
    }) as Issue

    expect(shouldDispatch(issue, createState(), selectionOptions)).toBe(false)
  })

  it('returns false when the issue state is terminal', () => {
    const issue = createIssue({
      state: 'Done',
    })

    expect(shouldDispatch(issue, createState(), selectionOptions)).toBe(false)
  })

  it('returns false when the issue state is not active', () => {
    const issue = createIssue({
      state: 'Backlog',
    })

    expect(shouldDispatch(issue, createState(), selectionOptions)).toBe(false)
  })

  it('returns false when the issue is already claimed', () => {
    const issue = createIssue()
    const state = createState({
      claimed: new Set([issue.id]),
    })

    expect(shouldDispatch(issue, state, selectionOptions)).toBe(false)
  })

  it('returns false when the issue is already running', () => {
    const issue = createIssue()
    const state = createState({
      running: new Map([[issue.id, createRunningEntry(issue)]]),
    })

    expect(shouldDispatch(issue, state, selectionOptions)).toBe(false)
  })

  it('returns false when global slots are exhausted', () => {
    const issue = createIssue()
    const running = new Map<string, RunningEntry>([
      ['issue-2', createRunningEntry(createIssue({ id: 'issue-2', identifier: 'KAT-231' }))],
      ['issue-3', createRunningEntry(createIssue({ id: 'issue-3', identifier: 'KAT-232' }))],
      ['issue-4', createRunningEntry(createIssue({ id: 'issue-4', identifier: 'KAT-233' }))],
    ])

    expect(
      shouldDispatch(
        issue,
        createState({ running }),
        selectionOptions,
      ),
    ).toBe(false)
  })

  it('uses state.max_concurrent_agents for the per-state fallback limit', () => {
    const issue = createIssue({
      state: 'In Progress',
    })
    const running = new Map<string, RunningEntry>([
      [
        'issue-2',
        createRunningEntry(
          createIssue({
            id: 'issue-2',
            identifier: 'KAT-231',
            state: 'In Progress',
          }),
        ),
      ],
    ])

    expect(
      shouldDispatch(
        issue,
        createState({
          max_concurrent_agents: 1,
          running,
        }),
        selectionOptions,
      ),
    ).toBe(false)
  })

  it('returns false when per-state slots are exhausted using normalized running states', () => {
    const issue = createIssue({
      state: 'In Progress',
    })
    const running = new Map<string, RunningEntry>([
      [
        'issue-2',
        createRunningEntry(
          createIssue({
            id: 'issue-2',
            identifier: 'KAT-231',
            state: ' in progress ',
          }),
        ),
      ],
    ])

    expect(
      shouldDispatch(
        issue,
        createState({ running }),
        {
          ...selectionOptions,
          perStateLimits: { 'in progress': 1 },
        },
      ),
    ).toBe(false)
  })

  it('returns false for Todo when any blocker is non-terminal', () => {
    const issue = createIssue({
      blocked_by: [
        {
          id: 'issue-9',
          identifier: 'KAT-999',
          state: 'In Progress',
        },
      ],
    })

    expect(shouldDispatch(issue, createState(), selectionOptions)).toBe(false)
  })

  it('returns true for Todo when blockers are terminal', () => {
    const issue = createIssue({
      blocked_by: [
        {
          id: 'issue-9',
          identifier: 'KAT-999',
          state: 'Done',
        },
        {
          id: 'issue-10',
          identifier: 'KAT-1000',
          state: 'Cancelled',
        },
      ],
    })

    expect(shouldDispatch(issue, createState(), selectionOptions)).toBe(true)
  })
})
