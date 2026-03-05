import { describe, expect, it } from 'vitest'
import { coerceInteger, coerceStringList, normalizeStateMap } from '../../src/config/coerce.js'
import { DEFAULTS } from '../../src/config/defaults.js'

describe('defaults and coercion', () => {
  it('applies spec defaults', () => {
    expect(DEFAULTS.polling.interval_ms).toBe(30000)
    expect(DEFAULTS.agent.max_turns).toBe(20)
  })

  it('coerces string and number forms', () => {
    expect(coerceInteger('42', 1)).toBe(42)
    expect(coerceInteger('bad', 9)).toBe(9)
    expect(coerceStringList('Todo, In Progress')).toEqual(['Todo', 'In Progress'])
    expect(normalizeStateMap({ ' In Progress ': '3', Done: 0 })).toEqual({ 'in progress': 3 })
  })
})
