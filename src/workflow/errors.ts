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
