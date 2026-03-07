import type { Issue } from '../../domain/models.js'
import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'
import { createAgentSessionClient } from './session-client.js'

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

export function createAgentRunner(deps: RunnerDeps) {
  return {
    async runAttempt(issue: Issue, attempt: number | null) {
      const startedAt = new Date().toISOString()
      const sessionClient = createAgentSessionClient({
        codex: deps.codex,
        workspacePath: deps.workspacePath,
      })

      try {
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

        await sessionClient.startSession({
          title: `${issue.identifier}: ${issue.title}`,
          prompt: prompt.prompt,
        })

        const result = {
          attempt: {
            issue_id: issue.id,
            issue_identifier: issue.identifier,
            attempt,
            workspace_path: deps.workspacePath,
            started_at: startedAt,
            status: 'succeeded',
          },
          session: sessionClient.getLatestSession(),
        }
        await sessionClient.stopSession()
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
        await sessionClient.stopSession()
        return result
      }
    },
  }
}
