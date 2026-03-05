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
        const context = {
          issue: input.issue,
          attempt: input.attempt,
        }
        const prompt = await engine.parseAndRender(input.template, context)
        return {
          ok: true,
          prompt,
        }
      } catch (error) {
        return {
          ok: false,
          error: {
            kind: classifyPromptError(error),
            message: error instanceof Error ? error.message : String(error),
            template_excerpt: input.template.slice(0, 200),
            cause: error,
          },
        }
      }
    },
  }
}

function classifyPromptError(error: unknown): 'template_parse_error' | 'template_render_error' {
  const isErr = error instanceof Error
  const name = isErr ? error.name : ''
  const message = isErr ? error.message : String(error)
  const lowerMessage = message.toLowerCase()

  if (lowerMessage.includes('undefined variable') || lowerMessage.includes('undefined filter')) {
    return 'template_render_error'
  }

  if (
    /parse|tokenization/i.test(name) ||
    /parse|token|syntax|not closed/i.test(message)
  ) {
    return 'template_parse_error'
  }

  return 'template_render_error'
}
