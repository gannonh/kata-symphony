import type { WorkflowDefinition } from '../domain/models.js'

export type WorkflowLoaderErrorCode = 'missing_workflow_file'

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
