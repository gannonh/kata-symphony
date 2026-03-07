import { describe, expect, it } from 'vitest'
import { createProtocolClient } from '../../../src/execution/agent-runner/protocol-client.js'

function transportFixture(options?: {
  threadResponse?: unknown
  turnResponse?: unknown
  turnResponses?: unknown[]
  skipInitializeResponse?: boolean
}) {
  const writes: string[] = []
  const pending = new Map<number, (value: unknown) => void>()
  const turnResponses = [...(options?.turnResponses ?? [])]

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
        pending.get(msg.id)?.(
          turnResponses.shift() ??
            options?.turnResponse ??
            { result: { turn: { id: 'turn-1' } } },
        )
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

    await client.initializeSession()

    const thread = await client.startThread({
      cwd: '/tmp/ws',
      approvalPolicy: 'never',
      threadSandbox: 'workspace-write',
    })

    const result = await client.startTurn({
      threadId: thread.threadId,
      cwd: '/tmp/ws',
      title: 'KAT-228: Runner',
      prompt: 'hello',
      approvalPolicy: 'never',
      turnSandboxPolicy: { mode: 'workspace-write' },
    })

    expect(thread.threadId).toBe('thread-1')
    expect(result.turnId).toBe('turn-1')
    expect(result.sessionId).toBe('thread-1::turn-1')

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

    await client.initializeSession()

    await expect(client.startThread({ cwd: '/tmp/ws' })).rejects.toThrow('response_error')
  })

  it('throws response_error when turn id is missing', async () => {
    const transport = transportFixture({ turnResponse: { result: {} } })
    const client = createProtocolClient({
      readTimeoutMs: 500,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
    })

    await client.initializeSession()
    const thread = await client.startThread({ cwd: '/tmp/ws' })

    await expect(
      client.startTurn({
        cwd: '/tmp/ws',
        threadId: thread.threadId,
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

    await expect(client.initializeSession()).rejects.toThrow('response_timeout')
  })

  it('starts multiple turns on the same thread', async () => {
    const transport = transportFixture({
      turnResponses: [
        { result: { turn: { id: 'turn-1' } } },
        { result: { turn: { id: 'turn-2' } } },
      ],
    })

    const client = createProtocolClient({
      readTimeoutMs: 500,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
    })

    await client.initializeSession()
    const thread = await client.startThread({ cwd: '/tmp/ws' })
    const turn1 = await client.startTurn({
      cwd: '/tmp/ws',
      threadId: thread.threadId,
      title: 'first',
      prompt: 'one',
    })
    const turn2 = await client.startTurn({
      cwd: '/tmp/ws',
      threadId: thread.threadId,
      title: 'second',
      prompt: 'two',
    })

    expect(turn1.threadId).toBe(thread.threadId)
    expect(turn2.threadId).toBe(thread.threadId)
    expect(turn1.turnId).toBe('turn-1')
    expect(turn2.turnId).toBe('turn-2')
  })
})
