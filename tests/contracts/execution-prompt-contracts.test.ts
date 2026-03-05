import { describe, expect, it } from 'vitest'

import {
  PROMPT_ERROR_KINDS,
  type PromptBuildError,
  type PromptBuildResult,
} from '../../src/execution/prompt/contracts.js'
import { PROMPT_ERROR_KINDS as PROMPT_ERROR_KINDS_FROM_INDEX } from '../../src/execution/prompt/index.js'
import type {
  PromptBuildInput as PromptBuildInputFromExecution,
  PromptBuildResult as PromptBuildResultFromExecution,
  PromptBuilder as PromptBuilderFromExecution,
} from '../../src/execution/contracts.js'
import type {
  PromptBuildInput as PromptBuildInputFromPrompt,
  PromptBuilder as PromptBuilderFromPrompt,
} from '../../src/execution/prompt/index.js'

describe('execution prompt contracts', () => {
  it('accepts prompt build success results', () => {
    const result: PromptBuildResult = { ok: true, prompt: 'hi' }

    expect(result).toEqual({ ok: true, prompt: 'hi' })
  })

  it('accepts template render errors', () => {
    const error: PromptBuildError = {
      kind: 'template_render_error',
      message: 'render failed',
    }

    const result: PromptBuildResult = {
      ok: false,
      error,
    }

    expect(result).toEqual({
      ok: false,
      error: {
        kind: 'template_render_error',
        message: 'render failed',
      },
    })
  })

  it('exposes supported prompt error kinds', () => {
    expect(PROMPT_ERROR_KINDS).toEqual([
      'template_parse_error',
      'template_render_error',
    ])
  })

  it('exports prompt contracts from prompt index barrel', () => {
    expect(PROMPT_ERROR_KINDS_FROM_INDEX).toEqual(PROMPT_ERROR_KINDS)
  })

  it('supports compile-time PromptBuildInput and PromptBuilder usage via re-exports', async () => {
    const input: PromptBuildInputFromExecution = {
      template: 'Issue {{issue.identifier}}',
      issue: {
        id: 'issue-1',
        identifier: 'KAT-1',
        title: 'Title',
        description: null,
        priority: null,
        state: 'Todo',
        branch_name: null,
        url: null,
        labels: [],
        blocked_by: [],
        created_at: null,
        updated_at: null,
      },
      attempt: null,
    }

    const builder: PromptBuilderFromExecution = {
      async build(buildInput: PromptBuildInputFromPrompt): Promise<PromptBuildResultFromExecution> {
        return { ok: true, prompt: buildInput.template }
      },
    }

    const result = await builder.build(input)
    expect(result).toEqual({ ok: true, prompt: 'Issue {{issue.identifier}}' })

    const promptBuilderFromPrompt: PromptBuilderFromPrompt = builder
    expect(promptBuilderFromPrompt).toBe(builder)
  })
})
