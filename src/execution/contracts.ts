/* c8 ignore file -- contract surface with minimal runtime re-exports */
import type { Issue, LiveSession, RunAttempt, Workspace } from '../domain/models.js'
export type {
  PromptBuildError,
  PromptBuildInput,
  PromptBuildResult,
  PromptBuilder,
} from './prompt/contracts.js'
export { PROMPT_ERROR_KINDS } from './prompt/contracts.js'
export type {
  WorkspaceExecutionErrorCode,
  WorkspaceExecutionErrorContext,
} from './workspace/errors.js'
export {
  WORKSPACE_EXECUTION_ERROR_CODES,
  WorkspaceExecutionError,
} from './workspace/errors.js'

export interface WorkspaceManager {
  ensureWorkspace(issueIdentifier: string): Promise<Workspace>
  runBeforeRun(workspace: Workspace): Promise<void>
  runAfterRun(workspace: Workspace): Promise<void>
  removeWorkspace(issueIdentifier: string): Promise<{ removed: boolean; path: string }>
}

export interface AgentRunner {
  runAttempt(
    issue: Issue,
    attempt: number | null,
  ): Promise<{ attempt: RunAttempt; session: LiveSession | null }>
}
