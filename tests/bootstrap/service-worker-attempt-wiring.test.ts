import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

const harness = vi.hoisted(() => {
  const startSession = vi.fn().mockResolvedValue({
    threadId: 'thread-1',
    turnId: 'turn-1',
    sessionId: 'thread-1-turn-1',
  })
  const runTurn = vi.fn()
  const stopSession = vi.fn().mockResolvedValue(undefined)
  const getLatestSession = vi.fn(() => ({
    session_id: 'thread-1-turn-1',
    thread_id: 'thread-1',
    turn_id: 'turn-1',
    codex_app_server_pid: '321',
    last_codex_event: 'turn/completed',
    last_codex_timestamp: null,
    last_codex_message: null,
    codex_input_tokens: 1,
    codex_output_tokens: 2,
    codex_total_tokens: 3,
    last_reported_input_tokens: 1,
    last_reported_output_tokens: 2,
    last_reported_total_tokens: 3,
    turn_count: 1,
  }))
  const createAgentSessionClient = vi.fn((input: { workspacePath: string }) => {
    void input
    return {
      startSession,
      runTurn,
      stopSession,
      getLatestSession,
    }
  })

  return {
    createAgentSessionClient,
    startSession,
    runTurn,
    stopSession,
    getLatestSession,
  }
})

vi.mock('../../src/execution/agent-runner/index.js', () => ({
  createAgentSessionClient: harness.createAgentSessionClient,
}))

describe('bootstrap worker attempt runner wiring', () => {
  let originalApiKey: string | undefined

  beforeAll(() => {
    originalApiKey = process.env.LINEAR_API_KEY
    process.env.LINEAR_API_KEY = process.env.LINEAR_API_KEY ?? 'test-bootstrap-key'
  })

  afterAll(() => {
    if (originalApiKey === undefined) {
      delete process.env.LINEAR_API_KEY
      return
    }

    process.env.LINEAR_API_KEY = originalApiKey
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('creates the session client from the workspace path and logs codex events during a run', async () => {
    vi.resetModules()
    const consoleLogSpy = vi.spyOn(console, 'log').mockImplementation(() => {})

    try {
      const { createService } = await import('../../src/bootstrap/service.js')
      const service = createService()

      const result = await service.workerAttemptRunner.run(
        {
          id: 'issue-1',
          identifier: 'KAT-229',
          title: 'Worker pipeline',
          description: null,
          priority: 1,
          state: 'Review',
          branch_name: null,
          url: null,
          labels: [],
          blocked_by: [],
          created_at: null,
          updated_at: null,
        },
        null,
      )

      expect(harness.createAgentSessionClient).toHaveBeenCalledTimes(1)
      expect(harness.createAgentSessionClient).toHaveBeenCalledWith(
        expect.objectContaining({
          workspacePath: expect.stringContaining('KAT-229'),
        }),
      )
      expect(harness.startSession).toHaveBeenCalledTimes(1)
      expect(harness.runTurn).not.toHaveBeenCalled()
      expect(harness.stopSession).toHaveBeenCalledTimes(1)
      expect(consoleLogSpy).toHaveBeenCalledWith(
        'worker_attempt_codex_event',
        expect.objectContaining({
          issue_identifier: 'KAT-229',
          event: 'turn_completed',
          turn_number: 1,
        }),
      )
      expect(result.outcome).toMatchObject({
        kind: 'normal',
        reason_code: 'stopped_non_active_state',
        turns_executed: 1,
        final_issue_state: 'Review',
      })
    } finally {
      consoleLogSpy.mockRestore()
    }
  })
})
