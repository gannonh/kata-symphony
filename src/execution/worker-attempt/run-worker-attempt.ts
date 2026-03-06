import type { LiveSession, RunAttempt, Workspace, Issue } from '../../domain/models.js'
import type { TrackerClient } from '../../tracker/contracts.js'
import type { WorkspaceManager } from '../contracts.js'
import type { AgentSessionClient } from '../agent-runner/session-client.js'
import type {
  WorkerAttemptAbnormalReasonCode,
  WorkerAttemptOutcome,
  WorkerAttemptResult,
  WorkerAttemptRunner,
} from './contracts.js'
import { buildTurnPrompt } from './build-turn-prompt.js'

type WorkerAttemptWorkspaceDeps = Pick<
  WorkspaceManager,
  'ensureWorkspace' | 'runBeforeRun' | 'runAfterRun'
>

type WorkerAttemptTrackerDeps = Pick<TrackerClient, 'fetchIssuesByIds'>

type WorkerAttemptSessionClient = Pick<
  AgentSessionClient,
  'startSession' | 'runTurn' | 'stopSession' | 'getLatestSession'
>

export interface WorkerAttemptRunnerDeps {
  workspace: WorkerAttemptWorkspaceDeps
  tracker: WorkerAttemptTrackerDeps
  sessionClientFactory: (workspacePath: string) => WorkerAttemptSessionClient
  workflowTemplate: string
  activeStates: string[]
  maxTurns: number
  onCodexEvent?: (event: unknown) => void
}

function toErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message
  }

  return String(error)
}

function createAbnormalOutcome(
  reasonCode: WorkerAttemptAbnormalReasonCode,
  turnsExecuted: number,
  finalIssueState: string | null,
): WorkerAttemptOutcome {
  return {
    kind: 'abnormal',
    reason_code: reasonCode,
    turns_executed: turnsExecuted,
    final_issue_state: finalIssueState,
  }
}

export function createWorkerAttemptRunner(
  deps: WorkerAttemptRunnerDeps,
): WorkerAttemptRunner {
  return {
    async run(issue, attempt) {
      const startedAt = new Date().toISOString()
      const title = `${issue.identifier}: ${issue.title}`

      let workspace: Workspace | null = null
      let workspacePath = ''
      let client: WorkerAttemptSessionClient | null = null
      let session: LiveSession | null = null
      let attemptStatus: RunAttempt['status'] = 'failed'
      let attemptError: string | undefined
      let outcome: WorkerAttemptOutcome = createAbnormalOutcome(
        'workspace_error',
        0,
        null,
      )

      try {
        const executeAttempt = async () => {
          workspace = await deps.workspace.ensureWorkspace(issue.identifier)
          workspacePath = workspace.path

          try {
            await deps.workspace.runBeforeRun(workspace)
          } catch (error) {
            attemptError = toErrorMessage(error)
            outcome = createAbnormalOutcome('before_run_hook_error', 0, issue.state)
            return
          }

          client = deps.sessionClientFactory(workspace.path)

          const prompt = await buildTurnPrompt({
            template: deps.workflowTemplate,
            issue,
            attempt,
            turnNumber: 1,
            maxTurns: deps.maxTurns,
          })

          if (!prompt.ok) {
            attemptError = prompt.error.message
            outcome = createAbnormalOutcome('prompt_error', 0, issue.state)
            return
          }

          let startedSession: Awaited<
            ReturnType<WorkerAttemptSessionClient['startSession']>
          >
          try {
            startedSession = await client.startSession({
              title,
              prompt: prompt.prompt,
            })
          } catch (error) {
            attemptError = toErrorMessage(error)
            outcome = createAbnormalOutcome('agent_session_startup_error', 0, issue.state)
            return
          }

          try {
            await client.runTurn({
              threadId: startedSession.threadId,
              title,
              prompt: prompt.prompt,
            })
          } catch (error) {
            attemptError = toErrorMessage(error)
            outcome = createAbnormalOutcome('agent_turn_error', 0, issue.state)
            return
          }

          try {
            const refreshedIssues = await deps.tracker.fetchIssuesByIds([issue.id])
            const finalIssue = refreshedIssues[0] ?? issue
            const finalIssueState = finalIssue.state ?? issue.state

            attemptStatus = 'succeeded'
            outcome = {
              kind: 'normal',
              reason_code: 'stopped_non_active_state',
              turns_executed: 1,
              final_issue_state: finalIssueState,
            }
          } catch (error) {
            attemptError = toErrorMessage(error)
            outcome = createAbnormalOutcome('issue_state_refresh_error', 1, issue.state)
          }
        }

        await executeAttempt()
      } catch (error) {
        attemptError = toErrorMessage(error)
        outcome = createAbnormalOutcome('workspace_error', 0, null)
      } finally {
        if (client) {
          session = client.getLatestSession()
          await client.stopSession().catch(() => {})
        }

        if (workspace) {
          await deps.workspace.runAfterRun(workspace).catch(() => {})
        }
      }

      const result: WorkerAttemptResult = {
        attempt: {
          issue_id: issue.id,
          issue_identifier: issue.identifier,
          attempt,
          workspace_path: workspacePath,
          started_at: startedAt,
          status: attemptStatus,
          ...(attemptError ? { error: attemptError } : {}),
        },
        session,
        outcome,
      }

      return result
    },
  }
}
