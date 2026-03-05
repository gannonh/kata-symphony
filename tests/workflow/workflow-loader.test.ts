import { describe, expect, it } from 'vitest'
import { loadWorkflowDefinition } from '../../src/workflow/index.js'

describe('workflow loader path precedence', () => {
  it('uses explicit workflow path over cwd default', async () => {
    const calls: string[] = []
    const readFile = async (path: string): Promise<string> => {
      calls.push(path)
      return 'Prompt body'
    }

    await loadWorkflowDefinition({
      cwd: '/repo',
      workflowPath: './config/custom-workflow.md',
      readFile,
    })

    expect(calls[0]).toBe('/repo/config/custom-workflow.md')
  })

  it('uses cwd WORKFLOW.md when explicit path is absent', async () => {
    const calls: string[] = []
    await loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async (path: string): Promise<string> => {
        calls.push(path)
        return 'Prompt body'
      },
    })

    expect(calls[0]).toBe('/repo/WORKFLOW.md')
  })

  it('maps read failure to missing_workflow_file', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        readFile: async (): Promise<string> => {
          throw new Error('ENOENT')
        },
      }),
    ).rejects.toMatchObject({
      code: 'missing_workflow_file',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })
})
