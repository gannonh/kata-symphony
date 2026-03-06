import { describe, expect, it } from 'vitest'
import { createProtocolClient } from '../../../src/execution/agent-runner/protocol-client.js'

function transportFixture(options?: {
  threadResponse?: unknown
  turnResponse?: unknown
  skipInitializeResponse?: boolean
}) {
  const writes: string[] = []
  const pending = new Map<number, (value: unknown) => void>()

  return {
    writes,
    writeLine(line: string) {
      writes.push(line)
      const msg = JSON.parse(line) as { id?: number; method?: string }
      if (msg.method === 'initialize' && msg.id && !options?.skipInitializeResponse) {
        pending.get(msg.id)?.({ result: { serverInfo: { name: 'codex' } } })
      }
      if (msg.method === 'thread/start' && msg.id) {
        pending.get(msg.id)?.(options?.threadResponse ?? { result: { thread: { id: 'thread-1' } } })
      }
      if (msg.method === 'turn/start' && msg.id) {
        pending.get(msg.id)?.(options?.turnResponse ?? { result: { turn: { id: 'turn-1' } } })
      }
    },
    onRequest(id: number, resolver: (value: unknown) => void) {
      pending.set(id, resolver)
    },
  }
}

describe('protocol client handshake', () => {
  it('sends initialize -> initialized -> thread/start -> turn/start in order', async () => {
    const transport = transportFixture()

    const client = createProtocolClient({
      readTimeoutMs: 500,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
    })

    const result = await client.startSession({
      cwd: '/tmp/ws',
      title: 'KAT-228: Runner',
      prompt: 'hello',
      approvalPolicy: 'never',
      threadSandbox: 'workspace-write',
      turnSandboxPolicy: { mode: 'workspace-write' },
    })

    expect(result.threadId).toBe('thread-1')
    expect(result.turnId).toBe('turn-1')
    expect(result.sessionId).toBe('thread-1-turn-1')

    const methods = transport.writes.map((line) => (JSON.parse(line) as { method: string }).method)
    expect(methods).toEqual(['initialize', 'initialized', 'thread/start', 'turn/start'])
  })

  it('throws response_error when thread id is missing', async () => {
    const transport = transportFixture({ threadResponse: { result: {} } })
    const client = createProtocolClient({
      readTimeoutMs: 500,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
    })

    await expect(
      client.startSession({
        cwd: '/tmp/ws',
        title: 'KAT-228: Runner',
        prompt: 'hello',
      }),
    ).rejects.toThrow('response_error')
  })

  it('throws response_error when turn id is missing', async () => {
    const transport = transportFixture({ turnResponse: { result: {} } })
    const client = createProtocolClient({
      readTimeoutMs: 500,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
    })

    await expect(
      client.startSession({
        cwd: '/tmp/ws',
        title: 'KAT-228: Runner',
        prompt: 'hello',
      }),
    ).rejects.toThrow('response_error')
  })

  it('throws response_timeout when initialize does not respond', async () => {
    const transport = transportFixture({ skipInitializeResponse: true })
    const client = createProtocolClient({
      readTimeoutMs: 20,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
    })

    await expect(
      client.startSession({
        cwd: '/tmp/ws',
        title: 'KAT-228: Runner',
        prompt: 'hello',
      }),
    ).rejects.toThrow('response_timeout')
  })
})
