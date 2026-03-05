import type { Issue } from '../../domain/models.js'

export const PROMPT_ERROR_KINDS = [
  'template_parse_error',
  'template_render_error',
] as const

export type PromptErrorKind = (typeof PROMPT_ERROR_KINDS)[number]

export interface PromptBuildInput {
  template: string
  issue: Issue
  attempt: number | null
}

export interface PromptBuildError {
  kind: PromptErrorKind
  message: string
  template_excerpt?: string
  cause?: unknown
}

export type PromptBuildResult =
  | {
      ok: true
      prompt: string
    }
  | {
      ok: false
      error: PromptBuildError
    }

export interface PromptBuilder {
  build(input: PromptBuildInput): Promise<PromptBuildResult>
}
