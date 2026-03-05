import { describe, expect, it } from 'vitest'
import { createService, startService } from '../../src/bootstrap/service.js'

describe('service bootstrap wiring', () => {
  it('creates dependency graph and starts without orchestration loop', async () => {
    const service = createService()
    const snapshot = service.config.getSnapshot()

    expect(snapshot.tracker.kind).toBe('linear')
    expect(snapshot.agent.max_retry_backoff_ms).toBeGreaterThan(0)
    await expect(startService(service)).resolves.toBeUndefined()
  })

  it('sanitizes workspace identifiers used for workspace pathing', async () => {
    const service = createService()
    const workspace = await service.workspace.ensureWorkspace('KAT-221/fix*scope')

    expect(workspace.workspace_key).toBe('KAT-221_fix_scope')
    expect(workspace.path).toContain('/tmp/symphony/KAT-221_fix_scope')
  })
})
