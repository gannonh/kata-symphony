# KAT-255 TypeScript Scaffold Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bootstrap a Node 22 + TypeScript service scaffold with TDD, baseline lint/typecheck/test scripts, and a no-op daemon entrypoint that unblocks KAT-221.

**Architecture:** Implement a single-package TypeScript service with explicit layer folders (`config`, `tracker`, `orchestrator`, `execution`, `observability`) but no runtime orchestration behavior yet. Validate scaffold integrity through filesystem/package-script tests and a bootstrap process test. Keep implementation minimal and behavior-free beyond startup smoke requirements.

**Tech Stack:** Node 22, pnpm, TypeScript, Vitest, ESLint, tsx

---

Related skills during execution: `@symphony-implement-core`, `@symphony-verify-conformance`.

### Task 1: Initialize package/tooling skeleton via TDD

**Files:**
- Create: `tests/scaffold/package-scripts.test.ts`
- Create: `package.json`
- Create: `tsconfig.json`
- Create: `vitest.config.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

describe('package scripts', () => {
  it('defines required scripts', () => {
    const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
    expect(pkg.scripts).toMatchObject({
      lint: expect.any(String),
      typecheck: expect.any(String),
      test: expect.any(String),
      start: expect.any(String),
      dev: expect.any(String),
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/scaffold/package-scripts.test.ts`
Expected: FAIL with file-not-found for `package.json`.

**Step 3: Write minimal implementation**

- Add `package.json` with `pnpm` scripts and dependencies (`typescript`, `vitest`, `tsx`, `eslint`, `@types/node`).
- Add strict `tsconfig.json`.
- Add minimal `vitest.config.ts`.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest tests/scaffold/package-scripts.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add package.json tsconfig.json vitest.config.ts tests/scaffold/package-scripts.test.ts
git commit -m "test: add package script contract for ts scaffold"
```

### Task 2: Add scaffold directory contract test and folders

**Files:**
- Create: `tests/scaffold/structure.test.ts`
- Create: `src/config/.gitkeep`
- Create: `src/tracker/.gitkeep`
- Create: `src/orchestrator/.gitkeep`
- Create: `src/execution/.gitkeep`
- Create: `src/observability/.gitkeep`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

const required = [
  'src/config',
  'src/tracker',
  'src/orchestrator',
  'src/execution',
  'src/observability',
]

describe('scaffold layout', () => {
  it('contains required layer directories', () => {
    for (const dir of required) {
      expect(fs.existsSync(dir), `${dir} missing`).toBe(true)
    }
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/scaffold/structure.test.ts`
Expected: FAIL with missing directories.

**Step 3: Write minimal implementation**

Create required `src/*` directories with `.gitkeep` placeholders.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest tests/scaffold/structure.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src tests/scaffold/structure.test.ts
git commit -m "test: enforce initial service layer structure"
```

### Task 3: Add bootstrap process behavior test and entrypoint

**Files:**
- Create: `tests/bootstrap/startup.test.ts`
- Create: `src/main.ts`
- Modify: `package.json`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'

const START_MESSAGE = 'Symphony bootstrap ok'

describe('startup command', () => {
  it('prints bootstrap message and exits 0', () => {
    const result = spawnSync('pnpm', ['start'], { encoding: 'utf8' })
    expect(result.status).toBe(0)
    expect(result.stdout).toContain(START_MESSAGE)
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/bootstrap/startup.test.ts`
Expected: FAIL because `pnpm start` command or `src/main.ts` does not exist yet.

**Step 3: Write minimal implementation**

- Add `src/main.ts` with one log line: `Symphony bootstrap ok`.
- Wire `start` script (e.g., `tsx src/main.ts`) and `dev` script in `package.json`.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest tests/bootstrap/startup.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/main.ts package.json tests/bootstrap/startup.test.ts
git commit -m "feat: add no-op daemon bootstrap entrypoint"
```

### Task 4: Add lint baseline via contract test

**Files:**
- Create: `tests/scaffold/lint-config.test.ts`
- Create: `eslint.config.js`
- Modify: `package.json`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

describe('lint baseline', () => {
  it('has eslint config and lint script uses eslint', () => {
    expect(fs.existsSync('eslint.config.js')).toBe(true)
    const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
    expect(pkg.scripts.lint).toContain('eslint')
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/scaffold/lint-config.test.ts`
Expected: FAIL because ESLint config is missing.

**Step 3: Write minimal implementation**

- Add minimal `eslint.config.js` for TS files.
- Ensure `lint` script runs ESLint across `src` and `tests`.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest tests/scaffold/lint-config.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add eslint.config.js package.json tests/scaffold/lint-config.test.ts
git commit -m "chore: add eslint baseline for ts scaffold"
```

### Task 5: Add README commands and docs sync test

**Files:**
- Create: `tests/scaffold/readme-commands.test.ts`
- Modify: `README.md`
- Modify: `PLANS.md`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

const requiredSnippets = [
  'pnpm install',
  'pnpm run lint',
  'pnpm run typecheck',
  'pnpm test',
  'pnpm start',
]

describe('readme setup commands', () => {
  it('documents required pnpm commands', () => {
    const readme = fs.readFileSync('README.md', 'utf8')
    for (const snippet of requiredSnippets) {
      expect(readme).toContain(snippet)
    }
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/scaffold/readme-commands.test.ts`
Expected: FAIL because required command snippets are missing.

**Step 3: Write minimal implementation**

- Update `README.md` with `pnpm` install and script usage.
- Update `PLANS.md` to note KAT-255 scaffold in progress.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest tests/scaffold/readme-commands.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add README.md PLANS.md tests/scaffold/readme-commands.test.ts
git commit -m "docs: add pnpm scaffold workflow documentation"
```

### Task 6: Full verification and conformance evidence capture

**Files:**
- Modify: `docs/generated/kat-255-verification.md`

**Step 1: Run full checks**

Run: `make check`
Expected: PASS.

**Step 2: Run TypeScript toolchain checks**

Run: `pnpm run lint && pnpm run typecheck && pnpm test`
Expected: all PASS.

**Step 3: Run startup smoke**

Run: `pnpm start`
Expected: output includes `Symphony bootstrap ok`, process exits `0`.

**Step 4: Record evidence**

Write command outcomes and SPEC mapping (`3.2`, `3.3`, `17.7`, `18.1`) to `docs/generated/kat-255-verification.md`.

**Step 5: Commit**

```bash
git add docs/generated/kat-255-verification.md
git commit -m "docs: record KAT-255 scaffold verification evidence"
```

### Task 7: PR preparation

**Files:**
- Modify: `docs/generated/kat-255-pr-summary.md`

**Step 1: Draft PR summary**

Include:
- scope in/out
- key files added
- test evidence
- residual risks
- unblock note for `KAT-221` and `KAT-252`

**Step 2: Final sanity check**

Run: `make check`
Expected: PASS.

**Step 3: Commit**

```bash
git add docs/generated/kat-255-pr-summary.md
git commit -m "docs: add KAT-255 PR handoff summary"
```
