import { EventEmitter } from 'node:events'
import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { describe, expect, it } from 'vitest'
import { createStdioTransport } from '../../../src/execution/agent-runner/transport.js'

class MockReadable extends EventEmitter {
  setEncoding(encoding: BufferEncoding) {
    void encoding
  }
}

class MockWritable extends EventEmitter {
  writes: string[] = []
  ended = false

  write(chunk: string): boolean {
    this.writes.push(chunk)
    return true
  }

  end() {
    this.ended = true
  }
}

function childFixture() {
  const stdout = new MockReadable()
  const stderr = new MockReadable()
  const stdin = new MockWritable()

  const child = {
    stdout,
    stderr,
    stdin,
  } as unknown as ChildProcessWithoutNullStreams

  return { child, stdin, stdout, stderr }
}

describe('stdio transport', () => {
  it('parses object JSON messages from stdout and forwards stderr diagnostics', () => {
    const { child, stdin, stdout, stderr } = childFixture()
    const messages: unknown[] = []
    const stderrChunks: string[] = []

    const transport = createStdioTransport({
      child,
      onMessage(message) {
        messages.push(message)
      },
      onStderr(chunk) {
        stderrChunks.push(chunk)
      },
    })

    stdout.emit('data', '{"id":1')
    stdout.emit('data', ',"method":"initialize"}\n')
    stdout.emit('data', 'not-json\n')
    stdout.emit('data', '1\n')
    stderr.emit('data', 'diagnostic line\n')

    transport.sendLine('{"method":"initialized"}')

    expect(messages).toEqual([{ id: 1, method: 'initialize' }])
    expect(stderrChunks).toEqual(['diagnostic line\n'])
    expect(stdin.writes).toEqual(['{"method":"initialized"}\n'])
  })

  it('removes listeners and closes stdin on stop', () => {
    const { child, stdin, stdout } = childFixture()
    const messages: unknown[] = []

    const transport = createStdioTransport({
      child,
      onMessage(message) {
        messages.push(message)
      },
    })

    transport.stop()
    stdout.emit('data', '{"id":2}\n')

    expect(messages).toEqual([])
    expect(stdin.ended).toBe(true)
  })
})
