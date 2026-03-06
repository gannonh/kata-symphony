import path from 'node:path'

import { sanitizeWorkspaceKey } from '../../domain/normalization.js'

import { WorkspaceExecutionError } from './errors.js'

export interface WorkspacePathResolution {
  workspace_key: string
  path: string
}

export function assertWorkspaceInsideRoot(
  workspaceRootAbs: string,
  workspacePathAbs: string,
): void {
  const root = path.resolve(workspaceRootAbs)
  const candidate = path.resolve(workspacePathAbs)
  const relative = path.relative(root, candidate)

  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new WorkspaceExecutionError(
      'workspace_path_outside_root',
      'workspace path escaped root',
      {
        workspaceRoot: root,
        workspacePath: candidate,
      },
    )
  }
}

export function createWorkspacePathForIssue(
  workspaceRoot: string,
  issueIdentifier: string,
): WorkspacePathResolution {
  const resolvedRoot = path.resolve(workspaceRoot)
  const workspace_key = sanitizeWorkspaceKey(issueIdentifier)
  const candidatePath = path.resolve(resolvedRoot, workspace_key)

  assertWorkspaceInsideRoot(resolvedRoot, candidatePath)

  return {
    workspace_key,
    path: candidatePath,
  }
}
