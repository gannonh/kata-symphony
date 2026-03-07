import { describe, expect, it } from 'vitest'
import { inferEvidence, renderMarkdownEvidence } from '../../scripts/harness/generate_change_evidence.js'

const fixtureContextMap = {
  rules: [
    {
      pattern: 'src/execution/**',
      owned_by: ['SPEC.md', 'ARCHITECTURE.md', 'SECURITY.md', 'RELIABILITY.md'],
      impacted_areas: ['architecture', 'execution', 'security', 'reliability'],
    },
    {
      pattern: 'WORKFLOW.md',
      owned_by: ['SECURITY.md', 'docs/references/harness-engineering.md', 'docs/harness/BUILDING-WITH-HARNESS.md'],
      impacted_areas: ['workflow', 'security', 'harness'],
    },
    {
      pattern: 'src/config/**',
      owned_by: ['SPEC.md', 'ARCHITECTURE.md', 'docs/design-docs/'],
      impacted_areas: ['architecture', 'config', 'documentation'],
    },
  ],
}

describe('generate change evidence', () => {
  it('builds a deterministic stub from changed files and the context map', () => {
    const evidence = inferEvidence(
      'build-time-harness',
      ['src/execution/workspace/manager.ts', 'WORKFLOW.md'],
      fixtureContextMap,
    )

    expect(evidence).toMatchObject({
      topic: 'build-time-harness',
      changedFiles: ['src/execution/workspace/manager.ts', 'WORKFLOW.md'],
      decisionArtifacts: [],
      canonicalDocsUpdated: [],
      waivers: [],
      verification: [{ command: 'fill-in-command', result: 'pending' }],
      verificationArtifacts: [],
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
    const evidence = inferEvidence('config-change', ['src/config/build-effective-config.ts'], fixtureContextMap)
    const markdown = renderMarkdownEvidence(evidence)

    expect(markdown).toContain('# Change Evidence: config-change')
    expect(markdown).toContain('## Context Loaded')
    expect(markdown).toContain('## Decision Artifacts')
    expect(markdown).toContain('## Canonical Docs Updated')
    expect(markdown).toContain('## Verification')
    expect(markdown).toContain('## Verification Artifacts')
    expect(markdown).toContain('`ARCHITECTURE.md`')
  })
})
