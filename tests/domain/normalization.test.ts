import { describe, expect, it } from 'vitest'
import {
  normalizeIssueState,
  sanitizeWorkspaceKey,
  makeSessionId,
} from '../../src/domain/normalization.js'

describe('domain normalization rules', () => {
  it('sanitizes workspace key per allowlist', () => {
    expect(sanitizeWorkspaceKey('KAT-221/fix*scope')).toBe('KAT-221_fix_scope')
  })

  it('normalizes state by trim + lowercase', () => {
    expect(normalizeIssueState('  In Progress  ')).toBe('in progress')
  })

  it('builds session id as <thread_id>::<turn_id>', () => {
    expect(makeSessionId('thread-1', 'turn-3')).toBe('thread-1::turn-3')
  })

  it('guards dot-segment workspace keys used for pathing', () => {
    expect(sanitizeWorkspaceKey('.')).toBe('_')
    expect(sanitizeWorkspaceKey('..')).toBe('__')
  })

  it('guards empty workspace keys used for pathing', () => {
    expect(sanitizeWorkspaceKey('')).toBe('_')
  })
})
