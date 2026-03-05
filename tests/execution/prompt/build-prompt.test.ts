import { describe, expect, it } from 'vitest'

import { createPromptBuilder } from '../../../src/execution/prompt/index.js'

const issue = {
  id: 'issue-1',
  identifier: 'KAT-224',
  title: 'Implement prompt builder',
  description: null,
  priority: null,
  state: 'Todo',
  branch_name: null,
  url: null,
  labels: [],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('createPromptBuilder', () => {
  it('renders issue + attempt context into the template', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: 'Issue {{ issue.identifier }} attempt={{ attempt }}',
      issue,
      attempt: 1,
    })

    expect(result).toEqual({
      ok: true,
      prompt: 'Issue KAT-224 attempt=1',
    })
  })

  it('returns the default prompt for whitespace-only templates', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '   \n\t  ',
      issue,
      attempt: 1,
    })

    expect(result).toEqual({
      ok: true,
      prompt: 'You are working on an issue from Linear.',
    })
  })

  it('preserves nested arrays/maps for Liquid iteration', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template:
        '{% for label in issue.labels %}[{{ label }}]{% endfor %}|{% for b in issue.blocked_by %}{{ b.identifier }}{% endfor %}|{{ attempt }}',
      issue: {
        ...issue,
        labels: ['area:symphony', 'type:feature'],
        blocked_by: [{ id: '2', identifier: 'KAT-221', state: 'Done' }],
      },
      attempt: 2,
    })

    expect(result).toEqual({
      ok: true,
      prompt: '[area:symphony][type:feature]|KAT-221|2',
    })
  })
})
