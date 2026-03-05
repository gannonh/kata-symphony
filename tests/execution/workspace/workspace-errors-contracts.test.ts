import { describe, expect, it } from 'vitest'

import type { Workspace } from '../../../src/domain/models.js'
import {
  WORKSPACE_EXECUTION_ERROR_CODES,
  WorkspaceExecutionError,
  type WorkspaceManager,
} from '../../../src/execution/contracts.js'

describe('workspace execution contracts', () => {
  it('exposes supported workspace execution error codes', () => {
    expect(WORKSPACE_EXECUTION_ERROR_CODES).toEqual([
      'workspace_path_outside_root',
      'workspace_path_not_directory',
      'workspace_fs_error',
      'workspace_hook_failed',
      'workspace_hook_timeout',
    ])
  })

  it('keeps code and context on WorkspaceExecutionError', () => {
    const error = new WorkspaceExecutionError(
      'workspace_path_outside_root',
      'workspace path escaped root',
      {
        issueIdentifier: 'KAT-227',
        workspacePath: '/tmp/outside',
        workspaceRoot: '/tmp/root',
      },
    )

    expect(error).toBeInstanceOf(Error)
    expect(error.name).toBe('WorkspaceExecutionError')
    expect(error.code).toBe('workspace_path_outside_root')
    expect(error.context).toEqual({
      issueIdentifier: 'KAT-227',
      workspacePath: '/tmp/outside',
      workspaceRoot: '/tmp/root',
    })
    expect(error.message).toContain('workspace path escaped root')
  })

  it('propagates constructor cause on WorkspaceExecutionError', () => {
    const cause = new Error('hook timed out')
    const error = new WorkspaceExecutionError(
      'workspace_hook_timeout',
      'workspace hook timed out',
      {
        issueIdentifier: 'KAT-227',
        hook: 'before_run',
        timeoutMs: 5000,
      },
      cause,
    )

    expect(error.cause).toBe(cause)
    expect(error.code).toBe('workspace_hook_timeout')
  })

  it('supports additive WorkspaceManager lifecycle methods', async () => {
    const workspace: Workspace = {
      path: '/tmp/KAT-227',
      workspace_key: 'KAT-227',
      created_now: false,
    }

    const manager: WorkspaceManager = {
      async ensureWorkspace(issueIdentifier: string) {
        return {
          ...workspace,
          workspace_key: issueIdentifier,
        }
      },
      async runBeforeRun() {},
      async runAfterRun() {},
      async removeWorkspace(issueIdentifier: string) {
        return {
          removed: issueIdentifier.length > 0,
          path: `/tmp/${issueIdentifier}`,
        }
      },
    }

    const ensured = await manager.ensureWorkspace('KAT-227')
    await manager.runBeforeRun(ensured)
    await manager.runAfterRun(ensured)
    const removed = await manager.removeWorkspace('KAT-227')

    expect(ensured.workspace_key).toBe('KAT-227')
    expect(removed).toEqual({
      removed: true,
      path: '/tmp/KAT-227',
    })
  })
})
