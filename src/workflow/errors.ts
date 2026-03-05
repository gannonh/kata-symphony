import type { WorkflowLoaderError } from './contracts.js'

function withCode(
  code: WorkflowLoaderError['code'],
  workflowPath: string,
  message: string,
  cause?: unknown,
): WorkflowLoaderError {
  const error =
    cause === undefined
      ? (new Error(message) as WorkflowLoaderError)
      : (new Error(message, { cause }) as WorkflowLoaderError)
  error.code = code
  error.workflowPath = workflowPath
  return error
}

export function createMissingWorkflowFileError(
  workflowPath: string,
  cause?: unknown,
): WorkflowLoaderError {
  return withCode(
    'missing_workflow_file',
    workflowPath,
    `Workflow file not found: ${workflowPath}`,
    cause,
  )
}

export function createWorkflowParseError(
  workflowPath: string,
  detail: string,
): WorkflowLoaderError {
  return withCode(
    'workflow_parse_error',
    workflowPath,
    `Failed to parse workflow file: ${detail}`,
  )
}

export function createWorkflowFrontMatterNotAMapError(
  workflowPath: string,
): WorkflowLoaderError {
  return withCode(
    'workflow_front_matter_not_a_map',
    workflowPath,
    `Workflow front matter root must be a YAML map/object: ${workflowPath}`,
  )
}
