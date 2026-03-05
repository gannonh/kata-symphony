# KAT-222 WORKFLOW.md Discovery and Parser Contract Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement deterministic `WORKFLOW.md` path discovery and YAML front matter parsing that returns a `WorkflowDefinition` plus typed, testable errors matching `SPEC.md` Sections 5.1-5.2 and 5.5.

**Architecture:** Add a dedicated `src/workflow/` module with a small public API (`loadWorkflowDefinition`) and explicit error constructors. Keep path resolution, file reading, and parsing pure/testable through injectable options. Use focused Vitest unit tests to lock down precedence rules, parse/body split behavior, and error discriminants before implementation.

**Tech Stack:** TypeScript (Node 22), Vitest, pnpm, `yaml` package for front matter decode.

---

**Skill refs for execution:** `@test-driven-development`, `@verification-before-completion`

### Task 1: Scaffold workflow contracts and public module exports

**Files:**
- Create: `src/workflow/contracts.ts`
- Create: `src/workflow/errors.ts`
- Create: `src/workflow/loader.ts`
- Create: `src/workflow/index.ts`
- Modify: `src/domain/index.ts`
- Test: `tests/workflow/workflow-module-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import {
  createMissingWorkflowFileError,
  createWorkflowParseError,
  createWorkflowFrontMatterNotAMapError,
  type WorkflowLoaderErrorCode,
} from '../../src/workflow/index.js'

describe('workflow module contracts', () => {
  it('exports typed error constructors with stable codes', () => {
    const missing = createMissingWorkflowFileError('/repo/WORKFLOW.md')
    const parse = createWorkflowParseError('/repo/WORKFLOW.md', 'invalid yaml')
    const nonMap = createWorkflowFrontMatterNotAMapError('/repo/WORKFLOW.md')

    const codes: WorkflowLoaderErrorCode[] = [missing.code, parse.code, nonMap.code]
    expect(codes).toEqual([
      'missing_workflow_file',
      'workflow_parse_error',
      'workflow_front_matter_not_a_map',
    ])
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/workflow/workflow-module-contracts.test.ts`
Expected: FAIL with module export/type errors (workflow module not implemented yet).

**Step 3: Write minimal implementation**

```ts
// src/workflow/contracts.ts
import type { WorkflowDefinition } from '../domain/models.js'

export type WorkflowLoaderErrorCode =
  | 'missing_workflow_file'
  | 'workflow_parse_error'
  | 'workflow_front_matter_not_a_map'

export interface WorkflowLoaderError extends Error {
  code: WorkflowLoaderErrorCode
  workflowPath: string
}

export interface LoadWorkflowDefinitionOptions {
  workflowPath?: string
  cwd?: string
  readFile?: (path: string) => Promise<string>
}

export type LoadWorkflowDefinition = (
  options?: LoadWorkflowDefinitionOptions,
) => Promise<WorkflowDefinition>
```

```ts
// src/workflow/errors.ts
import type { WorkflowLoaderError } from './contracts.js'

function withCode(
  code: WorkflowLoaderError['code'],
  workflowPath: string,
  message: string,
): WorkflowLoaderError {
  const error = new Error(message) as WorkflowLoaderError
  error.code = code
  error.workflowPath = workflowPath
  return error
}

export function createMissingWorkflowFileError(workflowPath: string): WorkflowLoaderError {
  return withCode('missing_workflow_file', workflowPath, `Workflow file not found: ${workflowPath}`)
}

export function createWorkflowParseError(workflowPath: string, detail: string): WorkflowLoaderError {
  return withCode('workflow_parse_error', workflowPath, `Failed to parse workflow file: ${detail}`)
}

export function createWorkflowFrontMatterNotAMapError(workflowPath: string): WorkflowLoaderError {
  return withCode(
    'workflow_front_matter_not_a_map',
    workflowPath,
    'Workflow front matter root must be a YAML map/object',
  )
}
```

```ts
// src/workflow/loader.ts
import type { LoadWorkflowDefinition } from './contracts.js'

export const loadWorkflowDefinition: LoadWorkflowDefinition = async () => {
  throw new Error('not implemented')
}
```

```ts
// src/workflow/index.ts
export { loadWorkflowDefinition } from './loader.js'
export type {
  LoadWorkflowDefinition,
  LoadWorkflowDefinitionOptions,
  WorkflowLoaderError,
  WorkflowLoaderErrorCode,
} from './contracts.js'
export {
  createMissingWorkflowFileError,
  createWorkflowFrontMatterNotAMapError,
  createWorkflowParseError,
} from './errors.js'
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/workflow/workflow-module-contracts.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/workflow/workflow-module-contracts.test.ts src/workflow/contracts.ts src/workflow/errors.ts src/workflow/loader.ts src/workflow/index.ts src/domain/index.ts
git commit -m "feat(workflow): scaffold loader contracts and typed errors"
```

### Task 2: Lock down path precedence and missing-file error behavior

**Files:**
- Modify: `tests/workflow/workflow-loader.test.ts`
- Modify: `src/workflow/loader.ts`

**Step 1: Write the failing test**

```ts
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
      readFile: async (path) => {
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
        readFile: async () => {
          throw new Error('ENOENT')
        },
      }),
    ).rejects.toMatchObject({
      code: 'missing_workflow_file',
      workflowPath: '/repo/WORKFLOW.md',
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/workflow/workflow-loader.test.ts -t "path precedence"`
Expected: FAIL because loader currently throws `not implemented`.

**Step 3: Write minimal implementation**

```ts
// src/workflow/loader.ts
import path from 'node:path'
import { createMissingWorkflowFileError } from './errors.js'
import type { LoadWorkflowDefinition } from './contracts.js'

const defaultReadFile = async (filePath: string): Promise<string> => {
  const { readFile } = await import('node:fs/promises')
  return readFile(filePath, 'utf8')
}

export const loadWorkflowDefinition: LoadWorkflowDefinition = async (options = {}) => {
  const cwd = options.cwd ?? process.cwd()
  const selectedPath =
    options.workflowPath && options.workflowPath.trim().length > 0
      ? options.workflowPath
      : 'WORKFLOW.md'
  const resolvedPath = path.resolve(cwd, selectedPath)

  const readFile = options.readFile ?? defaultReadFile
  let raw = ''
  try {
    raw = await readFile(resolvedPath)
  } catch {
    throw createMissingWorkflowFileError(resolvedPath)
  }

  return {
    config: {},
    prompt_template: raw.trim(),
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/workflow/workflow-loader.test.ts -t "path precedence"`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/workflow/workflow-loader.test.ts src/workflow/loader.ts
git commit -m "feat(workflow): implement file path precedence and missing-file error"
```

### Task 3: Add front matter split and prompt body trim behavior

**Files:**
- Modify: `tests/workflow/workflow-loader.test.ts`
- Modify: `src/workflow/loader.ts`

**Step 1: Write the failing test**

```ts
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
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/workflow/workflow-loader.test.ts -t "front matter"`
Expected: FAIL because loader currently always returns empty config.

**Step 3: Write minimal implementation**

Install parser dependency first:

Run: `pnpm add yaml`
Expected: `yaml` added to `dependencies` and lockfile updated.

```ts
import { parse as parseYaml } from 'yaml'
import { createWorkflowParseError } from './errors.js'

function splitFrontMatter(raw: string): { yamlText: string | null; body: string } {
  if (!raw.startsWith('---')) {
    return { yamlText: null, body: raw }
  }

  const lines = raw.split('\n')
  if (lines[0].trim() !== '---') {
    return { yamlText: null, body: raw }
  }

  let closingIndex = -1
  for (let i = 1; i < lines.length; i += 1) {
    if (lines[i].trim() === '---') {
      closingIndex = i
      break
    }
  }

  if (closingIndex < 0) {
    throw createWorkflowParseError('<path-set-by-caller>', 'missing closing front matter delimiter')
  }

  const yamlText = lines.slice(1, closingIndex).join('\n')
  const body = lines.slice(closingIndex + 1).join('\n')
  return { yamlText, body }
}

// In loadWorkflowDefinition:
const split = splitFrontMatter(raw)
let config: Record<string, unknown> = {}
if (split.yamlText !== null) {
  try {
    const decoded = parseYaml(split.yamlText)
    config = (decoded ?? {}) as Record<string, unknown>
  } catch (error) {
    throw createWorkflowParseError(resolvedPath, (error as Error).message)
  }
}

return {
  config,
  prompt_template: split.body.trim(),
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/workflow/workflow-loader.test.ts -t "front matter"`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/workflow/workflow-loader.test.ts src/workflow/loader.ts package.json pnpm-lock.yaml
git commit -m "feat(workflow): parse yaml front matter and prompt body"
```

### Task 4: Enforce parse error and non-map front matter error contract

**Files:**
- Modify: `tests/workflow/workflow-loader.test.ts`
- Modify: `src/workflow/loader.ts`

**Step 1: Write the failing test**

```ts
it('returns workflow_parse_error for invalid yaml front matter', async () => {
  await expect(
    loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => `---
tracker: [unclosed
---
Prompt`,
    }),
  ).rejects.toMatchObject({ code: 'workflow_parse_error' })
})

it('returns workflow_parse_error for missing closing delimiter', async () => {
  await expect(
    loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => `---
tracker:
  kind: linear
Prompt without close`,
    }),
  ).rejects.toMatchObject({ code: 'workflow_parse_error' })
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
  ).rejects.toMatchObject({ code: 'workflow_front_matter_not_a_map' })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/workflow/workflow-loader.test.ts -t "workflow_front_matter_not_a_map|workflow_parse_error"`
Expected: FAIL because current implementation may coerce non-map roots.

**Step 3: Write minimal implementation**

```ts
import { createWorkflowFrontMatterNotAMapError } from './errors.js'

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

// In loadWorkflowDefinition YAML branch:
let decoded: unknown
try {
  decoded = parseYaml(split.yamlText)
} catch (error) {
  throw createWorkflowParseError(resolvedPath, (error as Error).message)
}

if (!isPlainObject(decoded)) {
  throw createWorkflowFrontMatterNotAMapError(resolvedPath)
}

config = decoded
```

Also ensure `splitFrontMatter` receives `resolvedPath` so missing-delimiter errors use real path context.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/workflow/workflow-loader.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/workflow/workflow-loader.test.ts src/workflow/loader.ts
git commit -m "feat(workflow): enforce deterministic parse and non-map yaml errors"
```

### Task 5: Add end-to-end contract coverage and docs touchpoint

**Files:**
- Modify: `tests/contracts/layer-contracts.test.ts`
- Modify: `tests/contracts/runtime-modules.test.ts`
- Modify: `docs/plans/2026-03-05-kat-222-workflow-discovery-parser-design.md` (status notes only, if needed)

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { loadWorkflowDefinition } from '../../src/workflow/index.js'

describe('workflow layer contract integration', () => {
  it('returns WorkflowDefinition shape expected by domain contract', async () => {
    const definition = await loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => 'Hello prompt',
    })

    expect(definition).toEqual({
      config: {},
      prompt_template: 'Hello prompt',
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts`
Expected: FAIL until imports/coverage are wired for the new workflow layer.

**Step 3: Write minimal implementation**

Update contract/runtime module tests to include workflow module presence and expected export names.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts tests/workflow/*.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts docs/plans/2026-03-05-kat-222-workflow-discovery-parser-design.md
git commit -m "test(workflow): add layer-level contract coverage for workflow loader"
```

### Task 6: Full verification gate before handoff

**Files:**
- Modify: `docs/generated/kat-222-pr-summary.md` (if your PR workflow requires it)

**Step 1: Run focused unit and contract tests**

Run: `pnpm vitest run tests/workflow tests/contracts`
Expected: PASS.

**Step 2: Run repository quality gates**

Run: `pnpm run lint && pnpm run typecheck && pnpm test`
Expected: PASS.

**Step 3: Run harness gate**

Run: `make check`
Expected: PASS.

**Step 4: Capture evidence for Linear/PR**

Record:
- Commands executed
- Pass/fail results
- Acceptance criteria mapping:
  - path precedence
  - prompt body trim/split
  - deterministic typed errors

**Step 5: Commit evidence docs (if produced)**

```bash
git add docs/generated/kat-222-pr-summary.md
git commit -m "docs: add KAT-222 verification evidence"
```
