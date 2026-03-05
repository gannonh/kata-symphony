import { describe, expect, it } from 'vitest'

describe('runtime-loadable contract modules', () => {
  it('loads tracker, execution, and observability contract modules', async () => {
    const trackerContracts = await import('../../src/tracker/contracts.js')
    const executionContracts = await import('../../src/execution/contracts.js')
    const observabilityContracts = await import(
      '../../src/observability/contracts.js'
    )

    expect(trackerContracts).toBeTypeOf('object')
    expect(executionContracts).toBeTypeOf('object')
    expect(observabilityContracts).toBeTypeOf('object')
  })

  it('loads domain barrel exports', async () => {
    const domain = await import('../../src/domain/index.js')

    expect(domain.DOMAIN_MODELS_SCHEMA_VERSION).toBe(1)
    expect(domain.sanitizeWorkspaceKey('KAT-221/fix')).toBe('KAT-221_fix')
  })
})
