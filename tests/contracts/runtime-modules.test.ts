import { describe, expect, it } from 'vitest'

describe('runtime-loadable contract modules', () => {
  it('loads tracker, execution, observability, and workflow contract modules', async () => {
    const trackerContracts = await import('../../src/tracker/contracts.js')
    const trackerRuntime = await import('../../src/tracker/index.js')
    const executionContracts = await import('../../src/execution/contracts.js')
    const executionWorkerAttempt = await import(
      '../../src/execution/worker-attempt/index.js'
    )
    const executionWorkspace = await import('../../src/execution/workspace/index.js')
    const observabilityContracts = await import(
      '../../src/observability/contracts.js'
    )
    const workflow = await import('../../src/workflow/index.js')

    expect(trackerContracts).toBeTypeOf('object')
    expect(Object.keys(trackerContracts)).toEqual([])
    expect(typeof trackerRuntime.createLinearTrackerClient).toBe('function')
    expect(typeof trackerRuntime.TrackerIntegrationError).toBe('function')
    expect(executionContracts).toBeTypeOf('object')
    expect(executionContracts.PROMPT_ERROR_KINDS).toBeDefined()
    expect(executionContracts.WORKER_ATTEMPT_OUTCOME_KINDS).toBeDefined()
    expect(executionContracts.WORKER_ATTEMPT_REASON_CODES).toBeDefined()
    expect(executionWorkerAttempt).toBeTypeOf('object')
    expect(executionWorkerAttempt.WORKER_ATTEMPT_OUTCOME_KINDS).toEqual([
      'normal',
      'abnormal',
    ])
    expect(executionWorkerAttempt.WORKER_ATTEMPT_REASON_CODES).toEqual([
      'stopped_non_active_state',
      'stopped_max_turns_reached',
      'workspace_error',
      'before_run_hook_error',
      'agent_session_startup_error',
      'prompt_error',
      'agent_turn_error',
      'issue_state_refresh_error',
    ])
    expect(executionWorkspace).toBeTypeOf('object')
    expect(typeof executionWorkspace.createWorkspaceManager).toBe('function')
    expect(observabilityContracts).toBeTypeOf('object')
    expect(Object.keys(observabilityContracts)).toEqual([])
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

  it('exposes orchestrator preflight gate exports', async () => {
    const preflight = await import('../../src/orchestrator/preflight/index.js')

    expect(typeof preflight.isDispatchPreflightFailure).toBe('function')
    expect(typeof preflight.validateDispatchPreflight).toBe('function')
    expect(typeof preflight.logPreflightFailure).toBe('function')
    expect(typeof preflight.runTickPreflightGate).toBe('function')
    expect(Object.keys(preflight)).toEqual(
      expect.arrayContaining([
        'isDispatchPreflightFailure',
        'validateDispatchPreflight',
        'logPreflightFailure',
        'runTickPreflightGate',
      ]),
    )
    expect(preflight.isDispatchPreflightFailure({ ok: false, errors: [] })).toBe(
      true,
    )
    expect(preflight.isDispatchPreflightFailure({ ok: true })).toBe(false)
  })

  it('loads orchestrator runtime contract barrel', async () => {
    const orchestratorRuntime = await import('../../src/orchestrator/runtime/index.js')

    expect(orchestratorRuntime).toBeTypeOf('object')
    expect(typeof orchestratorRuntime.deriveClaimState).toBe('function')
    expect(Object.keys(orchestratorRuntime)).toEqual(
      expect.arrayContaining([
        'deriveClaimState',
        'shouldDispatch',
        'sortCandidatesForDispatch',
        'applyCodexUpdate',
        'claimRunningIssue',
        'createInitialOrchestratorState',
        'deriveWorkerExitIntent',
        'recordCompletion',
        'releaseIssue',
        'runPollTick',
      ]),
    )
  })
})
