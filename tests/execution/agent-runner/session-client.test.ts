import { PassThrough } from 'node:stream'
import { EventEmitter } from 'node:events'
import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { describe, expect, it, vi } from 'vitest'

import { createAgentSessionClient } from '../../../src/execution/agent-runner/session-client.js'

type FakeChild = EventEmitter & {
  pid?: number
  stdout: PassThrough
  stderr: PassThrough
  stdin: PassThrough
  kill: ReturnType<typeof vi.fn>
}

interface FakeChildHarness {
  child: FakeChild
  emitDeferredCompletion(turnNumber: number): void
}

function createFakeChild(turnPlan: Array<'complete' | 'defer'> = ['complete', 'complete']): FakeChildHarness {
  const child = new EventEmitter() as FakeChild
  child.pid = 321
  child.stdout = new PassThrough()
  child.stderr = new PassThrough()
  child.stdin = new PassThrough()
  child.kill = vi.fn(() => true)
  const deferredCompletions = new Map<number, () => void>()

  let turnCount = 0
  let buffer = ''

  child.stdin.on('data', (chunk) => {
    buffer += chunk.toString()
    const lines = buffer.split('\n')
    buffer = lines.pop() ?? ''

    for (const line of lines) {
      if (!line.trim()) {
        continue
      }

      const message = JSON.parse(line) as {
        id?: number
        method?: string
        params?: { threadId?: string }
      }

      if (message.method === 'initialize' && message.id) {
        child.stdout.write(
          `${JSON.stringify({ id: message.id, result: { serverInfo: { name: 'fake' } } })}\n`,
        )
        continue
      }

      if (message.method === 'thread/start' && message.id) {
        child.stdout.write(
          `${JSON.stringify({ id: message.id, result: { thread: { id: 'thread-1' } } })}\n`,
        )
        continue
      }

      if (message.method === 'turn/start' && message.id) {
        turnCount += 1
        child.stdout.write(
          `${JSON.stringify({ id: message.id, result: { turn: { id: `turn-${turnCount}` } } })}\n`,
        )

        const emitCompletion = () => {
          child.stdout.write(
            `${JSON.stringify({
              method: 'turn/completed',
              params: {
                usage: {
                  input_tokens: turnCount,
                  output_tokens: turnCount + 1,
                  total_tokens: turnCount + 2,
                },
              },
            })}\n`,
          )
        }

        if (turnPlan[turnCount - 1] === 'defer') {
          deferredCompletions.set(turnCount, emitCompletion)
          continue
        }

        emitCompletion()
      }
    }
  })

  return {
    child,
    emitDeferredCompletion(turnNumber: number) {
      deferredCompletions.get(turnNumber)?.()
      deferredCompletions.delete(turnNumber)
    },
  }
}

describe('agent session client', () => {
  it('starts a session, runs a continuation turn on the same thread, and clears state on stop', async () => {
    const fakeChild = createFakeChild()

    const client = createAgentSessionClient({
      codex: {
        command: 'echo noop',
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp/ws',
      spawnChild: vi.fn(() => fakeChild.child as unknown as ChildProcessWithoutNullStreams),
    })

    const firstTurn = await client.startSession({
      title: 'KAT-229: first',
      prompt: 'first prompt',
    })
    expect(firstTurn).toEqual({
      threadId: 'thread-1',
      turnId: 'turn-1',
      sessionId: 'thread-1-turn-1',
    })

    expect(client.getLatestSession()).toMatchObject({
      thread_id: 'thread-1',
      turn_id: 'turn-1',
      codex_total_tokens: 3,
      turn_count: 1,
    })

    const secondTurn = await client.runTurn({
      threadId: firstTurn.threadId,
      title: 'KAT-229: second',
      prompt: 'second prompt',
    })

    expect(secondTurn).toEqual({
      threadId: 'thread-1',
      turnId: 'turn-2',
      sessionId: 'thread-1-turn-2',
    })
    expect(client.getLatestSession()).toMatchObject({
      thread_id: 'thread-1',
      turn_id: 'turn-2',
      codex_total_tokens: 4,
      turn_count: 2,
    })

    await client.stopSession()

    expect(fakeChild.child.kill).toHaveBeenCalledTimes(1)
    expect(client.getLatestSession()).toBeNull()
  })

  it('invalidates the runtime after a timed out turn so late completions cannot be reused', async () => {
    const fakeChild = createFakeChild(['complete', 'defer', 'complete'])

    const client = createAgentSessionClient({
      codex: {
        command: 'echo noop',
        turn_timeout_ms: 10,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp/ws',
      spawnChild: vi.fn(() => fakeChild.child as unknown as ChildProcessWithoutNullStreams),
    })

    const firstTurn = await client.startSession({
      title: 'KAT-229: first',
      prompt: 'first prompt',
    })

    await expect(
      client.runTurn({
        threadId: firstTurn.threadId,
        title: 'KAT-229: second',
        prompt: 'second prompt',
      }),
    ).rejects.toThrow('response_timeout')

    fakeChild.emitDeferredCompletion(2)

    await expect(
      client.runTurn({
        threadId: firstTurn.threadId,
        title: 'KAT-229: third',
        prompt: 'third prompt',
      }),
    ).rejects.toThrow('session_not_started')

    expect(fakeChild.child.kill).toHaveBeenCalledTimes(1)
    expect(client.getLatestSession()).toBeNull()
  })
})
