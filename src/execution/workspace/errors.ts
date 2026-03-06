export const WORKSPACE_EXECUTION_ERROR_CODES = [
  'workspace_path_outside_root',
  'workspace_path_not_directory',
  'workspace_path_symlink',
  'workspace_fs_error',
  'workspace_hook_failed',
  'workspace_hook_timeout',
] as const

export type WorkspaceExecutionErrorCode =
  (typeof WORKSPACE_EXECUTION_ERROR_CODES)[number]

export interface WorkspaceExecutionErrorContext {
  issueIdentifier?: string
  workspacePath?: string
  workspaceRoot?: string
  hook?: string
  timeoutMs?: number
  [key: string]: unknown
}

export class WorkspaceExecutionError extends Error {
  readonly code: WorkspaceExecutionErrorCode
  readonly context: WorkspaceExecutionErrorContext

  constructor(
    code: WorkspaceExecutionErrorCode,
    message: string,
    context: WorkspaceExecutionErrorContext = {},
    cause?: unknown,
  ) {
    if (cause === undefined) {
      super(message)
    } else {
      super(message, { cause })
    }
    this.name = 'WorkspaceExecutionError'
    this.code = code
    this.context = { ...context }
  }
}
