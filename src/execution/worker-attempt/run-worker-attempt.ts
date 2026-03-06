import type { LiveSession, RunAttempt, Workspace, Issue } from '../../domain/models.js'
import type { TrackerClient } from '../../tracker/contracts.js'
import type { WorkspaceManager } from '../contracts.js'
import type { AgentSessionClient } from '../agent-runner/session-client.js'
import type {
  WorkerAttemptAbnormalReasonCode,
  WorkerAttemptNormalReasonCode,
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

function createNormalOutcome(
  reasonCode: WorkerAttemptNormalReasonCode,
  turnsExecuted: number,
  finalIssueState: string | null,
): WorkerAttemptOutcome {
  return {
    kind: 'normal',
    reason_code: reasonCode,
    turns_executed: turnsExecuted,
    final_issue_state: finalIssueState,
  }
}

function normalizeState(state: string | null): string {
  return state?.trim().toLowerCase() ?? ''
}

export function createWorkerAttemptRunner(
  deps: WorkerAttemptRunnerDeps,
): WorkerAttemptRunner {
  return {
    async run(issue, attempt) {
      const startedAt = new Date().toISOString()
      const title = `${issue.identifier}: ${issue.title}`
      const activeStates = new Set(
        deps.activeStates.map((state) => normalizeState(state)),
      )

      const runtimeState: {
        workspace: Workspace | null
        client: WorkerAttemptSessionClient | null
      } = {
        workspace: null,
        client: null,
      }
      let workspacePath = ''
      let session: LiveSession | null = null
      let attemptStatus: RunAttempt['status'] = 'failed'
      let attemptError: string | undefined
      let currentIssue: Issue = issue
      let outcome: WorkerAttemptOutcome = createAbnormalOutcome(
        'workspace_error',
        0,
        null,
      )

      try {
        const executeAttempt = async () => {
          runtimeState.workspace = await deps.workspace.ensureWorkspace(issue.identifier)
          workspacePath = runtimeState.workspace.path

          try {
            await deps.workspace.runBeforeRun(runtimeState.workspace)
          } catch (error) {
            attemptError = toErrorMessage(error)
            outcome = createAbnormalOutcome('before_run_hook_error', 0, issue.state)
            return
          }

          runtimeState.client = deps.sessionClientFactory(runtimeState.workspace.path)
          let threadId: string | null = null

          for (let turnNumber = 1; turnNumber <= deps.maxTurns; turnNumber += 1) {
            const prompt = await buildTurnPrompt({
              template: deps.workflowTemplate,
              issue: currentIssue,
              attempt,
              turnNumber,
              maxTurns: deps.maxTurns,
            })

            if (!prompt.ok) {
              attemptError = prompt.error.message
              outcome = createAbnormalOutcome(
                'prompt_error',
                turnNumber - 1,
                currentIssue.state,
              )
              return
            }

            let turnStart: Awaited<
              ReturnType<WorkerAttemptSessionClient['startSession']>
            >
            try {
              if (turnNumber === 1) {
                turnStart = await runtimeState.client.startSession({
                  title,
                  prompt: prompt.prompt,
                })
              } else {
                turnStart = await runtimeState.client.runTurn({
                  threadId: threadId as string,
                  title,
                  prompt: prompt.prompt,
                })
              }
            } catch (error) {
              attemptError = toErrorMessage(error)
              outcome = createAbnormalOutcome(
                turnNumber === 1
                  ? 'agent_session_startup_error'
                  : 'agent_turn_error',
                turnNumber - 1,
                currentIssue.state,
              )
              return
            }

            threadId = turnStart.threadId
            deps.onCodexEvent?.({
              issue_id: issue.id,
              issue_identifier: issue.identifier,
              event: 'turn_completed',
              turn_number: turnNumber,
              timestamp: new Date().toISOString(),
              session: runtimeState.client.getLatestSession(),
            })

            try {
              const refreshedIssues = await deps.tracker.fetchIssuesByIds([issue.id])
              currentIssue = refreshedIssues[0] ?? currentIssue
            } catch (error) {
              attemptError = toErrorMessage(error)
              outcome = createAbnormalOutcome(
                'issue_state_refresh_error',
                turnNumber,
                currentIssue.state,
              )
              return
            }

            if (!activeStates.has(normalizeState(currentIssue.state))) {
              attemptStatus = 'succeeded'
              outcome = createNormalOutcome(
                'stopped_non_active_state',
                turnNumber,
                currentIssue.state,
              )
              return
            }

            if (turnNumber >= deps.maxTurns) {
              attemptStatus = 'succeeded'
              outcome = createNormalOutcome(
                'stopped_max_turns_reached',
                turnNumber,
                currentIssue.state,
              )
              return
            }
          }
        }

        await executeAttempt()
      } catch (error) {
        attemptError = toErrorMessage(error)
        outcome = createAbnormalOutcome('workspace_error', 0, null)
      } finally {
        const currentClient = runtimeState.client
        if (currentClient) {
          session = currentClient.getLatestSession()
          await currentClient.stopSession().catch(() => {})
        }

        const currentWorkspace = runtimeState.workspace
        if (currentWorkspace) {
          await deps.workspace.runAfterRun(currentWorkspace).catch(() => {})
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
