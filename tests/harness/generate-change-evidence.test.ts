import { describe, expect, it } from 'vitest'
import { inferEvidence, loadContextMap, renderMarkdownEvidence } from '../../scripts/harness/generate_change_evidence.js'

describe('generate change evidence', () => {
  it('builds a deterministic stub from changed files and the context map', () => {
    const contextMap = loadContextMap(process.cwd())
    const evidence = inferEvidence(
      'build-time-harness',
      ['src/execution/workspace/manager.ts', 'WORKFLOW.md'],
      contextMap,
    )

    expect(evidence).toMatchObject({
      topic: 'build-time-harness',
      changedFiles: ['src/execution/workspace/manager.ts', 'WORKFLOW.md'],
      canonicalDocsUpdated: [],
      waivers: [],
      verification: [{ command: 'fill-in-command', result: 'pending' }],
    })

    expect(evidence.contextLoaded).toEqual([
      'ARCHITECTURE.md',
      'docs/harness/BUILDING-WITH-HARNESS.md',
      'docs/references/harness-engineering.md',
      'RELIABILITY.md',
      'SECURITY.md',
      'SPEC.md',
    ])
    expect(evidence.impactedAreas).toEqual([
      'architecture',
      'execution',
      'harness',
      'reliability',
      'security',
      'workflow',
    ])
  })

  it('renders the markdown stub sections expected by the evidence contract', () => {
    const contextMap = loadContextMap(process.cwd())
    const evidence = inferEvidence('config-change', ['src/config/build-effective-config.ts'], contextMap)
    const markdown = renderMarkdownEvidence(evidence)

    expect(markdown).toContain('# Change Evidence: config-change')
    expect(markdown).toContain('## Context Loaded')
    expect(markdown).toContain('## Canonical Docs Updated')
    expect(markdown).toContain('## Verification')
    expect(markdown).toContain('`ARCHITECTURE.md`')
  })
})
