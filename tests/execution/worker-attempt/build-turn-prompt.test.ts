import { describe, expect, it } from 'vitest'

import { buildTurnPrompt } from '../../../src/execution/worker-attempt/build-turn-prompt.js'

const issue = {
  id: '1',
  identifier: 'KAT-229',
  title: 'Worker attempt pipeline',
  description: null,
  priority: 1,
  state: 'In Progress',
  branch_name: null,
  url: null,
  labels: ['area:symphony'],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('buildTurnPrompt', () => {
  it('uses the workflow template for the first turn', async () => {
    const result = await buildTurnPrompt({
      template: 'Issue {{ issue.identifier }} attempt={{ attempt }}',
      issue,
      attempt: null,
      turnNumber: 1,
      maxTurns: 3,
    })

    expect(result).toEqual({ ok: true, prompt: 'Issue KAT-229 attempt=' })
  })

  it('uses short continuation guidance for later turns without repeating the workflow body', async () => {
    const result = await buildTurnPrompt({
      template: 'DO NOT REUSE THIS BODY',
      issue,
      attempt: 1,
      turnNumber: 2,
      maxTurns: 3,
    })

    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.prompt).toContain('Continue working on KAT-229')
      expect(result.prompt).not.toContain('DO NOT REUSE THIS BODY')
    }
  })
})
