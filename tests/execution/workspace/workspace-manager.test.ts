import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, describe, expect, it } from 'vitest'

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
})
