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
  it('creates workspace, runs exactly one turn via startSession, stops the session, and always runs after_run', async () => {
    const ensureWorkspace = vi.fn().mockResolvedValue({
      path: '/tmp/ws/KAT-229',
      workspace_key: 'KAT-229',
      created_now: false,
    })
    const runBeforeRun = vi.fn().mockResolvedValue(undefined)
    const runAfterRun = vi.fn().mockResolvedValue(undefined)
    const latestSession = {
      session_id: 'thread-1-turn-1',
      thread_id: 'thread-1',
      turn_id: 'turn-1',
      codex_app_server_pid: '123',
      last_codex_event: 'turn/completed',
      last_codex_timestamp: null,
      last_codex_message: null,
      codex_input_tokens: 11,
      codex_output_tokens: 7,
      codex_total_tokens: 18,
      last_reported_input_tokens: 11,
      last_reported_output_tokens: 7,
      last_reported_total_tokens: 18,
      turn_count: 1,
    }
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
      tracker: {
        fetchIssuesByIds: vi.fn().mockResolvedValue([{ ...issue, state: 'Review' }]),
      },
      sessionClientFactory: () => ({
        startSession,
        runTurn,
        stopSession,
        getLatestSession: () => latestSession,
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
    expect(result.session).toEqual(latestSession)
    expect(startSession).toHaveBeenCalledTimes(1)
    expect(runTurn).not.toHaveBeenCalled()
    expect(stopSession).toHaveBeenCalledTimes(1)
    expect(runAfterRun).toHaveBeenCalledTimes(1)
  })
})
