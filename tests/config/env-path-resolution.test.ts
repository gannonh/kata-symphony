import { describe, expect, it } from 'vitest'
import { resolveEnvToken, resolvePathValue } from '../../src/config/resolve.js'

describe('env and path resolution', () => {
  it('resolves $VAR token values', () => {
    const env = { LINEAR_API_KEY: 'abc123' }
    expect(resolveEnvToken('$LINEAR_API_KEY', env)).toBe('abc123')
    expect(resolveEnvToken('$MISSING', env)).toBe('')
    expect(resolveEnvToken('literal', env)).toBe('literal')
  })

  it('expands ~ and env-backed path values only for path fields', () => {
    const env = { SYMPHONY_WORKSPACE_ROOT: '/tmp/ws', HOME: '/Users/test' }
    expect(resolvePathValue('$SYMPHONY_WORKSPACE_ROOT', env)).toBe('/tmp/ws')
    expect(resolvePathValue('~/symphony', env)).toBe('/Users/test/symphony')
    expect(resolvePathValue('relativeRoot', env)).toBe('relativeRoot')
  })

  it('handles HOME-missing fallbacks', () => {
    const env = {}
    expect(resolvePathValue('~', env)).toBe('~')
    expect(resolvePathValue('~/symphony', env)).toBe('~/symphony')
  })
})
