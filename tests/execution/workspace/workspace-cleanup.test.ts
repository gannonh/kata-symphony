import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { createWorkspaceManager } from '../../../src/execution/workspace/index.js'

describe('workspace lifecycle helpers', () => {
  const dirs: string[] = []

  afterEach(async () => {
    await Promise.all(dirs.map((dir) => rm(dir, { recursive: true, force: true })))
  })

  it('before_run propagates timeout/failure as fatal', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)

    const runCommand = vi.fn().mockResolvedValue({
      timed_out: true,
      exit_code: null,
      stderr: 'timeout',
    })

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: 'echo x',
        after_run: null,
        before_remove: null,
        timeout_ms: 10,
      },
      runCommand,
    })

    const workspace = await manager.ensureWorkspace('KAT-227')

    await expect(manager.runBeforeRun(workspace)).rejects.toMatchObject({
      code: 'workspace_hook_timeout',
    })
  })

  it('removeWorkspace ignores before_remove failures and still deletes', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'kat-227-'))
    dirs.push(root)

    await mkdir(path.join(root, 'KAT-227'), { recursive: true })

    const runCommand = vi.fn().mockResolvedValue({
      timed_out: false,
      exit_code: 99,
      stderr: 'fail',
    })

    const manager = createWorkspaceManager({
      workspaceRoot: root,
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: 'exit 99',
        timeout_ms: 10,
      },
      runCommand,
    })

    const result = await manager.removeWorkspace('KAT-227')

    expect(result.removed).toBe(true)
  })
})
