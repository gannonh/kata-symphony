import { Liquid } from 'liquidjs'

import type { PromptBuilder, PromptBuildInput, PromptBuildResult } from './contracts.js'

const DEFAULT_PROMPT = 'You are working on an issue from Linear.'

export function createPromptBuilder(): PromptBuilder {
  const engine = new Liquid({
    strictVariables: true,
    strictFilters: true,
  })

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
        const prompt = await engine.parseAndRender(input.template, {
          issue: input.issue,
          attempt: input.attempt,
        })
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
