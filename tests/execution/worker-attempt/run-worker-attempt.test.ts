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
      tracker: {
        fetchIssuesByIds: vi.fn().mockResolvedValue([{ ...issue, state: 'Review' }]),
      },
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
