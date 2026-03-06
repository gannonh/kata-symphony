import { describe, expect, it, vi } from 'vitest'

const issue = {
  id: '1',
  identifier: 'KAT-228',
  title: 'Build runner',
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

async function runWithWaitError(waitError: unknown) {
  vi.resetModules()

  const kill = vi.fn()
  vi.doMock('node:child_process', () => ({
    spawn: vi.fn(() => ({
      pid: 123,
      killed: false,
      kill,
      stdout: {},
      stderr: {},
      stdin: {},
    })),
  }))

  vi.doMock('../../../src/execution/agent-runner/transport.js', () => ({
    createStdioTransport: vi.fn(() => ({
      sendLine: vi.fn(),
      stop: vi.fn(),
    })),
  }))

  vi.doMock('../../../src/execution/agent-runner/protocol-client.js', () => ({
    createProtocolClient: vi.fn(() => ({
      startSession: vi.fn(async () => ({
        threadId: 'thread-1',
        turnId: 'turn-1',
        sessionId: 'thread-1-turn-1',
      })),
    })),
  }))

  vi.doMock('../../../src/execution/agent-runner/session-reducer.js', () => ({
    createSessionReducer: vi.fn(() => ({
      acceptMessage: vi.fn(),
      waitForTurnCompletion: vi.fn(async () => {
        throw waitError
      }),
      toLiveSession: vi.fn(() => null),
    })),
  }))

  const { createAgentRunner } = await import('../../../src/execution/agent-runner/runner.js')

  const runner = createAgentRunner({
    codex: {
      command: 'echo noop',
      turn_timeout_ms: 5000,
      read_timeout_ms: 500,
      stall_timeout_ms: 1000,
    },
    workspacePath: '/tmp',
    buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
  })

  const result = await runner.runAttempt(issue, null)
  expect(kill).toHaveBeenCalledTimes(1)
  return result
}

describe('agent runner error mapping', () => {
  it('maps generic Error instances to their message', async () => {
    const result = await runWithWaitError(new Error('boom'))
    expect(result.attempt.status).toBe('failed')
    expect(result.attempt).toMatchObject({ error: 'boom' })
  })

  it('maps non-Error throw values via String()', async () => {
    const result = await runWithWaitError(42)
    expect(result.attempt.status).toBe('failed')
    expect(result.attempt).toMatchObject({ error: '42' })
  })
})
