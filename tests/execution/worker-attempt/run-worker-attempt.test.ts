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

function createSessionSnapshot(turnCount: number) {
  return {
    session_id: `thread-1-turn-${turnCount}`,
    thread_id: 'thread-1',
    turn_id: `turn-${turnCount}`,
    codex_app_server_pid: '123',
    last_codex_event: 'turn/completed',
    last_codex_timestamp: null,
    last_codex_message: null,
    codex_input_tokens: turnCount,
    codex_output_tokens: turnCount + 1,
    codex_total_tokens: turnCount + 2,
    last_reported_input_tokens: turnCount,
    last_reported_output_tokens: turnCount + 1,
    last_reported_total_tokens: turnCount + 2,
    turn_count: turnCount,
  }
}

function createBaseDeps() {
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
  const runTurn = vi.fn()
  const stopSession = vi.fn().mockResolvedValue(undefined)
  let latestSession = createSessionSnapshot(1)

  return {
    ensureWorkspace,
    runBeforeRun,
    runAfterRun,
    startSession,
    runTurn,
    stopSession,
    getLatestSession() {
      return latestSession
    },
    setLatestSession(value: ReturnType<typeof createSessionSnapshot>) {
      latestSession = value
    },
  }
}

describe('worker attempt runner', () => {
  it('creates workspace, runs exactly one turn via startSession, stops the session, and always runs after_run', async () => {
    const deps = createBaseDeps()

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn().mockResolvedValue([{ ...issue, state: 'Review' }]),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
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
      turns_executed: 1,
      final_issue_state: 'Review',
    })
    expect(result.attempt).toMatchObject({
      issue_id: issue.id,
      issue_identifier: issue.identifier,
      attempt: null,
      workspace_path: '/tmp/ws/KAT-229',
      status: 'succeeded',
    })
    expect(result.session).toEqual(createSessionSnapshot(1))
    expect(deps.startSession).toHaveBeenCalledTimes(1)
    expect(deps.runTurn).not.toHaveBeenCalled()
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('reuses the same thread for continuation turns until the issue becomes non-active', async () => {
    const deps = createBaseDeps()
    const fetchIssuesByIds = vi
      .fn()
      .mockResolvedValueOnce([{ ...issue, state: 'In Progress' }])
      .mockResolvedValueOnce([{ ...issue, state: 'Review' }])
    const onCodexEvent = vi.fn((event: unknown) => {
      const payload = event as { turn_number: number }
      deps.setLatestSession(createSessionSnapshot(payload.turn_number))
    })

    deps.runTurn.mockResolvedValueOnce({
      threadId: 'thread-1',
      turnId: 'turn-2',
      sessionId: 'thread-1-turn-2',
    })

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: { fetchIssuesByIds },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }} attempt={{ attempt }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
      onCodexEvent,
    })

    const result = await runner.run(issue, 1)

    expect(deps.startSession).toHaveBeenCalledTimes(1)
    expect(deps.runTurn).toHaveBeenCalledTimes(1)
    expect(deps.runTurn).toHaveBeenCalledWith({
      threadId: 'thread-1',
      title: 'KAT-229: Worker pipeline',
      prompt:
        'Continue working on KAT-229 (turn 2 of 3). Do not repeat prior setup; continue from the current thread state.',
    })
    expect(fetchIssuesByIds).toHaveBeenCalledTimes(2)
    expect(onCodexEvent).toHaveBeenCalledTimes(2)
    expect(result.outcome).toMatchObject({
      kind: 'normal',
      reason_code: 'stopped_non_active_state',
      turns_executed: 2,
      final_issue_state: 'Review',
    })
    expect(result.session).toEqual(createSessionSnapshot(2))
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('maps tracker refresh failure to abnormal issue_state_refresh_error and still runs after_run', async () => {
    const deps = createBaseDeps()
    const fetchIssuesByIds = vi.fn().mockRejectedValue(new Error('tracker offline'))

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: { fetchIssuesByIds },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
      onCodexEvent: vi.fn(),
    })

    const result = await runner.run(issue, null)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      error: 'tracker offline',
    })
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'issue_state_refresh_error',
      turns_executed: 1,
      final_issue_state: 'In Progress',
    })
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('stops at maxTurns with normal stopped_max_turns_reached outcome', async () => {
    const deps = createBaseDeps()
    const fetchIssuesByIds = vi
      .fn()
      .mockResolvedValueOnce([{ ...issue, state: 'In Progress' }])
      .mockResolvedValueOnce([{ ...issue, state: 'In Progress' }])
      .mockResolvedValueOnce([{ ...issue, state: 'In Progress' }])
    const onCodexEvent = vi.fn((event: unknown) => {
      const payload = event as { turn_number: number }
      deps.setLatestSession(createSessionSnapshot(payload.turn_number))
    })

    deps.runTurn
      .mockResolvedValueOnce({
        threadId: 'thread-1',
        turnId: 'turn-2',
        sessionId: 'thread-1-turn-2',
      })
      .mockResolvedValueOnce({
        threadId: 'thread-1',
        turnId: 'turn-3',
        sessionId: 'thread-1-turn-3',
      })

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: { fetchIssuesByIds },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
      onCodexEvent,
    })

    const result = await runner.run(issue, 1)

    expect(deps.startSession).toHaveBeenCalledTimes(1)
    expect(deps.runTurn).toHaveBeenCalledTimes(2)
    expect(fetchIssuesByIds).toHaveBeenCalledTimes(3)
    expect(result.outcome).toMatchObject({
      kind: 'normal',
      reason_code: 'stopped_max_turns_reached',
      turns_executed: 3,
      final_issue_state: 'In Progress',
    })
    expect(result.session).toEqual(createSessionSnapshot(3))
  })

  it('maps before_run failures and still runs after_run without starting a session', async () => {
    const deps = createBaseDeps()
    deps.runBeforeRun.mockRejectedValueOnce('before hook failed')

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn(),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
    })

    const result = await runner.run(issue, null)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      error: 'before hook failed',
      workspace_path: '/tmp/ws/KAT-229',
    })
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'before_run_hook_error',
      turns_executed: 0,
      final_issue_state: 'In Progress',
    })
    expect(result.session).toBeNull()
    expect(deps.startSession).not.toHaveBeenCalled()
    expect(deps.stopSession).not.toHaveBeenCalled()
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('maps prompt build failures before session startup', async () => {
    const deps = createBaseDeps()

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn(),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: '{{ issue.identifier',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
    })

    const result = await runner.run(issue, null)

    expect(result.attempt.status).toBe('failed')
    expect(result.attempt.error).toContain('not closed')
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'prompt_error',
      turns_executed: 0,
      final_issue_state: 'In Progress',
    })
    expect(deps.startSession).not.toHaveBeenCalled()
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('maps first-turn session startup failures', async () => {
    const deps = createBaseDeps()
    deps.startSession.mockRejectedValueOnce(new Error('codex boot failed'))

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn(),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
    })

    const result = await runner.run(issue, null)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      error: 'codex boot failed',
    })
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'agent_session_startup_error',
      turns_executed: 0,
      final_issue_state: 'In Progress',
    })
    expect(deps.runTurn).not.toHaveBeenCalled()
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('maps continuation turn failures after the first turn has completed', async () => {
    const deps = createBaseDeps()
    const fetchIssuesByIds = vi.fn().mockResolvedValueOnce([{ ...issue, state: 'In Progress' }])
    const onCodexEvent = vi.fn((event: unknown) => {
      const payload = event as { turn_number: number }
      deps.setLatestSession(createSessionSnapshot(payload.turn_number))
    })
    deps.runTurn.mockRejectedValueOnce(new Error('turn failed'))

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: { fetchIssuesByIds },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
      onCodexEvent,
    })

    const result = await runner.run(issue, 2)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      error: 'turn failed',
    })
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'agent_turn_error',
      turns_executed: 1,
      final_issue_state: 'In Progress',
    })
    expect(fetchIssuesByIds).toHaveBeenCalledTimes(1)
    expect(onCodexEvent).toHaveBeenCalledTimes(1)
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('maps workspace provisioning failures before a workspace exists', async () => {
    const deps = createBaseDeps()
    deps.ensureWorkspace.mockRejectedValueOnce(new Error('workspace unavailable'))

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn(),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
    })

    const result = await runner.run(issue, null)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      error: 'workspace unavailable',
      workspace_path: '',
    })
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'workspace_error',
      turns_executed: 0,
      final_issue_state: null,
    })
    expect(deps.runBeforeRun).not.toHaveBeenCalled()
    expect(deps.stopSession).not.toHaveBeenCalled()
    expect(deps.runAfterRun).not.toHaveBeenCalled()
  })

  it('maps unexpected outer execution failures to workspace_error after workspace setup', async () => {
    const deps = createBaseDeps()
    const sessionClientFactory = vi.fn(() => {
      throw new Error('session factory exploded')
    })

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn(),
      },
      sessionClientFactory,
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
    })

    const result = await runner.run(issue, null)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      error: 'session factory exploded',
      workspace_path: '/tmp/ws/KAT-229',
    })
    expect(result.outcome).toMatchObject({
      kind: 'abnormal',
      reason_code: 'workspace_error',
      turns_executed: 0,
      final_issue_state: null,
    })
    expect(sessionClientFactory).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('swallows stopSession and after_run cleanup failures after producing a result', async () => {
    const deps = createBaseDeps()
    deps.stopSession.mockRejectedValueOnce(new Error('stop cleanup failed'))
    deps.runAfterRun.mockRejectedValueOnce(new Error('after cleanup failed'))

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn().mockResolvedValue([{ ...issue, state: 'Review' }]),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', 'in progress'],
      maxTurns: 3,
    })

    const result = await runner.run(issue, null)

    expect(result.outcome).toMatchObject({
      kind: 'normal',
      reason_code: 'stopped_non_active_state',
      turns_executed: 1,
      final_issue_state: 'Review',
    })
    expect(result.session).toEqual(createSessionSnapshot(1))
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })

  it('returns stopped_max_turns_reached when no turns are permitted', async () => {
    const deps = createBaseDeps()

    const runner = createWorkerAttemptRunner({
      workspace: {
        ensureWorkspace: deps.ensureWorkspace,
        runBeforeRun: deps.runBeforeRun,
        runAfterRun: deps.runAfterRun,
      },
      tracker: {
        fetchIssuesByIds: vi.fn(),
      },
      sessionClientFactory: () => ({
        startSession: deps.startSession,
        runTurn: deps.runTurn,
        stopSession: deps.stopSession,
        getLatestSession: deps.getLatestSession,
      }),
      workflowTemplate: 'Issue {{ issue.identifier }}',
      activeStates: ['todo', null as unknown as string],
      maxTurns: 0,
    })

    const result = await runner.run(issue, null)

    expect(result.attempt).toMatchObject({
      status: 'failed',
      workspace_path: '/tmp/ws/KAT-229',
    })
    expect(result.attempt.error).toBeUndefined()
    expect(result.outcome).toMatchObject({
      kind: 'normal',
      reason_code: 'stopped_max_turns_reached',
      turns_executed: 0,
      final_issue_state: 'In Progress',
    })
    expect(deps.startSession).not.toHaveBeenCalled()
    expect(deps.runTurn).not.toHaveBeenCalled()
    expect(deps.stopSession).toHaveBeenCalledTimes(1)
    expect(deps.runAfterRun).toHaveBeenCalledTimes(1)
  })
})
