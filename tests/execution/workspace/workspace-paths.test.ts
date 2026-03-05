import path from 'node:path'

import { describe, expect, it } from 'vitest'

import { sanitizeWorkspaceKey } from '../../../src/domain/normalization.js'
import { WorkspaceExecutionError } from '../../../src/execution/workspace/errors.js'
import {
  assertWorkspaceInsideRoot,
  createWorkspacePathForIssue,
} from '../../../src/execution/workspace/paths.js'

describe('workspace paths', () => {
  it('creates deterministic workspace path and key for an issue', () => {
    const workspaceRoot = './tmp/workspace-root/../workspace-root'
    const issueIdentifier = 'KAT-227:Task/2'

    const expectedRoot = path.resolve(workspaceRoot)
    const expectedKey = sanitizeWorkspaceKey(issueIdentifier)
    const expectedPath = path.resolve(expectedRoot, expectedKey)

    const first = createWorkspacePathForIssue(workspaceRoot, issueIdentifier)
    const second = createWorkspacePathForIssue(workspaceRoot, issueIdentifier)

    expect(first).toEqual({
      workspace_root: expectedRoot,
      workspace_key: expectedKey,
      path: expectedPath,
    })
    expect(second).toEqual(first)
  })

  it('accepts paths that stay inside the workspace root', () => {
    const workspaceRootAbs = path.resolve('/tmp/kata-workspaces')
    const workspacePathAbs = path.resolve(workspaceRootAbs, 'KAT-227')

    expect(() => {
      assertWorkspaceInsideRoot(workspaceRootAbs, workspacePathAbs)
    }).not.toThrow()

    expect(() => {
      assertWorkspaceInsideRoot(workspaceRootAbs, workspaceRootAbs)
    }).not.toThrow()
  })

  it('rejects paths outside the workspace root', () => {
    const workspaceRootAbs = path.resolve('/tmp/kata-workspaces')
    const workspacePathAbs = path.resolve(workspaceRootAbs, '..', 'outside')

    let thrown: unknown
    try {
      assertWorkspaceInsideRoot(workspaceRootAbs, workspacePathAbs)
    } catch (error) {
      thrown = error
    }

    expect(thrown).toBeInstanceOf(WorkspaceExecutionError)
    const workspaceError = thrown as WorkspaceExecutionError
    expect(workspaceError.code).toBe('workspace_path_outside_root')
    expect(workspaceError.context).toMatchObject({
      workspaceRoot: workspaceRootAbs,
      workspacePath: workspacePathAbs,
    })
  })
})
