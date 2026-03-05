import path from 'node:path'
import { readFile as fsReadFile } from 'node:fs/promises'
import { parse as parseYaml } from 'yaml'

import type { LoadWorkflowDefinition } from './contracts.js'
import { createMissingWorkflowFileError, createWorkflowParseError } from './errors.js'

const defaultReadFile = (filePath: string): Promise<string> =>
  fsReadFile(filePath, 'utf8')

function splitFrontMatter(
  raw: string,
  workflowPath: string,
): { yamlText: string | null; body: string } {
  if (!raw.startsWith('---')) {
    return { yamlText: null, body: raw }
  }

  const lines = raw.split(/\r?\n/)
  if (lines[0] !== '---') {
    return { yamlText: null, body: raw }
  }

  let closingIndex = -1
  for (let i = 1; i < lines.length; i += 1) {
    if (lines[i].trim() === '---') {
      closingIndex = i
      break
    }
  }

  if (closingIndex < 0) {
    throw createWorkflowParseError(workflowPath, 'missing closing front matter delimiter')
  }

  return {
    yamlText: lines.slice(1, closingIndex).join('\n'),
    body: lines.slice(closingIndex + 1).join('\n'),
  }
}

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

  const split = splitFrontMatter(raw, resolvedPath)
  let config: Record<string, unknown> = {}

  if (split.yamlText !== null) {
    try {
      const decoded = parseYaml(split.yamlText)
      config = (decoded ?? {}) as Record<string, unknown>
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      throw createWorkflowParseError(resolvedPath, message)
    }
  }

  return {
    config,
    prompt_template: split.body.trim(),
  }
}
