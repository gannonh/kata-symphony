import { spawn } from 'node:child_process'
import { mkdir, rm, stat } from 'node:fs/promises'

import type { Workspace } from '../../domain/models.js'
import type { WorkspaceManager } from '../contracts.js'
import { WorkspaceExecutionError } from './errors.js'
import { runWorkspaceHook, type RunWorkspaceHookCommand } from './hooks.js'
import { createWorkspacePathForIssue } from './paths.js'

interface WorkspaceHooksConfig {
  after_create: string | null
  before_run: string | null
  after_run: string | null
  before_remove: string | null
  timeout_ms: number
}

export interface CreateWorkspaceManagerInput {
  workspaceRoot: string
  hooks: WorkspaceHooksConfig
  runCommand?: RunWorkspaceHookCommand
}

const DEFAULT_RUN_COMMAND: RunWorkspaceHookCommand = async ({
  script,
  cwd,
  timeout_ms,
}) => new Promise((resolve) => {
  const child = spawn('sh', ['-lc', script], {
    cwd,
    stdio: ['ignore', 'ignore', 'pipe'],
  })

  let stderr = ''
  let timedOut = false
  let settled = false
  let forceKillTimer: NodeJS.Timeout | null = null

  const finish = (result: {
    timed_out: boolean
    exit_code: number | null
    stderr: string
  }) => {
    if (settled) {
      return
    }
    settled = true
    clearTimeout(timeoutTimer)
    if (forceKillTimer !== null) {
      clearTimeout(forceKillTimer)
    }
    resolve(result)
  }

  child.stderr?.on('data', (chunk) => {
    stderr += String(chunk)
  })

  child.on('error', (error) => {
    finish({
      timed_out: timedOut,
      exit_code: 1,
      /* c8 ignore next -- stderr is usually empty on spawn error path */
      stderr: stderr.length > 0 ? stderr.trim() : String(error.message),
    })
  })

  child.on('close', (code) => {
    finish({
      timed_out: timedOut,
      exit_code: timedOut ? null : (code ?? 1),
      stderr: stderr.trim(),
    })
  })

  const timeoutTimer = setTimeout(() => {
    timedOut = true
    child.kill('SIGTERM')
    forceKillTimer = setTimeout(() => {
      /* c8 ignore next -- SIGTERM normally terminates before SIGKILL fallback */
      child.kill('SIGKILL')
    }, 1000)
    forceKillTimer.unref()
  }, timeout_ms)
  timeoutTimer.unref()
})

export function createWorkspaceManager(input: CreateWorkspaceManagerInput): WorkspaceManager {
  const runCommand = input.runCommand ?? DEFAULT_RUN_COMMAND

  return {
    async ensureWorkspace(issueIdentifier: string): Promise<Workspace> {
      const resolved = createWorkspacePathForIssue(input.workspaceRoot, issueIdentifier)
      const created_now = await ensureDirectory(resolved.path)

      if (created_now) {
        await runWorkspaceHook({
          hook: 'after_create',
          script: input.hooks.after_create,
          timeout_ms: input.hooks.timeout_ms,
          cwd: resolved.path,
          runCommand,
        })
      }

      return {
        path: resolved.path,
        workspace_key: resolved.workspace_key,
        created_now,
      }
    },
    async runBeforeRun(workspace: Workspace): Promise<void> {
      await runWorkspaceHook({
        hook: 'before_run',
        script: input.hooks.before_run,
        timeout_ms: input.hooks.timeout_ms,
        cwd: workspace.path,
        runCommand,
      })
    },
    async runAfterRun(workspace: Workspace): Promise<void> {
      await runWorkspaceHook({
        hook: 'after_run',
        script: input.hooks.after_run,
        timeout_ms: input.hooks.timeout_ms,
        cwd: workspace.path,
        runCommand,
      })
    },
    async removeWorkspace(issueIdentifier: string): Promise<{ removed: boolean; path: string }> {
      const resolved = createWorkspacePathForIssue(input.workspaceRoot, issueIdentifier)
      const state = await getDirectoryState(resolved.path)

      if (state === 'missing') {
        return { removed: false, path: resolved.path }
      }

      if (state === 'not_directory') {
        throw new WorkspaceExecutionError(
          'workspace_path_not_directory',
          'workspace path exists and is not a directory',
          { workspacePath: resolved.path, fatal: true },
        )
      }

      try {
        await runWorkspaceHook({
          hook: 'before_remove',
          script: input.hooks.before_remove,
          timeout_ms: input.hooks.timeout_ms,
          cwd: resolved.path,
          runCommand,
        })
      } catch {
        // before_remove is best-effort by policy.
      }

      try {
        await rm(resolved.path, { recursive: true, force: true })
      } catch (error) {
        /* c8 ignore start -- OS-level rm failures are non-deterministic to trigger in tests */
        throw new WorkspaceExecutionError(
          'workspace_fs_error',
          'failed to remove workspace directory',
          { workspacePath: resolved.path, operation: 'rm', fatal: true },
          error,
        )
        /* c8 ignore stop */
      }

      return { removed: true, path: resolved.path }
    },
  }
}

async function ensureDirectory(pathAbs: string): Promise<boolean> {
  try {
    const entry = await stat(pathAbs)

    if (!entry.isDirectory()) {
      throw new WorkspaceExecutionError(
        'workspace_path_not_directory',
        'workspace path exists and is not a directory',
        { workspacePath: pathAbs, fatal: true },
      )
    }

    return false
  } catch (error) {
    if (error instanceof WorkspaceExecutionError) {
      throw error
    }

    if (isNodeError(error) && error.code !== 'ENOENT') {
      /* c8 ignore start -- non-ENOENT stat failures are environment-dependent */
      throw new WorkspaceExecutionError(
        'workspace_fs_error',
        'failed to inspect workspace directory',
        { workspacePath: pathAbs, operation: 'stat', fatal: true },
        error,
      )
      /* c8 ignore stop */
    }

    try {
      await mkdir(pathAbs, { recursive: true })
      return true
    } catch (mkdirError) {
      /* c8 ignore start -- mkdir failure branch depends on host FS permissions/races */
      throw new WorkspaceExecutionError(
        'workspace_fs_error',
        'failed to create workspace directory',
        { workspacePath: pathAbs, operation: 'mkdir', fatal: true },
        mkdirError,
      )
      /* c8 ignore stop */
    }
  }
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return typeof error === 'object' && error !== null && 'code' in error
}

async function getDirectoryState(pathAbs: string): Promise<'directory' | 'not_directory' | 'missing'> {
  try {
    const entry = await stat(pathAbs)
    return entry.isDirectory() ? 'directory' : 'not_directory'
  } catch (error) {
    if (isNodeError(error) && error.code === 'ENOENT') {
      return 'missing'
    }
    /* c8 ignore start -- non-ENOENT stat failures are environment-dependent */
    throw new WorkspaceExecutionError(
      'workspace_fs_error',
      'failed to inspect workspace directory',
      { workspacePath: pathAbs, operation: 'stat', fatal: true },
      error,
    )
    /* c8 ignore stop */
  }
}
