import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { createWorkspaceManager } from '../../../src/execution/workspace/index.js'

describe('workspace manager ensureWorkspace', () => {
  const dirs: string[] = []

  afterEach(async () => {
    await Promise.all(dirs.map((dir) => rm(dir, { recursive: true, force: true })))
  })

  it('creates missing workspace and marks created_now true', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const workspace = await manager.ensureWorkspace('KAT-227/fix*scope')

    expect(workspace.created_now).toBe(true)
    expect(workspace.path).toContain('KAT-227_fix_scope')
  })

  it('fails when workspace target exists as non-directory', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)

    await writeFile(path.join(root, 'KAT-227'), 'not a directory')

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    await expect(manager.ensureWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_path_not_directory',
    })
  })

  it('reuses existing workspace and marks created_now false', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const first = await manager.ensureWorkspace('KAT-227')
    const second = await manager.ensureWorkspace('KAT-227')

    expect(first.created_now).toBe(true)
    expect(second.created_now).toBe(false)
    expect(second.path).toBe(first.path)
  })

  it('returns removed=false when workspace directory is already missing', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const result = await manager.removeWorkspace('KAT-227')

    expect(result.removed).toBe(false)
    expect(result.path).toContain('KAT-227')
  })

  it('throws when removeWorkspace target is not a directory', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    await writeFile(path.join(root, 'KAT-227'), 'not a directory')

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    await expect(manager.removeWorkspace('KAT-227')).rejects.toMatchObject({
      code: 'workspace_path_not_directory',
    })
  })

  it('treats after_run failures as nonfatal', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const runCommand = vi.fn().mockResolvedValue({
      timed_out: false,
      exit_code: 3,
      stderr: 'nonfatal failure',
    })

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: 'exit 3',
        before_remove: null,
        timeout_ms: 1000,
      },
      runCommand,
    })

    const workspace = await manager.ensureWorkspace('KAT-227')
    await expect(manager.runAfterRun(workspace)).resolves.toBeUndefined()
  })

  it('ignores before_remove command exceptions and still removes workspace', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    await mkdir(path.join(root, 'KAT-227'), { recursive: true })

    const runCommand = vi.fn().mockRejectedValue(new Error('command crashed'))
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: 'echo cleanup',
        timeout_ms: 1000,
      },
      runCommand,
    })

    const removed = await manager.removeWorkspace('KAT-227')
    expect(removed.removed).toBe(true)
  })

  it('runs default command for after_create successfully when no runCommand is injected', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: 'echo created >/dev/null',
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const workspace = await manager.ensureWorkspace('KAT-227')
    expect(workspace.created_now).toBe(true)
  })

  it('handles default-command stderr output for after_create', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: 'echo created 1>&2',
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const workspace = await manager.ensureWorkspace('KAT-227')
    expect(workspace.created_now).toBe(true)
  })

  it('propagates default-command fatal before_run exit failures', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: 'exit 7',
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const workspace = await manager.ensureWorkspace('KAT-227')
    await expect(manager.runBeforeRun(workspace)).rejects.toMatchObject({
      code: 'workspace_hook_failed',
    })
  })

  it('propagates default-command timeout for fatal before_run hook', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: 'sleep 1',
        after_run: null,
        before_remove: null,
        timeout_ms: 1,
      },
    })

    const workspace = await manager.ensureWorkspace('KAT-227')
    await expect(manager.runBeforeRun(workspace)).rejects.toMatchObject({
      code: 'workspace_hook_timeout',
    })
  })

  it('maps default-command spawn errors to fatal before_run failures', async () => {
    const manager = createWorkspaceManager({
      workspaceRoot: '/tmp',
      hooks: {
        after_create: null,
        before_run: 'echo hi',
        after_run: null,
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    await expect(
      manager.runBeforeRun({
        path: '/definitely/not/a/real/workspace/path',
        workspace_key: 'KAT-227',
        created_now: false,
      }),
    ).rejects.toMatchObject({ code: 'workspace_hook_failed' })
  })

  it('maps null exit code to nonfatal after_run ignored failure with default command', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)
    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: 'kill -TERM $$',
        before_remove: null,
        timeout_ms: 1000,
      },
    })

    const workspace = await manager.ensureWorkspace('KAT-227')
    await expect(manager.runAfterRun(workspace)).resolves.toBeUndefined()
  })
})
