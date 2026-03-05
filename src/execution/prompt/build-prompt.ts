import type { PromptBuilder, PromptBuildInput, PromptBuildResult } from './contracts.js'

const DEFAULT_PROMPT = 'You are working on an issue from Linear.'

export function createPromptBuilder(): PromptBuilder {
  return {
    async build(input: PromptBuildInput): Promise<PromptBuildResult> {
      const trimmedTemplate = input.template.trim()
      if (trimmedTemplate.length === 0) {
        return {
          ok: true,
          prompt: DEFAULT_PROMPT,
        }
      }

      try {
        const prompt = renderTemplate(input)
        return {
          ok: true,
          prompt,
        }
      } catch (error) {
        return {
          ok: false,
          error: {
            kind: 'template_render_error',
            message: 'Failed to render prompt template.',
            cause: error,
          },
        }
      }
    },
  }
}

function renderTemplate(input: PromptBuildInput): string {
  return input.template
    .replace(/\{\{\s*issue\.identifier\s*\}\}/g, input.issue.identifier)
    .replace(/\{\{\s*attempt\s*\}\}/g, String(input.attempt))
}
