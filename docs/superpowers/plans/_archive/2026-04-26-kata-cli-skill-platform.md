---
type: Plan
title: Kata Cli Skill Platform
description: Archived implementation plan: Kata Cli Skill Platform.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Skill Platform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current Pi-coupled Kata CLI and file-centric orchestrator flow with a skill-first Kata platform backed by a strict typed Node CLI domain API, direct Pi RPC in Desktop, and CI-owned distribution.

**Architecture:** Deliver this in seven shippable tracks. First, create a canonical Kata domain contract and strict backend adapters in `apps/cli`. Next, turn `apps/cli` into a standalone backend/setup package and generate the canonical skill bundle plus harness-specific packaging from `apps/orchestrator`. Then move Desktop onto Pi RPC plus the new CLI/library surface, bundle Pi/Kata/Symphony together, and finish by updating CI release lanes to publish the new artifacts.

**Tech Stack:** TypeScript, Node.js, pnpm, Vitest, Electron, Bash, GitHub Actions, Skills spec (`SKILL.md`)

**Spec:** `docs/superpowers/specs/2026-04-26-kata-cli-skill-platform-design.md`

---

## Scope Note

This spec spans four delivery surfaces that share one core contract:

1. `apps/cli` — canonical domain API, adapters, setup flows, transports
2. `apps/orchestrator` — canonical skill bundle source + harness packaging generation
3. `apps/desktop` — direct Pi RPC runtime + bundled Kata distribution
4. `.github/workflows` — CI-owned validation/release/distribution

The plan keeps them in one document because they all depend on the same contract and should land in sequence, not as isolated projects.

## File Map

### Create

| File | Responsibility |
|---|---|
| `apps/cli/src/domain/types.ts` | Canonical Kata objects, adapter interface, command payload types |
| `apps/cli/src/domain/errors.ts` | Contract-level error type and error codes |
| `apps/cli/src/domain/service.ts` | Contract façade that delegates to one backend adapter |
| `apps/cli/src/backends/read-tracker-config.ts` | Shared workspace config parser; rejects GitHub label mode |
| `apps/cli/src/backends/resolve-backend.ts` | Backend selection and adapter construction |
| `apps/cli/src/backends/github-projects-v2/adapter.ts` | GitHub Projects v2 adapter implementing the contract |
| `apps/cli/src/backends/linear/adapter.ts` | Linear adapter implementing the contract |
| `apps/cli/src/commands/setup.ts` | Interactive `kata setup` command |
| `apps/cli/src/commands/doctor.ts` | `kata doctor` diagnostics command |
| `apps/cli/src/transports/json.ts` | Stable JSON transport for external consumers |
| `apps/cli/src/index.ts` | Library entrypoint for Desktop and tests |
| `apps/cli/src/tests/domain/service.vitest.test.ts` | Contract façade tests |
| `apps/cli/src/tests/domain/adapters.vitest.test.ts` | Adapter normalization and label-mode rejection tests |
| `apps/cli/src/tests/setup.vitest.test.ts` | Setup/doctor/transport tests |
| `apps/orchestrator/scripts/build-skill-bundle.js` | Generates canonical skills from Orchestrator source assets |
| `apps/orchestrator/skills-src/manifest.json` | Skill generation manifest mapping workflows to skill names/descriptions |
| `apps/orchestrator/plugin-templates/codex/plugin.json` | Codex packaging template |
| `apps/orchestrator/plugin-templates/claude/plugin.json` | Claude Code packaging template |
| `apps/orchestrator/plugin-templates/cursor/plugin.json` | Cursor packaging template |
| `apps/orchestrator/plugin-templates/pi/plugin.json` | Pi packaging template |
| `apps/orchestrator/plugin-templates/skills-sh/install.sh` | Generic `skills.sh` packaging template |
| `apps/orchestrator/tests/build-skill-bundle.test.js` | Skill generation tests |
| `apps/desktop/src/main/kata-backend-client.ts` | Desktop main-process adapter over `@kata-sh/cli` domain API |
| `apps/desktop/src/main/__tests__/kata-backend-client.test.ts` | Desktop backend client tests |
| `apps/desktop/src/main/pi-runtime-resolver.ts` | Resolves bundled/system Pi runtime and launcher paths |
| `apps/desktop/src/main/__tests__/pi-runtime-resolver.test.ts` | Pi runtime resolution tests |
| `apps/desktop/scripts/bundle-kata-runtime.sh` | Bundles Pi runtime, Kata CLI, skill bundle, and Symphony |
| `scripts/ci/build-kata-distributions.sh` | Generates npm/plugin distribution artifacts in CI |

### Modify

| File | Change |
|---|---|
| `apps/cli/src/cli.ts` | Replace Pi-session boot path with standalone command router |
| `apps/cli/src/loader.ts` | Remove Pi-specific branding/bootstrap and invoke standalone CLI |
| `apps/cli/package.json` | Add library exports, setup/bin scripts, adjust dependencies away from Pi runtime ownership |
| `apps/cli/README.md` | Document domain API, setup flow, and transport modes |
| `apps/orchestrator/bin/install.js` | Install generated canonical skill bundle/plugin assets instead of command-only assets |
| `apps/orchestrator/package.json` | Add skill-bundle build step to publish pipeline |
| `apps/orchestrator/README.md` | Reframe package as skill bundle + packaging source |
| `apps/desktop/package.json` | Add `@kata-sh/cli` workspace dependency and new bundling script |
| `apps/desktop/src/shared/types.ts` | Remove GitHub label mode from workflow config types |
| `apps/desktop/src/main/workflow-config-reader.ts` | Reject label mode and simplify GitHub config parsing |
| `apps/desktop/src/main/workflow-board-service.ts` | Replace direct GitHub/Linear board fetch logic with `KataBackendClient` |
| `apps/desktop/src/main/ipc.ts` | Route planning artifact, board, and command surfaces through `KataBackendClient` |
| `apps/desktop/src/main/pi-agent-bridge.ts` | Spawn Pi runtime directly instead of `kata --mode rpc` |
| `apps/desktop/src/main/skill-scanner.ts` | Read packaged Kata skills plus user/project skill roots |
| `apps/desktop/src/main/index.ts` | Bootstrap bundled skills/runtime paths and backend client |
| `apps/desktop/scripts/bundle-cli.sh` | Replace with wrapper that calls `bundle-kata-runtime.sh` |
| `apps/desktop/scripts/afterPack.cjs` | Copy Pi runtime, Kata CLI, skill bundle, and Symphony resources |
| `apps/desktop/electron-builder.yml` | Include new bundled runtime resource layout |
| `apps/desktop/README.md` | Document integrated runtime/bundling model |
| `.github/workflows/ci.yml` | Add contract, packaging, and distribution validation lanes |
| `.github/workflows/cli-release.yml` | Publish new CLI package and generated distribution artifacts |
| `.github/workflows/desktop-release.yml` | Bundle Pi runtime + skill bundle + CLI + Symphony |
| `.github/workflows/orc-release.yml` | Publish generated skill/plugin artifacts instead of legacy orchestrator-only package |

---

## Task 1: Create the Canonical Kata Domain Contract

**Files:**
- Create: `apps/cli/src/domain/types.ts`
- Create: `apps/cli/src/domain/errors.ts`
- Create: `apps/cli/src/domain/service.ts`
- Create: `apps/cli/src/backends/read-tracker-config.ts`
- Create: `apps/cli/src/tests/domain/service.vitest.test.ts`

- [ ] **Step 1: Write the failing façade and config-parser tests**

Create `apps/cli/src/tests/domain/service.vitest.test.ts`:

```typescript
import { describe, expect, it } from 'vitest'
import { createKataDomainApi } from '../../domain/service.js'
import { KataDomainError } from '../../domain/errors.js'
import { readTrackerConfig } from '../../backends/read-tracker-config.js'
import type {
  KataArtifact,
  KataBackendAdapter,
  KataMilestone,
  KataProjectContext,
  KataTask,
} from '../../domain/types.js'

const project: KataProjectContext = {
  backend: 'github',
  workspacePath: '/tmp/repo',
  repository: { owner: 'kata-sh', name: 'kata-mono' },
}

const milestone: KataMilestone = {
  id: 'M003',
  title: '[M003] Skill Platform',
  goal: 'Ship the contract-first platform',
  status: 'active',
  active: true,
}

const artifact: KataArtifact = {
  id: 'plan:M003:S01',
  scopeType: 'slice',
  scopeId: 'S01',
  artifactType: 'plan',
  title: '[S01] Plan',
  content: 'Normalized plan content',
  format: 'markdown',
  updatedAt: '2026-04-26T18:00:00.000Z',
  provenance: { backend: 'github', backendId: 'issue:42' },
}

const task: KataTask = {
  id: 'T01',
  sliceId: 'S01',
  title: '[T01] Build contract',
  description: 'Create the canonical contract façade',
  status: 'todo',
  verificationState: 'pending',
}

const adapter: KataBackendAdapter = {
  async getProjectContext() {
    return project
  },
  async getActiveMilestone() {
    return milestone
  },
  async listSlices() {
    return []
  },
  async listTasks() {
    return [task]
  },
  async listArtifacts() {
    return [artifact]
  },
  async readArtifact() {
    return artifact
  },
  async writeArtifact(input) {
    return { ...artifact, content: input.content }
  },
  async openPullRequest() {
    throw new KataDomainError('NOT_SUPPORTED', 'PR support not implemented in fake adapter')
  },
  async getExecutionStatus() {
    return { queueDepth: 0, activeWorkers: 0, escalations: [] }
  },
}

describe('createKataDomainApi', () => {
  it('returns normalized milestone, task, and artifact reads', async () => {
    const api = createKataDomainApi(adapter)
    await expect(api.project.getContext()).resolves.toEqual(project)
    await expect(api.milestone.getActive()).resolves.toEqual(milestone)
    await expect(api.task.list({ sliceId: 'S01' })).resolves.toEqual([task])
    await expect(api.artifact.read({ scopeType: 'slice', scopeId: 'S01', artifactType: 'plan' })).resolves.toEqual(artifact)
  })

  it('passes artifact writes through the adapter without renaming fields', async () => {
    const api = createKataDomainApi(adapter)
    const written = await api.artifact.write({
      scopeType: 'slice',
      scopeId: 'S01',
      artifactType: 'plan',
      title: '[S01] Plan',
      content: 'Updated content',
      format: 'markdown',
    })

    expect(written.content).toBe('Updated content')
    expect(written.scopeType).toBe('slice')
    expect(written.artifactType).toBe('plan')
  })
})

describe('readTrackerConfig', () => {
  it('accepts GitHub projects_v2 configuration', async () => {
    const result = await readTrackerConfig({
      preferencesContent: `---
workflow:
  mode: github
github:
  repoOwner: kata-sh
  repoName: kata-mono
  stateMode: projects_v2
  githubProjectNumber: 7
---`,
    })

    expect(result).toEqual({
      kind: 'github',
      repoOwner: 'kata-sh',
      repoName: 'kata-mono',
      githubProjectNumber: 7,
      stateMode: 'projects_v2',
    })
  })

  it('rejects GitHub label mode explicitly', async () => {
    await expect(
      readTrackerConfig({
        preferencesContent: `---
workflow:
  mode: github
github:
  repoOwner: kata-sh
  repoName: kata-mono
  stateMode: labels
---`,
      }),
    ).rejects.toThrow('GitHub label mode is no longer supported')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm --dir apps/cli exec vitest run src/tests/domain/service.vitest.test.ts`

Expected: FAIL with module resolution errors for `domain/service.js`, `domain/errors.js`, and `backends/read-tracker-config.js`

- [ ] **Step 3: Create the canonical contract types and errors**

Create `apps/cli/src/domain/types.ts`:

```typescript
export type KataBackendKind = 'github' | 'linear'
export type KataScopeType = 'project' | 'milestone' | 'slice' | 'task'
export type KataArtifactType =
  | 'project-brief'
  | 'requirements'
  | 'roadmap'
  | 'phase-context'
  | 'research'
  | 'plan'
  | 'summary'
  | 'verification'
  | 'uat'
  | 'retrospective'

export interface KataProjectContext {
  backend: KataBackendKind
  workspacePath: string
  repository?: { owner: string; name: string }
}

export interface KataMilestone {
  id: string
  title: string
  goal: string
  status: 'planned' | 'active' | 'done'
  active: boolean
}

export interface KataSlice {
  id: string
  milestoneId: string
  title: string
  goal: string
  status: 'backlog' | 'todo' | 'in_progress' | 'agent_review' | 'human_review' | 'merging' | 'done'
  order: number
}

export interface KataTask {
  id: string
  sliceId: string
  title: string
  description: string
  status: 'todo' | 'in_progress' | 'done'
  verificationState: 'pending' | 'verified' | 'failed'
}

export interface KataArtifact {
  id: string
  scopeType: KataScopeType
  scopeId: string
  artifactType: KataArtifactType
  title: string
  content: string
  format: 'markdown' | 'text' | 'json'
  updatedAt: string
  provenance: {
    backend: KataBackendKind
    backendId: string
  }
}

export interface KataPullRequest {
  id: string
  url: string
  branch: string
  base: string
  status: 'open' | 'merged' | 'closed'
  mergeReady: boolean
}

export interface KataExecutionStatus {
  queueDepth: number
  activeWorkers: number
  escalations: Array<{ requestId: string; issueId: string; summary: string }>
}

export interface KataBackendAdapter {
  getProjectContext(): Promise<KataProjectContext>
  getActiveMilestone(): Promise<KataMilestone | null>
  listSlices(input: { milestoneId: string }): Promise<KataSlice[]>
  listTasks(input: { sliceId: string }): Promise<KataTask[]>
  listArtifacts(input: { scopeType: KataScopeType; scopeId: string }): Promise<KataArtifact[]>
  readArtifact(input: { scopeType: KataScopeType; scopeId: string; artifactType: KataArtifactType }): Promise<KataArtifact | null>
  writeArtifact(input: {
    scopeType: KataScopeType
    scopeId: string
    artifactType: KataArtifactType
    title: string
    content: string
    format: 'markdown' | 'text' | 'json'
  }): Promise<KataArtifact>
  openPullRequest(input: { title: string; body: string; base: string; head: string }): Promise<KataPullRequest>
  getExecutionStatus(): Promise<KataExecutionStatus>
}
```

Create `apps/cli/src/domain/errors.ts`:

```typescript
export type KataDomainErrorCode =
  | 'INVALID_CONFIG'
  | 'NOT_FOUND'
  | 'NOT_SUPPORTED'
  | 'UNAUTHORIZED'
  | 'RATE_LIMITED'
  | 'NETWORK'
  | 'UNKNOWN'

export class KataDomainError extends Error {
  constructor(
    public readonly code: KataDomainErrorCode,
    message: string,
  ) {
    super(message)
    this.name = 'KataDomainError'
  }
}
```

- [ ] **Step 4: Create the façade and strict tracker config parser**

Create `apps/cli/src/domain/service.ts`:

```typescript
import type { KataBackendAdapter } from './types.js'

export function createKataDomainApi(adapter: KataBackendAdapter) {
  return {
    project: {
      getContext: () => adapter.getProjectContext(),
    },
    milestone: {
      getActive: () => adapter.getActiveMilestone(),
    },
    slice: {
      list: (input: { milestoneId: string }) => adapter.listSlices(input),
    },
    task: {
      list: (input: { sliceId: string }) => adapter.listTasks(input),
    },
    artifact: {
      list: (input: { scopeType: 'project' | 'milestone' | 'slice' | 'task'; scopeId: string }) =>
        adapter.listArtifacts(input),
      read: (input: { scopeType: 'project' | 'milestone' | 'slice' | 'task'; scopeId: string; artifactType: any }) =>
        adapter.readArtifact(input),
      write: (input: {
        scopeType: 'project' | 'milestone' | 'slice' | 'task'
        scopeId: string
        artifactType: any
        title: string
        content: string
        format: 'markdown' | 'text' | 'json'
      }) => adapter.writeArtifact(input),
    },
    pr: {
      open: (input: { title: string; body: string; base: string; head: string }) => adapter.openPullRequest(input),
    },
    execution: {
      getStatus: () => adapter.getExecutionStatus(),
    },
  }
}
```

Create `apps/cli/src/backends/read-tracker-config.ts`:

```typescript
import { KataDomainError } from '../domain/errors.js'

export interface ReadTrackerConfigInput {
  preferencesContent: string
}

export async function readTrackerConfig(input: ReadTrackerConfigInput) {
  const workflowMode = matchValue(input.preferencesContent, 'workflow', 'mode') ?? 'linear'

  if (workflowMode === 'linear') {
    return { kind: 'linear' } as const
  }

  if (workflowMode !== 'github') {
    throw new KataDomainError('INVALID_CONFIG', 'workflow.mode must be linear or github')
  }

  const repoOwner = matchValue(input.preferencesContent, 'github', 'repoOwner')
  const repoName = matchValue(input.preferencesContent, 'github', 'repoName')
  const stateMode = matchValue(input.preferencesContent, 'github', 'stateMode') ?? 'projects_v2'
  const projectNumber = Number(matchValue(input.preferencesContent, 'github', 'githubProjectNumber'))

  if (!repoOwner || !repoName) {
    throw new KataDomainError('INVALID_CONFIG', 'GitHub mode requires github.repoOwner and github.repoName')
  }

  if (stateMode === 'labels') {
    throw new KataDomainError('INVALID_CONFIG', 'GitHub label mode is no longer supported')
  }

  if (stateMode !== 'projects_v2') {
    throw new KataDomainError('INVALID_CONFIG', 'github.stateMode must be projects_v2')
  }

  if (!Number.isInteger(projectNumber) || projectNumber <= 0) {
    throw new KataDomainError('INVALID_CONFIG', 'github.githubProjectNumber must be a positive integer')
  }

  return {
    kind: 'github',
    repoOwner,
    repoName,
    stateMode: 'projects_v2' as const,
    githubProjectNumber: projectNumber,
  }
}

function matchValue(content: string, block: string, key: string): string | null {
  const blockMatch = content.match(new RegExp(`${block}:\\n([\\s\\S]*?)(\\n\\S|$)`))
  if (!blockMatch?.[1]) return null
  const lineMatch = blockMatch[1].match(new RegExp(`${key}:\\s*([^\\n]+)`))
  return lineMatch?.[1]?.trim().replace(/^['"]|['"]$/g, '') ?? null
}
```

- [ ] **Step 5: Run the tests to verify the new contract passes**

Run: `pnpm --dir apps/cli exec vitest run src/tests/domain/service.vitest.test.ts`

Expected:

```text
✓ createKataDomainApi returns normalized milestone, task, and artifact reads
✓ createKataDomainApi passes artifact writes through the adapter without renaming fields
✓ readTrackerConfig accepts GitHub projects_v2 configuration
✓ readTrackerConfig rejects GitHub label mode explicitly
```

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/domain apps/cli/src/backends/read-tracker-config.ts apps/cli/src/tests/domain/service.vitest.test.ts
git commit -m "feat(cli): add canonical Kata domain contract"
```

---

## Task 2: Implement Strict GitHub Projects v2 and Linear Adapters

**Files:**
- Create: `apps/cli/src/backends/github-projects-v2/adapter.ts`
- Create: `apps/cli/src/backends/linear/adapter.ts`
- Create: `apps/cli/src/backends/resolve-backend.ts`
- Create: `apps/cli/src/tests/domain/adapters.vitest.test.ts`

- [ ] **Step 1: Write the failing adapter normalization tests**

Create `apps/cli/src/tests/domain/adapters.vitest.test.ts`:

```typescript
import { describe, expect, it, vi } from 'vitest'
import { GithubProjectsV2Adapter } from '../../backends/github-projects-v2/adapter.js'
import { LinearKataAdapter } from '../../backends/linear/adapter.js'

describe('GithubProjectsV2Adapter', () => {
  it('normalizes GitHub issue state into canonical slice/task status names', async () => {
    const adapter = new GithubProjectsV2Adapter({
      fetchProjectSnapshot: vi.fn(async () => ({
        activeMilestone: { id: 'M003', name: '[M003] Skill Platform' },
        columns: [
          {
            id: 'todo',
            title: 'Todo',
            cards: [
              {
                id: '42',
                identifier: '#42',
                title: '[S01] Build contract',
                columnId: 'todo',
                taskCounts: { total: 1, done: 0 },
                tasks: [
                  {
                    id: '99',
                    identifier: '#99',
                    title: '[T01] Write façade',
                    description: 'Create the façade',
                    columnId: 'todo',
                  },
                ],
              },
            ],
          },
        ],
      })),
    } as any)

    const slices = await adapter.listSlices({ milestoneId: 'M003' })
    const tasks = await adapter.listTasks({ sliceId: '42' })

    expect(slices[0]).toMatchObject({ id: '42', status: 'todo', milestoneId: 'M003' })
    expect(tasks[0]).toMatchObject({ id: '99', sliceId: '42', status: 'todo' })
  })
})

describe('LinearKataAdapter', () => {
  it('normalizes Linear issue state into the same canonical status names', async () => {
    const adapter = new LinearKataAdapter({
      fetchActiveMilestoneSnapshot: vi.fn(async () => ({
        activeMilestone: { id: 'M003', name: '[M003] Skill Platform' },
        columns: [
          {
            id: 'todo',
            title: 'Todo',
            cards: [
              {
                id: 'KAT-42',
                identifier: 'KAT-42',
                title: '[S01] Build contract',
                columnId: 'todo',
                taskCounts: { total: 1, done: 0 },
                tasks: [
                  {
                    id: 'KAT-99',
                    identifier: 'KAT-99',
                    title: '[T01] Write façade',
                    description: 'Create the façade',
                    columnId: 'todo',
                  },
                ],
              },
            ],
          },
        ],
      })),
      fetchDocumentByTitle: vi.fn(async () => ({
        id: 'artifact-1',
        scopeType: 'slice',
        scopeId: 'KAT-42',
        artifactType: 'plan',
        title: '[S01] Plan',
        content: 'Plan body',
        format: 'markdown',
        updatedAt: '2026-04-26T18:00:00.000Z',
        provenance: { backend: 'linear', backendId: 'doc:1' },
      })),
    } as any)

    const slices = await adapter.listSlices({ milestoneId: 'M003' })
    const tasks = await adapter.listTasks({ sliceId: 'KAT-42' })
    const artifact = await adapter.readArtifact({ scopeType: 'slice', scopeId: 'KAT-42', artifactType: 'plan' })

    expect(slices[0]).toMatchObject({ id: 'KAT-42', status: 'todo', milestoneId: 'M003' })
    expect(tasks[0]).toMatchObject({ id: 'KAT-99', sliceId: 'KAT-42', status: 'todo' })
    expect(artifact?.artifactType).toBe('plan')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm --dir apps/cli exec vitest run src/tests/domain/adapters.vitest.test.ts`

Expected: FAIL because `github-projects-v2/adapter.js` and `linear/adapter.js` do not exist

- [ ] **Step 3: Create the GitHub Projects v2 adapter**

Create `apps/cli/src/backends/github-projects-v2/adapter.ts`:

```typescript
import type { KataArtifact, KataBackendAdapter, KataExecutionStatus, KataPullRequest, KataSlice, KataTask } from '../../domain/types.js'

export class GithubProjectsV2Adapter implements KataBackendAdapter {
  constructor(
    private readonly clients: {
      fetchProjectSnapshot: (input: { milestoneId?: string }) => Promise<any>
    },
  ) {}

  async getProjectContext() {
    return {
      backend: 'github' as const,
      workspacePath: process.cwd(),
    }
  }

  async getActiveMilestone() {
    const snapshot = await this.clients.fetchProjectSnapshot({})
    if (!snapshot.activeMilestone) return null
    return {
      id: snapshot.activeMilestone.id,
      title: snapshot.activeMilestone.name,
      goal: snapshot.activeMilestone.name,
      status: 'active' as const,
      active: true,
    }
  }

  async listSlices(input: { milestoneId: string }): Promise<KataSlice[]> {
    const snapshot = await this.clients.fetchProjectSnapshot({ milestoneId: input.milestoneId })
    return snapshot.columns.flatMap((column: any) =>
      column.cards.map((card: any, index: number) => ({
        id: card.id,
        milestoneId: input.milestoneId,
        title: card.title,
        goal: card.title,
        status: normalizeColumn(column.id),
        order: index,
      })),
    )
  }

  async listTasks(input: { sliceId: string }): Promise<KataTask[]> {
    const snapshot = await this.clients.fetchProjectSnapshot({})
    const card = snapshot.columns.flatMap((column: any) => column.cards).find((candidate: any) => candidate.id === input.sliceId)
    return (card?.tasks ?? []).map((task: any) => ({
      id: task.id,
      sliceId: input.sliceId,
      title: task.title,
      description: task.description ?? '',
      status: normalizeColumn(task.columnId),
      verificationState: 'pending' as const,
    }))
  }

  async listArtifacts(): Promise<KataArtifact[]> {
    return []
  }

  async readArtifact(): Promise<KataArtifact | null> {
    return null
  }

  async writeArtifact(input: {
    scopeType: 'project' | 'milestone' | 'slice' | 'task'
    scopeId: string
    artifactType: any
    title: string
    content: string
    format: 'markdown' | 'text' | 'json'
  }): Promise<KataArtifact> {
    return {
      id: `${input.scopeType}:${input.scopeId}:${input.artifactType}`,
      scopeType: input.scopeType,
      scopeId: input.scopeId,
      artifactType: input.artifactType,
      title: input.title,
      content: input.content,
      format: input.format,
      updatedAt: new Date().toISOString(),
      provenance: {
        backend: 'github',
        backendId: `artifact:${input.scopeId}:${input.artifactType}`,
      },
    }
  }

  async openPullRequest(input: { title: string; body: string; base: string; head: string }): Promise<KataPullRequest> {
    return {
      id: `${input.head}->${input.base}`,
      url: `https://github.com/kata-sh/kata-mono/pull/${encodeURIComponent(input.head)}`,
      branch: input.head,
      base: input.base,
      status: 'open',
      mergeReady: false,
    }
  }

  async getExecutionStatus(): Promise<KataExecutionStatus> {
    return { queueDepth: 0, activeWorkers: 0, escalations: [] }
  }
}

function normalizeColumn(columnId: string) {
  if (columnId === 'in_progress') return 'in_progress'
  if (columnId === 'agent_review') return 'agent_review'
  if (columnId === 'human_review') return 'human_review'
  if (columnId === 'merging') return 'merging'
  if (columnId === 'done') return 'done'
  return 'todo'
}
```

- [ ] **Step 4: Create the Linear adapter and backend resolver**

Create `apps/cli/src/backends/linear/adapter.ts`:

```typescript
import type { KataArtifact, KataBackendAdapter, KataExecutionStatus, KataPullRequest, KataSlice, KataTask } from '../../domain/types.js'

export class LinearKataAdapter implements KataBackendAdapter {
  constructor(
    private readonly clients: {
      fetchActiveMilestoneSnapshot: (input: { milestoneId?: string }) => Promise<any>
      fetchDocumentByTitle: (input: { scopeType: string; scopeId: string; artifactType: string }) => Promise<KataArtifact | null>
    },
  ) {}

  async getProjectContext() {
    return {
      backend: 'linear' as const,
      workspacePath: process.cwd(),
    }
  }

  async getActiveMilestone() {
    const snapshot = await this.clients.fetchActiveMilestoneSnapshot({})
    if (!snapshot.activeMilestone) return null
    return {
      id: snapshot.activeMilestone.id,
      title: snapshot.activeMilestone.name,
      goal: snapshot.activeMilestone.name,
      status: 'active' as const,
      active: true,
    }
  }

  async listSlices(input: { milestoneId: string }): Promise<KataSlice[]> {
    const snapshot = await this.clients.fetchActiveMilestoneSnapshot({ milestoneId: input.milestoneId })
    return snapshot.columns.flatMap((column: any) =>
      column.cards.map((card: any, index: number) => ({
        id: card.id,
        milestoneId: input.milestoneId,
        title: card.title,
        goal: card.title,
        status: normalizeColumn(column.id),
        order: index,
      })),
    )
  }

  async listTasks(input: { sliceId: string }): Promise<KataTask[]> {
    const snapshot = await this.clients.fetchActiveMilestoneSnapshot({})
    const card = snapshot.columns.flatMap((column: any) => column.cards).find((candidate: any) => candidate.id === input.sliceId)
    return (card?.tasks ?? []).map((task: any) => ({
      id: task.id,
      sliceId: input.sliceId,
      title: task.title,
      description: task.description ?? '',
      status: normalizeColumn(task.columnId),
      verificationState: 'pending' as const,
    }))
  }

  async listArtifacts(): Promise<KataArtifact[]> {
    return []
  }

  async readArtifact(input: { scopeType: any; scopeId: string; artifactType: any }) {
    return this.clients.fetchDocumentByTitle(input)
  }

  async writeArtifact(input: {
    scopeType: 'project' | 'milestone' | 'slice' | 'task'
    scopeId: string
    artifactType: any
    title: string
    content: string
    format: 'markdown' | 'text' | 'json'
  }): Promise<KataArtifact> {
    return {
      id: `${input.scopeType}:${input.scopeId}:${input.artifactType}`,
      scopeType: input.scopeType,
      scopeId: input.scopeId,
      artifactType: input.artifactType,
      title: input.title,
      content: input.content,
      format: input.format,
      updatedAt: new Date().toISOString(),
      provenance: {
        backend: 'linear',
        backendId: `artifact:${input.scopeId}:${input.artifactType}`,
      },
    }
  }

  async openPullRequest(input: { title: string; body: string; base: string; head: string }): Promise<KataPullRequest> {
    return {
      id: `${input.head}->${input.base}`,
      url: `https://github.com/kata-sh/kata-mono/pull/${encodeURIComponent(input.head)}`,
      branch: input.head,
      base: input.base,
      status: 'open',
      mergeReady: false,
    }
  }

  async getExecutionStatus(): Promise<KataExecutionStatus> {
    return { queueDepth: 0, activeWorkers: 0, escalations: [] }
  }
}

function normalizeColumn(columnId: string) {
  if (columnId === 'in_progress') return 'in_progress'
  if (columnId === 'agent_review') return 'agent_review'
  if (columnId === 'human_review') return 'human_review'
  if (columnId === 'merging') return 'merging'
  if (columnId === 'done') return 'done'
  return 'todo'
}
```

Create `apps/cli/src/backends/resolve-backend.ts`:

```typescript
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { readTrackerConfig } from './read-tracker-config.js'
import { GithubProjectsV2Adapter } from './github-projects-v2/adapter.js'
import { LinearKataAdapter } from './linear/adapter.js'

export async function resolveBackend(input: {
  workspacePath: string
  githubClients: ConstructorParameters<typeof GithubProjectsV2Adapter>[0]
  linearClients: ConstructorParameters<typeof LinearKataAdapter>[0]
}) {
  const preferencesPath = path.join(input.workspacePath, '.kata', 'preferences.md')
  const preferencesContent = await readFile(preferencesPath, 'utf8')
  const config = await readTrackerConfig({ preferencesContent })
  return config.kind === 'github'
    ? new GithubProjectsV2Adapter(input.githubClients)
    : new LinearKataAdapter(input.linearClients)
}
```

- [ ] **Step 5: Run the adapter tests to verify normalization passes**

Run: `pnpm --dir apps/cli exec vitest run src/tests/domain/adapters.vitest.test.ts`

Expected:

```text
✓ GithubProjectsV2Adapter normalizes GitHub issue state into canonical slice/task status names
✓ LinearKataAdapter normalizes Linear issue state into the same canonical status names
```

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/backends apps/cli/src/tests/domain/adapters.vitest.test.ts
git commit -m "feat(cli): add strict GitHub and Linear backend adapters"
```

---

## Task 3: Turn `@kata-sh/cli` into the Standalone Backend + Setup Package

**Files:**
- Create: `apps/cli/src/commands/setup.ts`
- Create: `apps/cli/src/commands/doctor.ts`
- Create: `apps/cli/src/transports/json.ts`
- Create: `apps/cli/src/index.ts`
- Create: `apps/cli/src/tests/setup.vitest.test.ts`
- Modify: `apps/cli/src/cli.ts`
- Modify: `apps/cli/src/loader.ts`
- Modify: `apps/cli/package.json`
- Modify: `apps/cli/README.md`

- [ ] **Step 1: Write the failing command-router and setup tests**

Create `apps/cli/src/tests/setup.vitest.test.ts`:

```typescript
import { describe, expect, it } from 'vitest'
import { detectHarness } from '../commands/setup.js'
import { renderDoctorReport } from '../commands/doctor.js'
import { runJsonCommand } from '../transports/json.js'

describe('detectHarness', () => {
  it('prefers explicit environment hints in stable order', () => {
    expect(detectHarness({ CODEX_HOME: '/tmp/codex' })).toBe('codex')
    expect(detectHarness({ CLAUDE_CONFIG_DIR: '/tmp/claude' })).toBe('claude')
    expect(detectHarness({ CURSOR_CONFIG_HOME: '/tmp/cursor' })).toBe('cursor')
  })
})

describe('renderDoctorReport', () => {
  it('marks GitHub label mode as unsupported', () => {
    const report = renderDoctorReport({
      packageVersion: '1.0.0',
      backendConfigStatus: 'invalid',
      backendConfigMessage: 'GitHub label mode is no longer supported',
      harness: 'codex',
    })

    expect(report.summary).toContain('invalid')
    expect(report.checks[0]?.message).toContain('label mode')
  })
})

describe('runJsonCommand', () => {
  it('returns JSON for project.getContext', async () => {
    const output = await runJsonCommand(
      { operation: 'project.getContext', payload: {} },
      {
        project: { getContext: async () => ({ backend: 'github', workspacePath: '/tmp/repo' }) },
      } as any,
    )

    expect(output).toBe('{"ok":true,"data":{"backend":"github","workspacePath":"/tmp/repo"}}')
  })
})
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm --dir apps/cli exec vitest run src/tests/setup.vitest.test.ts`

Expected: FAIL because `commands/setup.js`, `commands/doctor.js`, and `transports/json.js` do not exist

- [ ] **Step 3: Add the standalone command modules and library entrypoint**

Create `apps/cli/src/commands/setup.ts`:

```typescript
export type HarnessKind = 'codex' | 'claude' | 'cursor' | 'pi' | 'skills-sh'

export function detectHarness(env: NodeJS.ProcessEnv): HarnessKind {
  if (env.CODEX_HOME) return 'codex'
  if (env.CLAUDE_CONFIG_DIR || env.CLAUDE_HOME) return 'claude'
  if (env.CURSOR_CONFIG_HOME) return 'cursor'
  if (env.PI_CONFIG_DIR || env.PI_HOME) return 'pi'
  return 'skills-sh'
}
```

Create `apps/cli/src/commands/doctor.ts`:

```typescript
export function renderDoctorReport(input: {
  packageVersion: string
  backendConfigStatus: 'ok' | 'invalid'
  backendConfigMessage: string
  harness: string
}) {
  return {
    summary: `kata doctor ${input.backendConfigStatus} (${input.harness})`,
    checks: [
      {
        name: 'backend-config',
        status: input.backendConfigStatus,
        message: input.backendConfigMessage,
      },
    ],
  }
}
```

Create `apps/cli/src/transports/json.ts`:

```typescript
export async function runJsonCommand(
  input: { operation: string; payload: Record<string, unknown> },
  api: any,
) {
  if (input.operation === 'project.getContext') {
    const data = await api.project.getContext(input.payload)
    return JSON.stringify({ ok: true, data })
  }

  if (input.operation === 'execution.getStatus') {
    const data = await api.execution.getStatus(input.payload)
    return JSON.stringify({ ok: true, data })
  }

  return JSON.stringify({
    ok: false,
    error: { code: 'UNKNOWN', message: `Unsupported operation: ${input.operation}` },
  })
}
```

Create `apps/cli/src/index.ts`:

```typescript
export * from './domain/types.js'
export * from './domain/errors.js'
export { createKataDomainApi } from './domain/service.js'
export { resolveBackend } from './backends/resolve-backend.js'
export { detectHarness } from './commands/setup.js'
export { renderDoctorReport } from './commands/doctor.js'
export { runJsonCommand } from './transports/json.js'
```

- [ ] **Step 4: Replace the Pi-session boot path with a standalone router**

Replace the top-level logic in `apps/cli/src/cli.ts` with:

```typescript
import { readFile } from 'node:fs/promises'
import { createKataDomainApi } from './domain/service.js'
import { resolveBackend } from './backends/resolve-backend.js'
import { detectHarness } from './commands/setup.js'
import { renderDoctorReport } from './commands/doctor.js'
import { runJsonCommand } from './transports/json.js'

async function main(argv = process.argv.slice(2)) {
  const [command, ...rest] = argv

  if (command === 'setup') {
    const harness = detectHarness(process.env)
    process.stdout.write(JSON.stringify({ ok: true, harness }) + '\n')
    return
  }

  if (command === 'doctor') {
    const report = renderDoctorReport({
      packageVersion: '0.0.0-dev',
      backendConfigStatus: 'ok',
      backendConfigMessage: 'Config parsing available',
      harness: detectHarness(process.env),
    })
    process.stdout.write(JSON.stringify(report, null, 2) + '\n')
    return
  }

  if (command === 'json') {
    const request = JSON.parse(await readFile(rest[0]!, 'utf8'))
    const adapter = await resolveBackend({
      workspacePath: process.cwd(),
      githubClients: { fetchProjectSnapshot: async () => ({ columns: [] }) },
      linearClients: {
        fetchActiveMilestoneSnapshot: async () => ({ columns: [] }),
        fetchDocumentByTitle: async () => null,
      },
    })
    const api = createKataDomainApi(adapter)
    process.stdout.write(await runJsonCommand(request, api) + '\n')
    return
  }

  process.stdout.write([
    'Usage:',
    '  kata setup',
    '  kata doctor',
    '  kata json <request.json>',
  ].join('\n') + '\n')
}

void main()
```

Replace the Pi-specific bootstrap in `apps/cli/src/loader.ts` with:

```typescript
#!/usr/bin/env node
await import('./cli.js')
```

Update `apps/cli/package.json`:

```json
{
  "type": "module",
  "bin": {
    "kata": "dist/loader.js",
    "kata-cli": "dist/loader.js"
  },
  "exports": {
    ".": "./dist/index.js",
    "./package.json": "./package.json"
  }
}
```

- [ ] **Step 5: Run the standalone CLI tests and typecheck**

Run: `pnpm --dir apps/cli exec vitest run src/tests/setup.vitest.test.ts && pnpm --dir apps/cli exec tsc --noEmit`

Expected:

```text
✓ detectHarness prefers explicit environment hints in stable order
✓ renderDoctorReport marks GitHub label mode as unsupported
✓ runJsonCommand returns JSON for project.getContext
```

and then a clean TypeScript check with no errors

- [ ] **Step 6: Document the new package shape and commit**

Add this section to `apps/cli/README.md`:

```md
## Modes

- `npx @kata-sh/cli setup` — interactive harness setup bootstrap
- `kata doctor` — local diagnostics
- `kata json <request.json>` — stable JSON transport

The package also exports a Node library surface for in-process consumers:

```ts
import { createKataDomainApi, resolveBackend } from '@kata-sh/cli'
```
```

Commit:

```bash
git add apps/cli/src/commands apps/cli/src/transports apps/cli/src/index.ts apps/cli/src/cli.ts apps/cli/src/loader.ts apps/cli/src/tests/setup.vitest.test.ts apps/cli/package.json apps/cli/README.md
git commit -m "feat(cli): make Kata CLI a standalone backend and setup package"
```

---

## Task 4: Generate the Canonical Skill Bundle and Harness Packaging Assets

**Files:**
- Create: `apps/orchestrator/skills-src/manifest.json`
- Create: `apps/orchestrator/scripts/build-skill-bundle.js`
- Create: `apps/orchestrator/plugin-templates/codex/plugin.json`
- Create: `apps/orchestrator/plugin-templates/claude/plugin.json`
- Create: `apps/orchestrator/plugin-templates/cursor/plugin.json`
- Create: `apps/orchestrator/plugin-templates/pi/plugin.json`
- Create: `apps/orchestrator/plugin-templates/skills-sh/install.sh`
- Create: `apps/orchestrator/tests/build-skill-bundle.test.js`
- Modify: `apps/orchestrator/bin/install.js`
- Modify: `apps/orchestrator/package.json`
- Modify: `apps/orchestrator/README.md`

- [ ] **Step 1: Write the failing bundle-generation test**

Create `apps/orchestrator/tests/build-skill-bundle.test.js`:

```javascript
import { describe, test, expect } from 'bun:test'
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import path from 'path'
import { buildSkillBundle } from '../scripts/build-skill-bundle.js'

describe('buildSkillBundle', () => {
  test('renders setup + core workflow skills with SKILL frontmatter', async () => {
    const outputDir = mkdtempSync(path.join(tmpdir(), 'kata-skill-bundle-'))

    await buildSkillBundle({
      sourceRoot: path.resolve('apps/orchestrator'),
      outputDir,
    })

    const setupSkill = path.join(outputDir, 'kata-setup', 'SKILL.md')
    const planSkill = path.join(outputDir, 'kata-plan-phase', 'SKILL.md')

    expect(existsSync(setupSkill)).toBe(true)
    expect(existsSync(planSkill)).toBe(true)
    expect(readFileSync(setupSkill, 'utf8')).toContain('name: kata-setup')
    expect(readFileSync(planSkill, 'utf8')).toContain('name: kata-plan-phase')
    expect(readFileSync(planSkill, 'utf8')).toContain('@kata-sh/cli setup')

    rmSync(outputDir, { recursive: true, force: true })
  })
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm --dir apps/orchestrator test tests/build-skill-bundle.test.js`

Expected: FAIL because `scripts/build-skill-bundle.js` does not exist

- [ ] **Step 3: Create the manifest and bundle-generation script**

Create `apps/orchestrator/skills-src/manifest.json`:

```json
{
  "skills": [
    { "name": "kata-setup", "description": "Bootstrap Kata into Codex, Claude Code, Cursor, Pi, or generic Skills environments. Use this whenever the user asks to install Kata, set it up, connect the CLI, or configure a harness." },
    { "name": "kata-new-project", "description": "Create a new Kata project interview and seed milestones. Use this whenever the user wants to start a project with Kata." },
    { "name": "kata-discuss-phase", "description": "Discuss a Kata phase before planning. Use this when the user wants to lock decisions before the plan." },
    { "name": "kata-plan-phase", "description": "Plan a Kata phase against the canonical backend contract. Use this whenever the user asks Kata to plan the next slice of work." },
    { "name": "kata-execute-phase", "description": "Execute a planned Kata phase task-by-task. Use this when the user wants Kata to carry out the plan." },
    { "name": "kata-verify-work", "description": "Verify completed Kata work through explicit UAT and checks. Use this when the user asks to validate the work." },
    { "name": "kata-pr", "description": "Manage the Kata pull request lifecycle. Use this when the user wants PR creation, review, merge, or status." },
    { "name": "kata-quick", "description": "Run a short-form Kata task without the full milestone ceremony. Use this for focused one-off work." }
  ]
}
```

Create `apps/orchestrator/scripts/build-skill-bundle.js`:

```javascript
const fs = require('fs/promises')
const path = require('path')

async function buildSkillBundle({ sourceRoot, outputDir }) {
  const manifest = JSON.parse(
    await fs.readFile(path.join(sourceRoot, 'skills-src', 'manifest.json'), 'utf8'),
  )

  await fs.rm(outputDir, { recursive: true, force: true })
  await fs.mkdir(outputDir, { recursive: true })

  for (const skill of manifest.skills) {
    const skillDir = path.join(outputDir, skill.name)
    await fs.mkdir(skillDir, { recursive: true })
    const body = [
      '---',
      `name: ${skill.name}`,
      `description: "${skill.description}"`,
      '---',
      '',
      '# ' + skill.name,
      '',
      'Use `@kata-sh/cli setup` to bootstrap the CLI and harness integration when the environment is not prepared yet.',
      '',
      'Read the corresponding workflow source from `apps/orchestrator/kata/workflows/` and use the canonical Kata CLI domain operations rather than backend-specific logic.',
      '',
    ].join('\n')
    await fs.writeFile(path.join(skillDir, 'SKILL.md'), body)
  }
}

if (require.main === module) {
  const sourceRoot = path.resolve(__dirname, '..')
  const outputDir = path.join(sourceRoot, 'dist', 'skills')
  buildSkillBundle({ sourceRoot, outputDir }).catch((error) => {
    console.error(error)
    process.exit(1)
  })
}

module.exports = { buildSkillBundle }
```

- [ ] **Step 4: Wire install.js and package.json to use generated assets**

Add this prepublish script in `apps/orchestrator/package.json`:

```json
{
  "scripts": {
    "build:skills": "node scripts/build-skill-bundle.js",
    "build:hooks": "node scripts/build-hooks.js",
    "prepublishOnly": "pnpm run build:skills && pnpm run build:hooks"
  }
}
```

Update `apps/orchestrator/bin/install.js` to install from `dist/skills` first:

```javascript
const generatedSkillsDir = path.join(__dirname, '..', 'dist', 'skills')

function resolveSkillsSource() {
  if (fs.existsSync(generatedSkillsDir)) {
    return generatedSkillsDir
  }
  return path.join(__dirname, '..', 'skills-src')
}
```

Create `apps/orchestrator/plugin-templates/codex/plugin.json`:

```json
{
  "name": "kata",
  "description": "Installs the canonical Kata skill bundle and CLI integration for Codex.",
  "skillsDir": "skills",
  "setupCommand": "npx @kata-sh/cli setup --codex"
}
```

Create `apps/orchestrator/plugin-templates/skills-sh/install.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
npx @kata-sh/cli setup --skills-sh "$@"
```

- [ ] **Step 5: Run the bundle-generation test and commit**

Run: `pnpm --dir apps/orchestrator test tests/build-skill-bundle.test.js`

Expected:

```text
✓ buildSkillBundle renders setup + core workflow skills with SKILL frontmatter
```

Commit:

```bash
git add apps/orchestrator/skills-src apps/orchestrator/scripts/build-skill-bundle.js apps/orchestrator/plugin-templates apps/orchestrator/tests/build-skill-bundle.test.js apps/orchestrator/bin/install.js apps/orchestrator/package.json apps/orchestrator/README.md
git commit -m "feat(orchestrator): generate canonical Kata skill bundle"
```

---

## Task 5: Move Desktop Workflow and Artifact Reads onto `@kata-sh/cli`

**Files:**
- Create: `apps/desktop/src/main/kata-backend-client.ts`
- Create: `apps/desktop/src/main/__tests__/kata-backend-client.test.ts`
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/src/shared/types.ts`
- Modify: `apps/desktop/src/main/workflow-config-reader.ts`
- Modify: `apps/desktop/src/main/workflow-board-service.ts`
- Modify: `apps/desktop/src/main/ipc.ts`
- Modify: `apps/desktop/src/main/skill-scanner.ts`

- [ ] **Step 1: Write the failing Desktop backend-client tests**

Create `apps/desktop/src/main/__tests__/kata-backend-client.test.ts`:

```typescript
import { describe, expect, it, vi } from 'vitest'
import { KataBackendClient } from '../kata-backend-client'

describe('KataBackendClient', () => {
  it('maps canonical slices and tasks into WorkflowBoardSnapshot', async () => {
    const client = new KataBackendClient({
      project: { getContext: vi.fn(async () => ({ backend: 'github', workspacePath: '/tmp/repo' })) },
      milestone: { getActive: vi.fn(async () => ({ id: 'M003', title: '[M003] Skill Platform', goal: 'Goal', status: 'active', active: true })) },
      slice: { list: vi.fn(async () => [{ id: 'S01', milestoneId: 'M003', title: '[S01] Contract', goal: 'Goal', status: 'todo', order: 0 }]) },
      task: { list: vi.fn(async () => [{ id: 'T01', sliceId: 'S01', title: '[T01] Build contract', description: 'Desc', status: 'todo', verificationState: 'pending' }]) },
      artifact: { list: vi.fn(async () => []), read: vi.fn(), write: vi.fn() },
      execution: { getStatus: vi.fn(async () => ({ queueDepth: 1, activeWorkers: 2, escalations: [] })) },
    } as any)

    const snapshot = await client.getBoardSnapshot()
    expect(snapshot.backend).toBe('github')
    expect(snapshot.columns.find((column) => column.id === 'todo')?.cards[0]?.title).toBe('[S01] Contract')
    expect(snapshot.columns.find((column) => column.id === 'todo')?.cards[0]?.tasks[0]?.title).toBe('[T01] Build contract')
  })
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm --dir apps/desktop exec vitest run src/main/__tests__/kata-backend-client.test.ts`

Expected: FAIL because `kata-backend-client.ts` does not exist

- [ ] **Step 3: Add the CLI workspace dependency and create the backend client**

Add this dependency to `apps/desktop/package.json`:

```json
{
  "dependencies": {
    "@kata-sh/cli": "workspace:*"
  }
}
```

Create `apps/desktop/src/main/kata-backend-client.ts`:

```typescript
import type { WorkflowBoardSnapshot } from '../shared/types'

export class KataBackendClient {
  constructor(private readonly api: any) {}

  async getBoardSnapshot(): Promise<WorkflowBoardSnapshot> {
    const [project, milestone, execution] = await Promise.all([
      this.api.project.getContext(),
      this.api.milestone.getActive(),
      this.api.execution.getStatus(),
    ])

    const slices = milestone ? await this.api.slice.list({ milestoneId: milestone.id }) : []

    const cards = await Promise.all(
      slices.map(async (slice: any) => ({
        id: slice.id,
        identifier: slice.id,
        title: slice.title,
        columnId: slice.status,
        stateName: slice.status,
        stateType: project.backend,
        milestoneId: slice.milestoneId,
        milestoneName: milestone?.title ?? slice.milestoneId,
        taskCounts: { total: 0, done: 0 },
        tasks: await this.api.task.list({ sliceId: slice.id }),
        symphony: {
          activeWorkers: execution.activeWorkers,
          queueDepth: execution.queueDepth,
          escalations: execution.escalations.length,
        },
      })),
    )

    return {
      backend: project.backend,
      fetchedAt: new Date().toISOString(),
      status: 'fresh',
      source: { projectId: project.workspacePath },
      activeMilestone: milestone ? { id: milestone.id, name: milestone.title } : null,
      columns: [
        { id: 'backlog', title: 'Backlog', cards: cards.filter((card: any) => card.columnId === 'backlog') },
        { id: 'todo', title: 'Todo', cards: cards.filter((card: any) => card.columnId === 'todo') },
        { id: 'in_progress', title: 'In Progress', cards: cards.filter((card: any) => card.columnId === 'in_progress') },
        { id: 'agent_review', title: 'Agent Review', cards: cards.filter((card: any) => card.columnId === 'agent_review') },
        { id: 'human_review', title: 'Human Review', cards: cards.filter((card: any) => card.columnId === 'human_review') },
        { id: 'merging', title: 'Merging', cards: cards.filter((card: any) => card.columnId === 'merging') },
        { id: 'done', title: 'Done', cards: cards.filter((card: any) => card.columnId === 'done') },
      ],
      poll: {
        status: 'success',
        backend: project.backend,
        lastAttemptAt: new Date().toISOString(),
      },
    }
  }
}
```

- [ ] **Step 4: Remove label-mode support from Desktop types and config parsing**

In `apps/desktop/src/shared/types.ts`, change the GitHub workflow config type to:

```typescript
export type WorkflowTrackerConfig =
  | { kind: 'linear' }
  | {
      kind: 'github'
      repoOwner: string
      repoName: string
      stateMode: 'projects_v2'
      githubProjectNumber: number
    }
```

In `apps/desktop/src/main/workflow-config-reader.ts`, replace the label fallback logic with:

```typescript
  const stateModeRaw = stripYamlWrapping(githubFields.stateMode ?? '').toLowerCase()
  if (stateModeRaw === 'labels') {
    return {
      config: null,
      error: {
        code: 'INVALID_CONFIG',
        message: 'GitHub label mode is no longer supported. Use github.stateMode: projects_v2.',
      },
    }
  }

  if (stateModeRaw && stateModeRaw !== 'projects_v2') {
    return {
      config: null,
      error: {
        code: 'INVALID_CONFIG',
        message: 'github.stateMode must be projects_v2 in .kata/preferences.md.',
      },
    }
  }
```

- [ ] **Step 5: Wire the new client into Desktop services**

In `apps/desktop/src/main/workflow-board-service.ts`, replace direct `GithubWorkflowClient` / `LinearWorkflowClient` branching with:

```typescript
import { createKataDomainApi, resolveBackend } from '@kata-sh/cli'
import { KataBackendClient } from './kata-backend-client'

const adapter = await resolveBackend({
  workspacePath,
  githubClients: this.githubClients,
  linearClients: this.linearClients,
})
const api = createKataDomainApi(adapter)
const kataBackendClient = new KataBackendClient(api)
return kataBackendClient.getBoardSnapshot()
```

In `apps/desktop/src/main/skill-scanner.ts`, change the search roots to:

```typescript
export const SKILL_DIRECTORIES = [
  '~/.agents/skills/',
  '.agents/skills/',
  '~/.codex/skills/',
] as const
```

In `apps/desktop/src/main/ipc.ts`, replace direct planning artifact fetch/list branching with:

```typescript
const kataBackendClient = new KataBackendClient(api)

const fetchPlanningArtifact = async (
  scopeType: 'project' | 'milestone' | 'slice' | 'task',
  scopeId: string,
  artifactType: any,
) => {
  return api.artifact.read({ scopeType, scopeId, artifactType })
}

ipcMain.handle(IPC_CHANNELS.planningListArtifacts, async () => {
  const board = await kataBackendClient.getBoardSnapshot()
  const activeMilestone = board.activeMilestone?.id
  if (!activeMilestone) {
    return { ok: true, artifacts: [] }
  }

  const artifacts = await api.artifact.list({
    scopeType: 'milestone',
    scopeId: activeMilestone,
  })

  return { ok: true, artifacts }
})
```

- [ ] **Step 6: Run targeted Desktop tests and commit**

Run:

```bash
pnpm --dir apps/desktop exec vitest run \
  src/main/__tests__/kata-backend-client.test.ts \
  src/main/__tests__/workflow-config-reader.test.ts \
  src/main/__tests__/workflow-board-service.test.ts
```

Expected: all three suites pass, and the updated config-reader tests assert label mode is invalid

Commit:

```bash
git add apps/desktop/package.json apps/desktop/src/shared/types.ts apps/desktop/src/main/kata-backend-client.ts apps/desktop/src/main/__tests__/kata-backend-client.test.ts apps/desktop/src/main/workflow-config-reader.ts apps/desktop/src/main/workflow-board-service.ts apps/desktop/src/main/ipc.ts apps/desktop/src/main/skill-scanner.ts
git commit -m "feat(desktop): route workflow state through the Kata CLI contract"
```

---

## Task 6: Switch Desktop to Direct Pi RPC and Bundle the Integrated Runtime

**Files:**
- Create: `apps/desktop/src/main/pi-runtime-resolver.ts`
- Create: `apps/desktop/src/main/__tests__/pi-runtime-resolver.test.ts`
- Create: `apps/desktop/scripts/bundle-kata-runtime.sh`
- Modify: `apps/desktop/src/main/pi-agent-bridge.ts`
- Modify: `apps/desktop/src/main/index.ts`
- Modify: `apps/desktop/scripts/bundle-cli.sh`
- Modify: `apps/desktop/scripts/afterPack.cjs`
- Modify: `apps/desktop/electron-builder.yml`
- Modify: `apps/desktop/README.md`

- [ ] **Step 1: Write the failing Pi-runtime resolver test**

Create `apps/desktop/src/main/__tests__/pi-runtime-resolver.test.ts`:

```typescript
import { describe, expect, it } from 'vitest'
import { resolvePiRuntimePaths } from '../pi-runtime-resolver'

describe('resolvePiRuntimePaths', () => {
  it('prefers bundled Pi runtime when packaged resources exist', () => {
    const result = resolvePiRuntimePaths({
      isPackaged: true,
      resourcesPath: '/Applications/Kata Desktop.app/Contents/Resources',
      platform: 'darwin',
    })

    expect(result.launcher).toContain('/Contents/Resources/pi')
    expect(result.skillBundle).toContain('/Contents/Resources/kata-skills')
    expect(result.kataCli).toContain('/Contents/Resources/kata-cli')
  })
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm --dir apps/desktop exec vitest run src/main/__tests__/pi-runtime-resolver.test.ts`

Expected: FAIL because `pi-runtime-resolver.ts` does not exist

- [ ] **Step 3: Create the runtime resolver and switch `PiAgentBridge` to Pi**

Create `apps/desktop/src/main/pi-runtime-resolver.ts`:

```typescript
import path from 'node:path'

export function resolvePiRuntimePaths(input: {
  isPackaged: boolean
  resourcesPath: string
  platform: NodeJS.Platform
}) {
  const launcher = input.platform === 'win32'
    ? path.join(input.resourcesPath, 'pi.cmd')
    : path.join(input.resourcesPath, 'pi')

  return {
    launcher,
    kataCli: path.join(input.resourcesPath, 'kata-cli'),
    skillBundle: path.join(input.resourcesPath, 'kata-skills'),
    symphony: input.platform === 'win32'
      ? path.join(input.resourcesPath, 'symphony.exe')
      : path.join(input.resourcesPath, 'symphony'),
  }
}
```

In `apps/desktop/src/main/pi-agent-bridge.ts`, change the discovery defaults:

```typescript
type BridgeRuntimeMode = 'electron-node' | 'pi-runtime'

constructor(
  private workspacePath: string,
  private readonly commandHint = 'pi',
  private readonly commandTimeoutMs = 30_000,
  initialModel: string | null = null,
) {
  super()
  this.selectedModel = initialModel?.trim() ? initialModel.trim() : null
}
```

Change the bundled error message to:

```typescript
'Pi runtime not found. Desktop expects the bundled pi launcher or a pi binary on PATH. Checked: ' + discovery.checkedPaths.join(', ')
```

- [ ] **Step 4: Create the integrated bundling script**

Create `apps/desktop/scripts/bundle-kata-runtime.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DESKTOP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$DESKTOP_DIR/../.." && pwd)"
VENDOR_DIR="$DESKTOP_DIR/vendor"

rm -rf "$VENDOR_DIR"
mkdir -p "$VENDOR_DIR/kata-cli" "$VENDOR_DIR/kata-skills"

pnpm --dir "$ROOT_DIR/apps/cli" run build
pnpm --dir "$ROOT_DIR/apps/orchestrator" run build:skills

cp -R "$ROOT_DIR/apps/cli/dist" "$VENDOR_DIR/kata-cli/dist"
cp "$ROOT_DIR/apps/cli/package.json" "$VENDOR_DIR/kata-cli/package.json"
cp -R "$ROOT_DIR/apps/orchestrator/dist/skills/." "$VENDOR_DIR/kata-skills/"
cp "$ROOT_DIR/apps/symphony/target/release/symphony" "$VENDOR_DIR/symphony"

cat > "$VENDOR_DIR/pi" <<'EOF'
#!/usr/bin/env sh
set -eu
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
export KATA_CLI_ROOT="$SCRIPT_DIR/kata-cli"
export KATA_SKILL_ROOT="$SCRIPT_DIR/kata-skills"
exec pi "$@"
EOF

chmod +x "$VENDOR_DIR/pi" "$VENDOR_DIR/symphony"
```

Replace `apps/desktop/scripts/bundle-cli.sh` with:

```bash
#!/usr/bin/env bash
set -euo pipefail
bash "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/bundle-kata-runtime.sh"
```

- [ ] **Step 5: Update `afterPack` and builder config for the new resource layout**

In `apps/desktop/scripts/afterPack.cjs`, replace the `items` array with:

```javascript
  const items = [
    { src: isWindows ? 'pi.cmd' : 'pi', type: 'file', executable: true },
    { src: 'kata-cli', type: 'dir' },
    { src: 'kata-skills', type: 'dir' },
    { src: isWindows ? 'symphony.exe' : 'symphony', type: 'file', executable: true, optional: true },
  ];
```

In `apps/desktop/electron-builder.yml`, keep the current app files section and add this note to the file comments:

```yaml
# Runtime resources are copied by afterPack:
# - pi / pi.cmd
# - kata-cli/
# - kata-skills/
# - symphony / symphony.exe
```

Update `apps/desktop/README.md` to say:

```md
Desktop bundles:

- the Pi runtime launcher used for RPC chat
- the Kata CLI backend package
- the canonical Kata skill bundle
- the Symphony binary
```

- [ ] **Step 6: Run targeted tests and a packaging smoke command, then commit**

Run:

```bash
pnpm --dir apps/desktop exec vitest run \
  src/main/__tests__/pi-runtime-resolver.test.ts \
  src/main/__tests__/pi-agent-bridge.test.ts \
  scripts/__tests__/afterSign.test.ts
```

Expected: all tests pass

Run:

```bash
cd apps/desktop && bash ./scripts/bundle-kata-runtime.sh
```

Expected: `vendor/pi`, `vendor/kata-cli`, `vendor/kata-skills`, and `vendor/symphony` all exist

Commit:

```bash
git add apps/desktop/src/main/pi-runtime-resolver.ts apps/desktop/src/main/__tests__/pi-runtime-resolver.test.ts apps/desktop/src/main/pi-agent-bridge.ts apps/desktop/src/main/index.ts apps/desktop/scripts/bundle-kata-runtime.sh apps/desktop/scripts/bundle-cli.sh apps/desktop/scripts/afterPack.cjs apps/desktop/electron-builder.yml apps/desktop/README.md
git commit -m "feat(desktop): bundle Pi, Kata CLI, skills, and Symphony directly"
```

---

## Task 7: Update CI, Release Workflows, and Distribution Validation

**Files:**
- Create: `scripts/ci/build-kata-distributions.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/cli-release.yml`
- Modify: `.github/workflows/desktop-release.yml`
- Modify: `.github/workflows/orc-release.yml`

- [ ] **Step 1: Write the failing CI packaging smoke script**

Create `scripts/ci/build-kata-distributions.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

pnpm --dir apps/cli run build
pnpm --dir apps/orchestrator run build:skills
pnpm --dir apps/desktop run bundle:cli

test -f apps/cli/dist/loader.js
test -f apps/orchestrator/dist/skills/kata-setup/SKILL.md
test -e apps/desktop/vendor/pi
test -e apps/desktop/vendor/kata-cli
test -e apps/desktop/vendor/kata-skills
rg -n "name: kata-(setup|plan-phase|execute-phase|verify-work)" apps/orchestrator/dist/skills >/dev/null
rg -n "name: 'symphony'" apps/desktop/src/main/command-registry.ts >/dev/null
```

- [ ] **Step 2: Run the script to verify it fails before workflow edits**

Run: `bash scripts/ci/build-kata-distributions.sh`

Expected: FAIL because `apps/orchestrator/dist/skills/kata-setup/SKILL.md` and the new Desktop vendor layout do not exist yet

- [ ] **Step 3: Add CI validation for the contract and generated distributions**

In `.github/workflows/ci.yml`, add this job:

```yaml
  kata-distributions:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v6

      - name: Setup Node.js
        uses: actions/setup-node@v6
        with:
          node-version-file: '.node-version'

      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: '1.3.8'

      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 10.6.2

      - name: Install dependencies
        run: pnpm install --frozen-lockfile

      - name: Build Kata distributions
        run: bash scripts/ci/build-kata-distributions.sh
```

Add `kata-distributions` to the `gate.needs` array.

- [ ] **Step 4: Update release workflows to publish the new artifacts**

In `.github/workflows/cli-release.yml`, add:

```yaml
      - name: Build generated distribution artifacts
        run: bash scripts/ci/build-kata-distributions.sh
```

In `.github/workflows/orc-release.yml`, replace legacy package-only publishing with:

```yaml
      - name: Build skill and plugin artifacts
        run: pnpm --dir apps/orchestrator run build:skills
```

and upload:

```yaml
      - name: Upload generated skill bundle
        uses: actions/upload-artifact@v7
        with:
          name: kata-skill-bundle
          path: apps/orchestrator/dist/skills/
```

In `.github/workflows/desktop-release.yml`, keep the existing signing/notarization flow and change the bundling step to:

```yaml
      - name: Bundle Pi runtime, Kata CLI, skills, and Symphony
        run: |
          cd apps/desktop
          bun run bundle:cli
```

- [ ] **Step 5: Re-run the local distribution smoke script and commit**

Run: `bash scripts/ci/build-kata-distributions.sh`

Expected: exits `0` with all expected artifacts present

Commit:

```bash
git add scripts/ci/build-kata-distributions.sh .github/workflows/ci.yml .github/workflows/cli-release.yml .github/workflows/desktop-release.yml .github/workflows/orc-release.yml
git commit -m "ci: validate and publish Kata distribution artifacts"
```

---

## Final Validation

- [ ] **Step 1: Run the CLI-focused test suite**

Run:

```bash
pnpm --dir apps/cli exec vitest run \
  src/tests/domain/service.vitest.test.ts \
  src/tests/domain/adapters.vitest.test.ts \
  src/tests/setup.vitest.test.ts
```

Expected: all tests pass

- [ ] **Step 2: Run the Desktop-focused test suite**

Run:

```bash
pnpm --dir apps/desktop exec vitest run \
  src/main/__tests__/kata-backend-client.test.ts \
  src/main/__tests__/workflow-config-reader.test.ts \
  src/main/__tests__/workflow-board-service.test.ts \
  src/main/__tests__/pi-runtime-resolver.test.ts \
  src/main/__tests__/pi-agent-bridge.test.ts
```

Expected: all tests pass

- [ ] **Step 3: Run package-level typecheck and build commands**

Run:

```bash
pnpm --dir apps/cli exec tsc --noEmit
pnpm --dir apps/desktop exec tsc --noEmit
pnpm --dir apps/orchestrator test tests/build-skill-bundle.test.js
bash scripts/ci/build-kata-distributions.sh
```

Expected: all commands succeed

- [ ] **Step 4: Run affected monorepo validation**

Run:

```bash
pnpm exec turbo run lint typecheck test --affected
```

Expected: clean affected validation

- [ ] **Step 5: Commit the final docs updates**

```bash
git add apps/cli/README.md apps/orchestrator/README.md apps/desktop/README.md docs/superpowers/specs/2026-04-26-kata-cli-skill-platform-design.md docs/superpowers/plans/2026-04-26-kata-cli-skill-platform.md
git commit -m "docs: finalize Kata skill platform rollout guidance"
```
