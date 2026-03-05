import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { createLineBuffer } from './line-buffer.js'

function isObjectRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export interface ProtocolMessage {
  id?: number
  method?: string
  params?: unknown
  result?: unknown
}

export interface StdioTransport {
  sendLine(line: string): void
  stop(): void
}

export function createStdioTransport(options: {
  child: ChildProcessWithoutNullStreams
  onMessage: (message: ProtocolMessage) => void
  onStderr?: (chunk: string) => void
}): StdioTransport {
  const lineBuffer = createLineBuffer()

  options.child.stdout.setEncoding('utf8')
  options.child.stderr.setEncoding('utf8')

  const handleStdout = (chunk: string) => {
    for (const line of lineBuffer.push(chunk)) {
      try {
        const parsed = JSON.parse(line) as unknown
        if (isObjectRecord(parsed)) {
          options.onMessage(parsed as ProtocolMessage)
        }
      } catch {
        // Ignore non-JSON lines from stdout; protocol framing is line-based JSON.
      }
    }
  }

  const handleStderr = (chunk: string) => {
    options.onStderr?.(chunk)
  }

  options.child.stdout.on('data', handleStdout)
  options.child.stderr.on('data', handleStderr)

  return {
    sendLine(line: string) {
      options.child.stdin.write(`${line}\n`)
    },
    stop() {
      options.child.stdout.off('data', handleStdout)
      options.child.stderr.off('data', handleStderr)
      options.child.stdin.end()
    },
  }
}
