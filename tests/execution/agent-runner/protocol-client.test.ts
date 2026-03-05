import { describe, expect, it } from 'vitest'
import { createProtocolClient } from '../../../src/execution/agent-runner/protocol-client.js'

function transportFixture() {
  const writes: string[] = []
  const pending = new Map<number, (value: unknown) => void>()

  return {
    writes,
    writeLine(line: string) {
      writes.push(line)
      const msg = JSON.parse(line) as { id?: number; method?: string }
      if (msg.method === 'initialize' && msg.id) pending.get(msg.id)?.({ result: { serverInfo: { name: 'codex' } } })
      if (msg.method === 'thread/start' && msg.id) pending.get(msg.id)?.({ result: { thread: { id: 'thread-1' } } })
      if (msg.method === 'turn/start' && msg.id) pending.get(msg.id)?.({ result: { turn: { id: 'turn-1' } } })
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
      now: () => Date.now(),
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
})
