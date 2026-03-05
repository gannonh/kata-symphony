# KAT-224 Strict Prompt Template Rendering Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement strict prompt template rendering for `issue`/`attempt` with typed parse/render errors, explicit empty-template fallback, and retry/continuation-safe semantics.

**Architecture:** Add a dedicated execution-layer prompt module that wraps a Liquid-compatible engine in strict mode and exposes a typed result contract. Keep rendering deterministic and side-effect free so worker attempts can fail fast on prompt errors without changing orchestrator logic. Validate behavior with focused unit tests and minimal contract tests to keep the surface stable for downstream worker pipeline integration.

**Tech Stack:** TypeScript (NodeNext), Vitest, Liquid-compatible templating library (`liquidjs`), ESLint, existing `make check` harness.

---

### Task 1: Add Prompt Rendering Contracts

**Files:**
- Create: `src/execution/prompt/contracts.ts`
- Create: `src/execution/prompt/index.ts`
- Modify: `src/execution/contracts.ts`
- Test: `tests/contracts/execution-prompt-contracts.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'

import {
  PROMPT_ERROR_KINDS,
  type PromptBuildError,
  type PromptBuildResult,
} from '../../src/execution/prompt/contracts.js'

describe('execution prompt contracts', () => {
  it('exposes typed prompt result and error contracts', () => {
    const result: PromptBuildResult = { ok: true, prompt: 'hi' }
    const error: PromptBuildError = {
      kind: 'template_render_error',
      message: 'unknown variable: issue.missing',
    }

    expect(result.ok).toBe(true)
    expect(PROMPT_ERROR_KINDS).toEqual([
      'template_parse_error',
      'template_render_error',
    ])
    expect(error.kind).toBe('template_render_error')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/contracts/execution-prompt-contracts.test.ts`
Expected: FAIL with module-not-found/type export errors for prompt contracts.

**Step 3: Write minimal implementation**

```ts
// src/execution/prompt/contracts.ts
import type { Issue } from '../../domain/models.js'

export const PROMPT_ERROR_KINDS = [
  'template_parse_error',
  'template_render_error',
] as const

export type PromptErrorKind = (typeof PROMPT_ERROR_KINDS)[number]

export interface PromptBuildInput {
  template: string
  issue: Issue
  attempt: number | null
}

export interface PromptBuildError {
  kind: PromptErrorKind
  message: string
  template_excerpt?: string
  cause?: unknown
}

export type PromptBuildResult =
  | { ok: true; prompt: string }
  | { ok: false; error: PromptBuildError }

export interface PromptBuilder {
  build(input: PromptBuildInput): Promise<PromptBuildResult>
}
```

```ts
// src/execution/prompt/index.ts
export * from './contracts.js'
```

```ts
// src/execution/contracts.ts
import type {
  PromptBuildInput,
  PromptBuildResult,
  PromptBuilder,
} from './prompt/contracts.js'

export type { PromptBuildInput, PromptBuildResult, PromptBuilder }
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/contracts/execution-prompt-contracts.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add src/execution/contracts.ts src/execution/prompt/contracts.ts src/execution/prompt/index.ts tests/contracts/execution-prompt-contracts.test.ts
git commit -m "feat(execution): add prompt rendering contracts"
```

### Task 2: Add Prompt Builder Success + Empty Fallback Behavior

**Files:**
- Create: `src/execution/prompt/build-prompt.ts`
- Modify: `src/execution/prompt/index.ts`
- Test: `tests/execution/prompt/build-prompt.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'

import { createPromptBuilder } from '../../../src/execution/prompt/build-prompt.js'
import type { Issue } from '../../../src/domain/models.js'

const issue: Issue = {
  id: '1',
  identifier: 'KAT-224',
  title: 'Strict prompt rendering',
  description: 'desc',
  priority: 2,
  state: 'In Progress',
  branch_name: 'feature/kat-224',
  url: 'https://linear.app/kata-sh/issue/KAT-224',
  labels: ['area:symphony'],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('createPromptBuilder success path', () => {
  it('renders issue and attempt fields', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: 'Issue {{ issue.identifier }} attempt={{ attempt }}',
      issue,
      attempt: 1,
    })

    expect(result).toEqual({
      ok: true,
      prompt: 'Issue KAT-224 attempt=1',
    })
  })

  it('uses explicit default prompt when template is empty', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '   ',
      issue,
      attempt: null,
    })

    expect(result).toEqual({
      ok: true,
      prompt: 'You are working on an issue from Linear.',
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/prompt/build-prompt.test.ts`
Expected: FAIL because `createPromptBuilder` does not exist.

**Step 3: Write minimal implementation**

```ts
// src/execution/prompt/build-prompt.ts
import { Liquid } from 'liquidjs'

import type { PromptBuilder } from './contracts.js'

const DEFAULT_PROMPT = 'You are working on an issue from Linear.'

export function createPromptBuilder(): PromptBuilder {
  const engine = new Liquid({
    strictVariables: true,
    strictFilters: true,
  })

  return {
    async build(input) {
      const template = input.template.trim() === '' ? DEFAULT_PROMPT : input.template
      const prompt = await engine.parseAndRender(template, {
        issue: input.issue,
        attempt: input.attempt,
      })

      return { ok: true, prompt }
    },
  }
}
```

```ts
// src/execution/prompt/index.ts
export * from './build-prompt.js'
export * from './contracts.js'
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/prompt/build-prompt.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add src/execution/prompt/build-prompt.ts src/execution/prompt/index.ts tests/execution/prompt/build-prompt.test.ts
git commit -m "feat(execution): add strict prompt builder success and fallback behavior"
```

### Task 3: Add Strict Unknown Variable/Filter and Parse Error Mapping

**Files:**
- Modify: `src/execution/prompt/build-prompt.ts`
- Test: `tests/execution/prompt/build-prompt-errors.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'

import { createPromptBuilder } from '../../../src/execution/prompt/build-prompt.js'
import type { Issue } from '../../../src/domain/models.js'

const issue = {
  id: '1',
  identifier: 'KAT-224',
  title: 'Strict prompt rendering',
  description: null,
  priority: null,
  state: 'Todo',
  branch_name: null,
  url: null,
  labels: [],
  blocked_by: [],
  created_at: null,
  updated_at: null,
} satisfies Issue

describe('createPromptBuilder error mapping', () => {
  it('returns template_render_error on unknown variable', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '{{ issue.missing_field }}',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_render_error')
    }
  })

  it('returns template_render_error on unknown filter', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: "{{ issue.identifier | no_such_filter }}",
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_render_error')
    }
  })

  it('returns template_parse_error on invalid syntax', async () => {
    const builder = createPromptBuilder()

    const result = await builder.build({
      template: '{{ issue.identifier ',
      issue,
      attempt: null,
    })

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('template_parse_error')
    }
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/prompt/build-prompt-errors.test.ts`
Expected: FAIL because builder currently throws instead of returning typed errors.

**Step 3: Write minimal implementation**

```ts
// src/execution/prompt/build-prompt.ts (core change)
function classifyPromptError(error: unknown): 'template_parse_error' | 'template_render_error' {
  const name = error instanceof Error ? error.name : ''
  const message = error instanceof Error ? error.message : String(error)

  if (name.includes('Parse') || /parse|token|syntax/i.test(message)) {
    return 'template_parse_error'
  }

  return 'template_render_error'
}

// inside build(input)
try {
  const prompt = await engine.parseAndRender(template, {
    issue: input.issue,
    attempt: input.attempt,
  })
  return { ok: true, prompt }
} catch (error) {
  return {
    ok: false,
    error: {
      kind: classifyPromptError(error),
      message: error instanceof Error ? error.message : String(error),
      template_excerpt: template.slice(0, 200),
      cause: error,
    },
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/prompt/build-prompt-errors.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add src/execution/prompt/build-prompt.ts tests/execution/prompt/build-prompt-errors.test.ts
git commit -m "feat(execution): map strict template failures to typed prompt errors"
```

### Task 4: Verify Iteration Semantics for `labels`/`blocked_by` and `attempt`

**Files:**
- Modify: `tests/execution/prompt/build-prompt.test.ts`

**Step 1: Write the failing test**

```ts
it('preserves nested arrays/maps for Liquid iteration', async () => {
  const builder = createPromptBuilder()

  const result = await builder.build({
    template:
      '{% for label in issue.labels %}[{{ label }}]{% endfor %}|{% for b in issue.blocked_by %}{{ b.identifier }}{% endfor %}|{{ attempt }}',
    issue: {
      ...issue,
      labels: ['area:symphony', 'type:feature'],
      blocked_by: [{ id: '2', identifier: 'KAT-221', state: 'Done' }],
    },
    attempt: 2,
  })

  expect(result).toEqual({
    ok: true,
    prompt: '[area:symphony][type:feature]|KAT-221|2',
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/execution/prompt/build-prompt.test.ts`
Expected: FAIL if context normalization is incorrect.

**Step 3: Write minimal implementation**

```ts
// src/execution/prompt/build-prompt.ts
// Ensure context is passed as plain object values with no mutation.
const context = {
  issue: { ...input.issue },
  attempt: input.attempt,
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/execution/prompt/build-prompt.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/execution/prompt/build-prompt.test.ts src/execution/prompt/build-prompt.ts
git commit -m "test(execution): cover prompt iteration and continuation semantics"
```

### Task 5: Wire Dependency, Run Full Checks, and Document Contract Notes

**Files:**
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `docs/design-docs/2026-03-05-kat-224-strict-prompt-template-rendering-design.md`
- Optional note: `docs/generated/kat-224-pr-summary.md`

**Step 1: Write the failing test/check**

Run: `pnpm install --frozen-lockfile`
Expected: FAIL if `liquidjs` dependency is not declared.

**Step 2: Add minimal dependency implementation**

```json
{
  "dependencies": {
    "liquidjs": "^10.19.1"
  }
}
```

**Step 3: Run verification suite**

Run:
- `pnpm run lint`
- `pnpm run typecheck`
- `pnpm test`
- `make check`

Expected: All PASS.

**Step 4: Update docs note with implementation evidence**

Add a short “Implementation Notes” section to the design doc linking key files/tests added.

**Step 5: Commit**

```bash
git add package.json pnpm-lock.yaml src/execution/prompt tests/execution/prompt tests/contracts/execution-prompt-contracts.test.ts docs/design-docs/2026-03-05-kat-224-strict-prompt-template-rendering-design.md
git commit -m "feat(execution): implement strict prompt template rendering contract"
```

## Final Verification and Handoff

1. Confirm acceptance criteria coverage explicitly:
   - Empty-body fallback explicit and safe.
   - Unknown variable/filter failures surfaced as typed prompt errors.
   - First-attempt and retry/continuation rendering semantics covered by tests.
2. Capture command output summary in PR/issue handoff.
3. Keep follow-up items (if any) out of this scope unless they are blocker-level defects.
