import type { WorkflowDefinition } from '../domain/models.js'

export type WorkflowLoaderErrorCode =
  | 'missing_workflow_file'
  | 'workflow_parse_error'
  | 'workflow_front_matter_not_a_map'

export interface WorkflowLoaderError extends Error {
  code: WorkflowLoaderErrorCode
  workflowPath: string
}

export interface LoadWorkflowDefinitionOptions {
  workflowPath?: string
  cwd?: string
  readFile?: (path: string) => Promise<string>
}

export type LoadWorkflowDefinition = (
  options?: LoadWorkflowDefinitionOptions,
) => Promise<WorkflowDefinition>
