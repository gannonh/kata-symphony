import { describe, expect, it } from 'vitest'

import {
  PROMPT_ERROR_KINDS,
  type PromptBuildError,
  type PromptBuildResult,
} from '../../src/execution/prompt/contracts.js'

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
})
