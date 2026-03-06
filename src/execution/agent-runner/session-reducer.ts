import type { LiveSession } from '../../domain/models.js'
import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'

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

function isObjectRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function parseUsage(params: unknown): UsageTotals {
  if (!isObjectRecord(params)) {
    return { input_tokens: 0, output_tokens: 0, total_tokens: 0 }
  }

  const usage = params.usage
  if (!isObjectRecord(usage)) {
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
  let latestMessage: string | null = null
  let latestTimestamp: string | null = null
  let usage: UsageTotals = { input_tokens: 0, output_tokens: 0, total_tokens: 0 }
  let completed = false
  let completionResolve!: () => void

  const completionPromise = new Promise<void>((resolve) => {
    completionResolve = resolve
  })

  return {
    acceptMessage(message: unknown) {
      if (!isObjectRecord(message)) {
        return
      }

      if (typeof message.method !== 'string') {
        return
      }

      latestEvent = message.method
      latestMessage = JSON.stringify(message)
      latestTimestamp = new Date().toISOString()

      if (message.method !== 'turn/completed') {
        return
      }

      usage = parseUsage(message.params)
      completed = true
      completionResolve()
    },

    async waitForTurnCompletion(timeoutMs: number) {
      if (completed) {
        return
      }

      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => {
          clearTimeout(timer)
          reject(new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_TIMEOUT))
        }, timeoutMs)

        completionPromise.then(() => {
          clearTimeout(timer)
          resolve()
        }, reject)
      })
    },

    toLiveSession(session: SessionStart, pid: number | undefined): LiveSession {
      return {
        session_id: session.sessionId,
        thread_id: session.threadId,
        turn_id: session.turnId,
        codex_app_server_pid: pid ? String(pid) : null,
        last_codex_event: latestEvent,
        last_codex_timestamp: latestTimestamp,
        last_codex_message: latestMessage,
        codex_input_tokens: usage.input_tokens,
        codex_output_tokens: usage.output_tokens,
        codex_total_tokens: usage.total_tokens,
        last_reported_input_tokens: usage.input_tokens,
        last_reported_output_tokens: usage.output_tokens,
        last_reported_total_tokens: usage.total_tokens,
        turn_count: 1,
      }
    },
  }
}
