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
})
