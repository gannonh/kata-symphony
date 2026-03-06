import { EventEmitter } from 'node:events'
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

interface HarnessOptions {
  startSession?: () => Promise<{
    threadId: string
    turnId: string
    sessionId: string
  }>
  waitForTurnCompletion?: () => Promise<void>
  buildPrompt?: () => Promise<
    { ok: true; prompt: string } | { ok: false; error: string | { message?: string } }
  >
}

async function createHarness(options: HarnessOptions = {}) {
  vi.resetModules()

  const kill = vi.fn()
  const child = new EventEmitter() as EventEmitter & {
    pid?: number
    killed?: boolean
    kill: () => void
    stdout: Record<string, unknown>
    stderr: Record<string, unknown>
    stdin: Record<string, unknown>
  }
  child.pid = 123
  child.killed = false
  child.kill = kill
  child.stdout = {}
  child.stderr = {}
  child.stdin = {}

  vi.doMock('node:child_process', () => ({
    spawn: vi.fn(() => child),
  }))

  vi.doMock('../../../src/execution/agent-runner/transport.js', () => ({
    createStdioTransport: vi.fn(() => ({
      sendLine: vi.fn(),
      stop: vi.fn(),
    })),
  }))

  const startSession =
    options.startSession ??
    (async () => ({
      threadId: 'thread-1',
      turnId: 'turn-1',
      sessionId: 'thread-1-turn-1',
    }))
  vi.doMock('../../../src/execution/agent-runner/protocol-client.js', () => ({
    createProtocolClient: vi.fn(() => ({
      startSession: vi.fn(startSession),
    })),
  }))

  const waitForTurnCompletion = options.waitForTurnCompletion ?? (async () => {})
  vi.doMock('../../../src/execution/agent-runner/session-reducer.js', () => ({
    createSessionReducer: vi.fn(() => ({
      acceptMessage: vi.fn(),
      waitForTurnCompletion: vi.fn(waitForTurnCompletion),
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
    buildPrompt:
      options.buildPrompt ??
      (async () => ({ ok: true as const, prompt: 'hello' })),
  })

  return { runner, child, kill }
}

async function runWithWaitError(waitError: unknown) {
  const harness = await createHarness({
    waitForTurnCompletion: async () => {
      throw waitError
    },
  })

  const result = await harness.runner.runAttempt(issue, null)
  expect(harness.kill).toHaveBeenCalledTimes(1)
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

  it(
    'fails fast when child exits before startup handshake resolves',
    async () => {
      const harness = await createHarness({
        startSession: () => new Promise(() => {}),
      })

      const resultPromise = harness.runner.runAttempt(issue, null)
      setTimeout(() => {
        harness.child.emit('exit', 1, null)
      }, 10)

      const result = await resultPromise
      expect(result.attempt.status).toBe('failed')
      expect(result.attempt).toMatchObject({ error: 'response_error' })
      expect(harness.kill).toHaveBeenCalledTimes(1)
    },
    1000,
  )

  it(
    'fails fast when child closes before turn completion arrives',
    async () => {
      const harness = await createHarness({
        waitForTurnCompletion: () => new Promise(() => {}),
      })

      const resultPromise = harness.runner.runAttempt(issue, null)
      setTimeout(() => {
        harness.child.emit('close', 1, null)
      }, 10)

      const result = await resultPromise
      expect(result.attempt.status).toBe('failed')
      expect(result.attempt).toMatchObject({ error: 'response_error' })
      expect(harness.kill).toHaveBeenCalledTimes(1)
    },
    1000,
  )

  it(
    'fails fast on child error and ignores subsequent failure events',
    async () => {
      const harness = await createHarness({
        startSession: () => new Promise(() => {}),
      })

      const resultPromise = harness.runner.runAttempt(issue, null)
      setTimeout(() => {
        harness.child.emit('error', new Error('spawn failed'))
        harness.child.emit('exit', 1, null)
      }, 10)

      const result = await resultPromise
      expect(result.attempt.status).toBe('failed')
      expect(result.attempt).toMatchObject({ error: 'response_error' })
      expect(harness.kill).toHaveBeenCalledTimes(1)
    },
    1000,
  )
})
