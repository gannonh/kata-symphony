import { Liquid } from 'liquidjs'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { Issue } from '../../../src/domain/models.js'
import { createPromptBuilder } from '../../../src/execution/prompt/build-prompt.js'

const issue = {
  id: '1',
  identifier: 'KAT-224',
  title: 'Strict prompt rendering',
  description: null,
  priority: null,
  state: 'Todo',
  branch_name: null,
  url: null,
  labels: [],
  blocked_by: [],
  created_at: null,
  updated_at: null,
} satisfies Issue

describe('createPromptBuilder error mapping', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('returns template_render_error on unknown variable', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '{{ issue.missing_field }}',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_render_error')
    }
  })

  it('returns template_render_error on unknown filter', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '{{ issue.identifier | no_such_filter }}',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_render_error')
    }
  })

  it('returns template_parse_error on invalid syntax', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '{{ issue.identifier ',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_parse_error')
    }
  })

  it('uses message regex fallback to classify parse errors', async () => {
    vi
      .spyOn(Liquid.prototype, 'parseAndRender')
      .mockRejectedValueOnce(new Error('syntax mismatch in template'))

    const builder = createPromptBuilder()
    const result = await builder.build({
      template: 'any',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_parse_error')
      expect(result.error.message).toContain('syntax mismatch')
    }
  })

  it('falls back to template_render_error for non-error throwables', async () => {
    vi
      .spyOn(Liquid.prototype, 'parseAndRender')
      .mockRejectedValueOnce('totally unexpected throwable')

    const builder = createPromptBuilder()
    const result = await builder.build({
      template: 'any',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_render_error')
      expect(result.error.message).toContain('totally unexpected throwable')
    }
  })

  it('classifies lowercase parse-like error names as template_parse_error', async () => {
    const parseError = new Error('unexpected failure while rendering')
    parseError.name = 'parseerror'

    vi.spyOn(Liquid.prototype, 'parseAndRender').mockRejectedValueOnce(parseError)

    const builder = createPromptBuilder()
    const result = await builder.build({
      template: 'any',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_parse_error')
    }
  })
})
