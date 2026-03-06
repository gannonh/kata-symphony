import type { LiveSession } from '../../domain/models.js'
import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'
import { isRecord } from '../../config/coerce.js'

interface SessionStart {
  threadId: string
  turnId: string
  sessionId: string
}

interface UsageTotals {
  input_tokens: number
  output_tokens: number
  total_tokens: number
}

function parseUsage(params: unknown): UsageTotals {
  if (!isRecord(params)) {
    return { input_tokens: 0, output_tokens: 0, total_tokens: 0 }
  }

  const usage = params.usage
  if (!isRecord(usage)) {
    return { input_tokens: 0, output_tokens: 0, total_tokens: 0 }
  }

  return {
    input_tokens: typeof usage.input_tokens === 'number' ? usage.input_tokens : 0,
    output_tokens: typeof usage.output_tokens === 'number' ? usage.output_tokens : 0,
    total_tokens: typeof usage.total_tokens === 'number' ? usage.total_tokens : 0,
  }
}

export function createSessionReducer() {
  let latestEvent: string | null = null
  let latestMessageRaw: unknown = null
  let latestTimestamp: number | null = null
  let usage: UsageTotals = { input_tokens: 0, output_tokens: 0, total_tokens: 0 }
  let completed = false
  let completionResolve!: () => void
  let completionPromise = new Promise<void>((resolve) => {
    completionResolve = resolve
  })

  return {
    acceptMessage(message: unknown) {
      if (!isRecord(message)) {
        return
      }

      if (typeof message.method !== 'string') {
        return
      }

      latestEvent = message.method
      latestMessageRaw = message
      latestTimestamp = Date.now()

      if (
        message.method !== 'turn/completed' &&
        message.method !== 'turn/failed' &&
        message.method !== 'turn/cancelled'
      ) {
        return
      }

      usage = parseUsage(message.params)
      completed = true
      completionResolve()
    },

    resetForNextTurn() {
      completed = false
      usage = { input_tokens: 0, output_tokens: 0, total_tokens: 0 }
      latestEvent = null
      latestMessageRaw = null
      latestTimestamp = null
      completionPromise = new Promise<void>((resolve) => {
        completionResolve = resolve
      })
    },

    async waitForTurnCompletion(timeoutMs: number) {
      if (!completed) {
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(() => {
            reject(new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_TIMEOUT))
          }, timeoutMs)

          completionPromise.then(() => {
            clearTimeout(timer)
            resolve()
          })
        })
      }

      if (latestEvent === 'turn/failed') {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.TURN_FAILED)
      }
      if (latestEvent === 'turn/cancelled') {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.TURN_CANCELLED)
      }
    },

    toLiveSession(session: SessionStart, pid: number | undefined, turnCount: number): LiveSession {
      return {
        session_id: session.sessionId,
        thread_id: session.threadId,
        turn_id: session.turnId,
        codex_app_server_pid: pid ? String(pid) : null,
        last_codex_event: latestEvent,
        last_codex_timestamp: latestTimestamp ? new Date(latestTimestamp).toISOString() : null,
        last_codex_message: latestMessageRaw ? JSON.stringify(latestMessageRaw) : null,
        codex_input_tokens: usage.input_tokens,
        codex_output_tokens: usage.output_tokens,
        codex_total_tokens: usage.total_tokens,
        last_reported_input_tokens: usage.input_tokens,
        last_reported_output_tokens: usage.output_tokens,
        last_reported_total_tokens: usage.total_tokens,
        turn_count: turnCount,
      }
    },
  }
}
