import path from 'node:path'
import { readFile as fsReadFile } from 'node:fs/promises'

import type { LoadWorkflowDefinition } from './contracts.js'
import { createMissingWorkflowFileError } from './errors.js'

const defaultReadFile = (filePath: string): Promise<string> =>
  fsReadFile(filePath, 'utf8')

export const loadWorkflowDefinition: LoadWorkflowDefinition = async (options = {}) => {
  const cwd = options.cwd ?? process.cwd()
  const selectedPath =
    options.workflowPath && options.workflowPath.trim().length > 0
      ? options.workflowPath
      : 'WORKFLOW.md'
  const resolvedPath = path.resolve(cwd, selectedPath)
  const readFile = options.readFile ?? defaultReadFile

  let raw: string
  try {
    raw = await readFile(resolvedPath)
  } catch {
    throw createMissingWorkflowFileError(resolvedPath)
  }

  return {
    config: {},
    prompt_template: raw.trim(),
  }
}
