import { WorkspaceExecutionError } from './errors.js'

export type WorkspaceHookName =
  | 'after_create'
  | 'before_run'
  | 'after_run'
  | 'before_remove'

export type HookRunOutcome = 'ok' | 'ignored_failure'

export interface HookRunResult {
  outcome: HookRunOutcome
}

export interface WorkspaceHookCommandResult {
  timed_out: boolean
  exit_code: number | null
  stderr: string
}

export type RunWorkspaceHookCommand = (args: {
  script: string
  cwd: string
  timeout_ms: number
}) => Promise<WorkspaceHookCommandResult>

const FATAL_HOOKS = new Set<WorkspaceHookName>(['after_create', 'before_run'])

export async function runWorkspaceHook(input: {
  hook: WorkspaceHookName
  script: string | null
  timeout_ms: number
  cwd: string
  runCommand: RunWorkspaceHookCommand
}): Promise<HookRunResult> {
  if (input.script === null || input.script.trim().length === 0) {
    return { outcome: 'ok' }
  }

  const result = await input.runCommand({
    script: input.script,
    cwd: input.cwd,
    timeout_ms: input.timeout_ms,
  })

  const fatal = FATAL_HOOKS.has(input.hook)

  if (result.timed_out) {
    if (fatal) {
      throw new WorkspaceExecutionError(
        'workspace_hook_timeout',
        `${input.hook} timed out`,
        {
          hook: input.hook,
          workspacePath: input.cwd,
          timeoutMs: input.timeout_ms,
          fatal: true,
          stderr: result.stderr,
        },
      )
    }
    return { outcome: 'ignored_failure' }
  }

  if (result.exit_code !== 0) {
    if (fatal) {
      throw new WorkspaceExecutionError(
        'workspace_hook_failed',
        `${input.hook} failed`,
        {
          hook: input.hook,
          workspacePath: input.cwd,
          timeoutMs: input.timeout_ms,
          exitCode: result.exit_code,
          fatal: true,
          stderr: result.stderr,
        },
      )
    }
    return { outcome: 'ignored_failure' }
  }

  return { outcome: 'ok' }
}
