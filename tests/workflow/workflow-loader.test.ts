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

  it('maps explicit workflow path read failure to missing_workflow_file', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        workflowPath: './config/custom-workflow.md',
        readFile: async (): Promise<string> => {
          throw new Error('ENOENT')
        },
      }),
    ).rejects.toMatchObject({
      code: 'missing_workflow_file',
      workflowPath: '/repo/config/custom-workflow.md',
    })
  })

  it('parses yaml front matter and trims prompt body', async () => {
    const result = await loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => `---
polling:
  interval_ms: 15000
---

  Hello {{ issue.identifier }}

`,
    })

    expect(result.config).toEqual({ polling: { interval_ms: 15000 } })
    expect(result.prompt_template).toBe('Hello {{ issue.identifier }}')
  })

  it('treats full file as prompt body when front matter is absent', async () => {
    const result = await loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => '\n\nRun the issue.\n\n',
    })

    expect(result.config).toEqual({})
    expect(result.prompt_template).toBe('Run the issue.')
  })

  it('parses yaml front matter with CRLF delimiters', async () => {
    const result = await loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => '---\r\npolling:\r\n  interval_ms: 5000\r\n---\r\n\r\nPrompt\r\n',
    })

    expect(result.config).toEqual({ polling: { interval_ms: 5000 } })
    expect(result.prompt_template).toBe('Prompt')
  })

  it('returns workflow_parse_error for invalid yaml front matter', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        readFile: async () => `---
polling: [oops
---
Prompt`,
      }),
    ).rejects.toMatchObject({
      code: 'workflow_parse_error',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })

  it('returns workflow_parse_error for missing closing delimiter', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        readFile: async () => `---
polling:
  interval_ms: 1000
Prompt without delimiter`,
      }),
    ).rejects.toMatchObject({
      code: 'workflow_parse_error',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })

  it('returns workflow_front_matter_not_a_map when yaml root is list', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        readFile: async () => `---
- item
---
Prompt`,
      }),
    ).rejects.toMatchObject({
      code: 'workflow_front_matter_not_a_map',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })

  it('returns workflow_front_matter_not_a_map when yaml root is scalar', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        readFile: async () => `---
hello
---
Prompt`,
      }),
    ).rejects.toMatchObject({
      code: 'workflow_front_matter_not_a_map',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })

  it('returns workflow_front_matter_not_a_map when yaml root is null', async () => {
    await expect(
      loadWorkflowDefinition({
        cwd: '/repo',
        readFile: async () => `---
# comment-only front matter
---
Prompt`,
      }),
    ).rejects.toMatchObject({
      code: 'workflow_front_matter_not_a_map',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })
})
