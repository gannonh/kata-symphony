import { describe, expect, it } from 'vitest'

describe('runtime-loadable contract modules', () => {
  it('loads tracker, execution, observability, and workflow contract modules', async () => {
    const trackerContracts = await import('../../src/tracker/contracts.js')
    const executionContracts = await import('../../src/execution/contracts.js')
    const observabilityContracts = await import(
      '../../src/observability/contracts.js'
    )
    const workflow = await import('../../src/workflow/index.js')

    expect(trackerContracts).toBeTypeOf('object')
    expect(executionContracts).toBeTypeOf('object')
    expect(observabilityContracts).toBeTypeOf('object')
    expect(workflow).toBeTypeOf('object')
  })

  it('loads domain barrel exports', async () => {
    const domain = await import('../../src/domain/index.js')

    expect(domain.DOMAIN_MODELS_SCHEMA_VERSION).toBe(1)
    expect(domain.sanitizeWorkspaceKey('KAT-221/fix')).toBe('KAT-221_fix')
  })

  it('exposes expected workflow runtime exports', async () => {
    const workflow = await import('../../src/workflow/index.js')

    expect(typeof workflow.loadWorkflowDefinition).toBe('function')
    expect(typeof workflow.createMissingWorkflowFileError).toBe('function')
    expect(typeof workflow.createWorkflowFrontMatterNotAMapError).toBe('function')
    expect(typeof workflow.createWorkflowParseError).toBe('function')
  })
})
