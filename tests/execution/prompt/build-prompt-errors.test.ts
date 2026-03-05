import { describe, expect, it } from 'vitest'

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
})
