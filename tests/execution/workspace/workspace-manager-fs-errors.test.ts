import { afterEach, describe, expect, it, vi } from 'vitest'

type FsPromisesModule = typeof import('node:fs/promises')

async function loadManagerWithFsMocks(mocks: Partial<FsPromisesModule>) {
  vi.resetModules()
  vi.doMock('node:fs/promises', async () => {
    const actual = await vi.importActual<FsPromisesModule>('node:fs/promises')
    return {
      ...actual,
      ...mocks,
    }
  })
  return import('../../../src/execution/workspace/manager.js')
}

describe('workspace manager filesystem error mapping', () => {
  afterEach(() => {
    vi.resetModules()
    vi.doUnmock('node:fs/promises')
  })

  it('maps rm failures to workspace_fs_error during removeWorkspace', async () => {
    const lstat = vi.fn().mockResolvedValue({ isDirectory: () => true, isSymbolicLink: () => false })
    const rm = vi.fn().mockRejectedValue(new Error('rm denied'))
    const { createWorkspaceManager } = await loadManagerWithFsMocks({ lstat, rm })

    const manager = createWorkspaceManager({
      workspaceRoot: '/tmp',
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
      runCommand: vi.fn().mockResolvedValue({
        timed_out: false,
        exit_code: 0,
        stderr: '',
      }),
    })

    await expect(manager.removeWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_fs_error',
    })
  })

  it('maps non-ENOENT lstat errors to workspace_fs_error during ensureWorkspace', async () => {
    const statError = Object.assign(new Error('permission denied'), { code: 'EACCES' })
    const lstat = vi.fn().mockRejectedValue(statError)
    const { createWorkspaceManager } = await loadManagerWithFsMocks({ lstat })

    const manager = createWorkspaceManager({
      workspaceRoot: '/tmp',
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    await expect(manager.ensureWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_fs_error',
    })
  })

  it('maps mkdir failures to workspace_fs_error during ensureWorkspace create path', async () => {
    const statError = Object.assign(new Error('missing'), { code: 'ENOENT' })
    const lstat = vi.fn().mockRejectedValue(statError)
    const mkdir = vi.fn().mockRejectedValue(new Error('mkdir denied'))
    const { createWorkspaceManager } = await loadManagerWithFsMocks({ lstat, mkdir })

    const manager = createWorkspaceManager({
      workspaceRoot: '/tmp',
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    await expect(manager.ensureWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_fs_error',
    })
  })

  it('maps non-ENOENT lstat errors to workspace_fs_error during removeWorkspace lookup', async () => {
    const statError = Object.assign(new Error('permission denied'), { code: 'EACCES' })
    const lstat = vi.fn().mockRejectedValue(statError)
    const { createWorkspaceManager } = await loadManagerWithFsMocks({ lstat })

    const manager = createWorkspaceManager({
      workspaceRoot: '/tmp',
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    await expect(manager.removeWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_fs_error',
    })
  })
})
