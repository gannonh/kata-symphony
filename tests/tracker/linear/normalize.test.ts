import { describe, expect, it } from 'vitest'
import {
  TrackerIntegrationError,
  normalizeLinearIssue,
} from '../../../src/tracker/index.js'

describe('linear normalization', () => {
  it('normalizes labels, blockers, priority and timestamps', () => {
    const issue = normalizeLinearIssue({
      id: 'lin-1',
      identifier: 'KAT-226',
      title: 'Tracker work',
      description: 'desc',
      priority: 2.7,
      state: { name: 'Todo' },
      branchName: 'feature/kat-226',
      url: 'https://linear.app/kata-sh/issue/KAT-226',
      labels: {
        nodes: [{ name: 'Area:Symphony' }, { name: 'TRACK:Integration' }],
      },
      inverseRelations: {
        nodes: [
          {
            type: 'blocks',
            issue: { id: 'lin-2', identifier: 'KAT-223', state: { name: 'Done' } },
          },
          {
            type: 'related',
            issue: { id: 'lin-3', identifier: 'KAT-999', state: { name: 'Todo' } },
          },
        ],
      },
      createdAt: '2026-03-05T00:00:00.000Z',
      updatedAt: '2026-03-05',
    })

    expect(issue.labels).toEqual(['area:symphony', 'track:integration'])
    expect(issue.blocked_by).toEqual([
      { id: 'lin-2', identifier: 'KAT-223', state: 'Done' },
    ])
    expect(issue.priority).toBeNull()
    expect(issue.created_at).toBe('2026-03-05T00:00:00.000Z')
    expect(issue.updated_at).toBeNull()
  })

  it('throws a linear unknown payload error when required issue fields are missing', () => {
    try {
      normalizeLinearIssue({
        id: 'lin-1',
        identifier: '   ',
        title: 'Tracker work',
        state: { name: 'Todo' },
      })
      throw new Error('expected normalizeLinearIssue to throw')
    } catch (error) {
      expect(error).toBeInstanceOf(TrackerIntegrationError)
      expect((error as TrackerIntegrationError).code).toBe(
        'linear_unknown_payload',
      )
      expect((error as TrackerIntegrationError).message).toBe(
        'Linear payload invalid: issue node missing required fields',
      )
    }
  })
})
