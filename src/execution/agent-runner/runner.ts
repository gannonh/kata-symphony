import { spawn } from 'node:child_process'
import type { Issue } from '../../domain/models.js'
import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'
import { createProtocolClient } from './protocol-client.js'
import { createSessionReducer } from './session-reducer.js'
import { createStdioTransport } from './transport.js'

interface BuildPromptResult {
  ok: true
  prompt: string
}

interface BuildPromptErrorResult {
  ok: false
  error: string | { message?: string }
}

interface RunnerDeps {
  codex: {
    command: string
    approval_policy?: string
    thread_sandbox?: string
    turn_sandbox_policy?: string
    turn_timeout_ms: number
    read_timeout_ms: number
    stall_timeout_ms: number
  }
  workspacePath: string
  buildPrompt: (input: {
    issue: Pick<Issue, 'identifier' | 'title'>
    attempt: number | null
  }) => Promise<BuildPromptResult | BuildPromptErrorResult>
}

function toErrorMessage(error: unknown): string {
  if (error instanceof AgentRunnerError) {
    return error.code
  }

  if (error instanceof Error && error.message.length > 0) {
    return error.message
  }

  return String(error)
}

function turnSandboxPolicy(value: string | undefined): { mode: string } | undefined {
  if (!value) {
    return undefined
  }

  return { mode: value }
}

export function createAgentRunner(deps: RunnerDeps) {
  return {
    async runAttempt(issue: Issue, attempt: number | null) {
      const startedAt = new Date().toISOString()
      const prompt = await deps.buildPrompt({
        issue: { identifier: issue.identifier, title: issue.title },
        attempt,
      })

      if (!prompt.ok) {
        const promptError =
          typeof prompt.error === 'string'
            ? prompt.error
            : prompt.error.message ?? AGENT_RUNNER_ERROR_CODES.RESPONSE_ERROR

        return {
          attempt: {
            issue_id: issue.id,
            issue_identifier: issue.identifier,
            attempt,
            workspace_path: deps.workspacePath,
            started_at: startedAt,
            status: 'failed',
            error: promptError,
          },
          session: null,
        }
      }

      const child = spawn('bash', ['-lc', deps.codex.command], {
        cwd: deps.workspacePath,
        stdio: ['pipe', 'pipe', 'pipe'],
      })

      const pending = new Map<number, (value: unknown) => void>()
      const sessionReducer = createSessionReducer()
      const transport = createStdioTransport({
        child,
        onMessage(message) {
          if (typeof message.id === 'number') {
            pending.get(message.id)?.(message)
            pending.delete(message.id)
            return
          }

          sessionReducer.acceptMessage(message)
        },
      })

      const protocolClient = createProtocolClient({
        readTimeoutMs: deps.codex.read_timeout_ms,
        sendLine: transport.sendLine,
        registerPending: (id, resolver) => {
          pending.set(id, resolver)
        },
        unregisterPending: (id) => {
          pending.delete(id)
        },
      })

      const cleanup = () => {
        pending.clear()
        transport.stop()
        child.kill()
      }

      try {
        const startSessionInput: {
          cwd: string
          title: string
          prompt: string
          approvalPolicy?: string
          threadSandbox?: string
          turnSandboxPolicy?: { mode: string }
        } = {
          cwd: deps.workspacePath,
          title: `${issue.identifier}: ${issue.title}`,
          prompt: prompt.prompt,
        }

        if (deps.codex.approval_policy) {
          startSessionInput.approvalPolicy = deps.codex.approval_policy
        }

        if (deps.codex.thread_sandbox) {
          startSessionInput.threadSandbox = deps.codex.thread_sandbox
        }

        const sandboxPolicy = turnSandboxPolicy(deps.codex.turn_sandbox_policy)
        if (sandboxPolicy) {
          startSessionInput.turnSandboxPolicy = sandboxPolicy
        }

        const sessionStart = await protocolClient.startSession(startSessionInput)

        const turnCompletionTimeoutMs = deps.codex.turn_timeout_ms
        await sessionReducer.waitForTurnCompletion(turnCompletionTimeoutMs)

        const result = {
          attempt: {
            issue_id: issue.id,
            issue_identifier: issue.identifier,
            attempt,
            workspace_path: deps.workspacePath,
            started_at: startedAt,
            status: 'succeeded',
          },
          session: sessionReducer.toLiveSession(sessionStart, child.pid),
        }
        cleanup()
        return result
      } catch (error) {
        const result = {
          attempt: {
            issue_id: issue.id,
            issue_identifier: issue.identifier,
            attempt,
            workspace_path: deps.workspacePath,
            started_at: startedAt,
            status: 'failed',
            error: toErrorMessage(error),
          },
          session: null,
        }
        cleanup()
        return result
      }
    },
  }
}
