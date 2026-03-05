/* c8 ignore file -- type-only contract surface */
import type { Issue, LiveSession, RunAttempt, Workspace } from '../domain/models.js'
export type {
  PromptBuildError,
  PromptBuildInput,
  PromptBuildResult,
  PromptBuilder,
} from './prompt/contracts.js'
export { PROMPT_ERROR_KINDS } from './prompt/contracts.js'

export interface WorkspaceManager {
  ensureWorkspace(issueIdentifier: string): Promise<Workspace>
}

export interface AgentRunner {
  runAttempt(
    issue: Issue,
    attempt: number | null,
  ): Promise<{ attempt: RunAttempt; session: LiveSession | null }>
}
