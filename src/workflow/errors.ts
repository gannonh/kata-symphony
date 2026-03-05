import type { WorkflowLoaderError } from './contracts.js'

function withCode(
  code: WorkflowLoaderError['code'],
  workflowPath: string,
  message: string,
): WorkflowLoaderError {
  const error = new Error(message) as WorkflowLoaderError
  error.code = code
  error.workflowPath = workflowPath
  return error
}

export function createMissingWorkflowFileError(workflowPath: string): WorkflowLoaderError {
  return withCode(
    'missing_workflow_file',
    workflowPath,
    `Workflow file not found: ${workflowPath}`,
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
