---
type: Plan
title: Kata Cli Phase A Real Backend
description: Archived implementation plan: Kata Cli Phase A Real Backend.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Phase A Real Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Phase A of the Kata CLI skill platform: Pi can run the core Kata workflow chain end-to-end against a real GitHub Projects v2 backend, with all durable artifact and state IO owned by `@kata-sh/cli`.

**Architecture:** The CLI owns the typed domain contract, GitHub Projects v2 adapter, artifact storage, and lifecycle transitions. Skills stay portable and progressively disclosed: `SKILL.md` is the compact orchestration layer, while workflow, alignment, setup, and runtime contract details live in bundled references. Phase A deliberately targets Pi + GitHub Projects v2 only; later phases handle backend parity, harness expansion, Desktop hardening, Symphony validation, and the formal e2e/eval framework.

**Tech Stack:** TypeScript, Node.js 20 fetch, GitHub REST API, GitHub GraphQL Projects v2 API, pnpm, Vitest, Bun test, Agent Skills `SKILL.md`, Pi coding agent

**Spec:** `docs/superpowers/specs/2026-04-27-kata-cli-skill-platform-realignment-design.md`

---

## Scope Check

The design document covers the full multi-phase platform, but this plan implements only Phase A.

Phase A acceptance chain:

```text
kata-setup
kata-new-project
kata-new-milestone
kata-plan-phase
kata-execute-phase
kata-verify-work
kata-complete-milestone
kata-new-milestone
kata-plan-phase
```

Phase A proof must run through Pi against a real GitHub Projects v2 backend. Unit tests may use fake HTTP clients, but the acceptance evidence may not rely on mocks, local fallback stores, or JSON-only transport checks.

## Current Status

This section is the authoritative progress tracker. The detailed task checkboxes below remain as the original implementation checklist.

| Area | Status | Evidence |
|---|---|---|
| Task 1: Phase A domain contract | Done, committed | `cc9a8c7e`, `3af2b2bc` |
| Task 2: CLI operation runner | Done, committed | `4a3fb4c5`, `c3740cd2`, `e1a3d748` |
| Task 3: GitHub client and project fields | Done, committed | `ac0be71d`, `f95f4c6a`, `54f5c74d` |
| Task 4: GitHub artifact IO | Done, committed | `9ff3c779`, `9586dc0b` |
| Task 5: Real GitHub Projects v2 backend adapter | Done, committed | `81b3fb1b`, `b6c90f9f`, `e9e06d0f` |
| Task 6: Progressive-disclosure skill source | Done, committed | `780ecee0`; source is `apps/cli/skills-src` |
| Task 7: Phase A workflow references | Done, committed | `780ecee0`; generated bundle has exactly nine Phase A skills |
| Task 8: Pi setup and doctor readiness | Implemented to current Phase A scope, committed | `780ecee0`; doctor still warns instead of live-validating GitHub Project v2 fields |
| Task 9: Manual Phase A runbook | In progress | Active runbook restored at `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md` |
| Task 10: CI guards | Done, committed | `780ecee0`; `bash scripts/ci/build-kata-distributions.sh` passed locally |
| Task 11: Full local verification | Done | `pnpm run validate:affected` passed after the Desktop snapshot adapter fix |
| Task 12: Manual Pi acceptance | In progress | `setup --pi`, `doctor`, and live `health.check` passed; interactive Pi skill chain and evidence URLs still pending |

## File Map

### Create

| File | Responsibility |
|---|---|
| `apps/cli/src/domain/operations.ts` | Operation names, input/output typing, and validation helpers for CLI-callable contract operations |
| `apps/cli/src/backends/github-projects-v2/client.ts` | Thin GitHub REST/GraphQL HTTP client using `fetch` and `GITHUB_TOKEN` |
| `apps/cli/src/backends/github-projects-v2/project-fields.ts` | Project field discovery, creation, and option normalization for Kata-owned fields |
| `apps/cli/src/backends/github-projects-v2/artifacts.ts` | GitHub issue-comment artifact read/write/list implementation |
| `apps/cli/src/commands/call.ts` | Workflow-friendly CLI operation runner (`kata call <operation> --input <file>`) |
| `apps/cli/src/tests/phase-a-contract.vitest.test.ts` | Contract operation tests for Phase A primitives and lifecycle operations |
| `apps/cli/src/tests/github-projects-v2.client.vitest.test.ts` | HTTP client request/response tests using injected fetch |
| `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts` | Adapter behavior tests using fake HTTP fixtures |
| `apps/cli/skills-src/references/alignment.md` | Shared fast/guided/deep alignment pattern |
| `apps/cli/skills-src/scripts/kata-call.mjs` | Thin skill helper script that delegates to `@kata-sh/cli call` |
| `apps/cli/skills-src/workflows/setup.md` | Portable setup skill workflow |
| `apps/cli/skills-src/workflows/new-project.md` | Portable Phase A new-project workflow |
| `apps/cli/skills-src/workflows/new-milestone.md` | Portable Phase A new-milestone workflow |
| `apps/cli/skills-src/workflows/plan-phase.md` | Portable Phase A planning workflow |
| `apps/cli/skills-src/workflows/execute-phase.md` | Portable Phase A execution workflow |
| `apps/cli/skills-src/workflows/verify-work.md` | Portable Phase A verification workflow |
| `apps/cli/skills-src/workflows/complete-milestone.md` | Portable Phase A milestone completion workflow |
| `apps/cli/skills-src/workflows/progress.md` | Portable Phase A progress workflow |
| `apps/cli/skills-src/workflows/health.md` | Portable Phase A health workflow |
| `apps/cli/src/tests/phase-a-skill-surface.vitest.test.ts` | Ensures shipped Phase A skills exclude standalone discuss commands and legacy workflow paths |

### Modify

| File | Change |
|---|---|
| `apps/cli/src/domain/types.ts` | Add Phase A create/update/list/complete input types and operation output types |
| `apps/cli/src/domain/service.ts` | Add project, milestone, slice, task, artifact, health, and execution methods needed by Phase A |
| `apps/cli/src/transports/json.ts` | Route all Phase A operations through a single operation dispatcher |
| `apps/cli/src/cli.ts` | Add `kata call`, keep `kata json` as compatibility/debug transport |
| `apps/cli/src/backends/resolve-backend.ts` | Remove production local fallback and construct the real GitHub adapter by default |
| `apps/cli/src/backends/github-projects-v2/adapter.ts` | Replace injected-test-only adapter with real GitHub Projects v2 implementation |
| `apps/cli/src/commands/doctor.ts` | Validate token, repo, project, and required Project v2 fields for GitHub mode |
| `apps/cli/scripts/bundle-skills.mjs` | Generate progressive-disclosure skills from `skills-src/workflows`, shared alignment, runtime contract, and scripts |
| `apps/cli/skills-src/manifest.json` | Expose only the nine Phase A skills |
| `apps/cli/src/tests/build-skill-bundle.vitest.test.ts` | Assert generated skills include alignment/workflow/runtime references and copied scripts |
| `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md` | Replace runbook with real Pi + GitHub Projects v2 Phase A acceptance steps |
| `scripts/ci/build-kata-distributions.sh` | Validate Phase A skill surface and CLI contract unit tests; do not claim real backend acceptance in CI |

## GitHub Projects v2 Phase A Mapping

Phase A uses real GitHub objects:

| Kata primitive | GitHub storage |
|---|---|
| `Project` | Repository + configured GitHub Projects v2 project |
| `Milestone` | GitHub milestone plus Project v2 item issue with `Kata Type = Milestone` |
| `Slice` | GitHub issue added to Project v2 with `Kata Type = Slice` |
| `Task` | GitHub issue added to Project v2 with `Kata Type = Task` and `Kata Parent ID = <slice id>` |
| `Artifact` | GitHub issue comment on the owning milestone/slice/task issue, or project tracking issue for project-scoped artifacts |

Required Project v2 fields:

```text
Status
Kata Type
Kata ID
Kata Parent ID
Kata Artifact Scope
```

Required Status options:

```text
Backlog
Todo
In Progress
Agent Review
Human Review
Merging
Done
```

Artifact comment marker:

```markdown
<!-- kata:artifact {"scopeType":"slice","scopeId":"S001","artifactType":"plan"} -->
```

GitHub API references:

- GitHub GraphQL mutations include `addProjectV2ItemById`, `updateProjectV2ItemFieldValue`, and `createProjectV2Field`.
- `updateProjectV2ItemFieldValue` supports single-select, text, number, date, and iteration fields.

Reference: `https://docs.github.com/en/graphql/reference/mutations`

## Task 1: Expand the Phase A Domain Contract

**Files:**
- Modify: `apps/cli/src/domain/types.ts`
- Modify: `apps/cli/src/domain/service.ts`
- Create: `apps/cli/src/domain/operations.ts`
- Create: `apps/cli/src/tests/phase-a-contract.vitest.test.ts`

- [ ] **Step 1: Write the failing Phase A contract test**

Create `apps/cli/src/tests/phase-a-contract.vitest.test.ts`:

```typescript
import { describe, expect, it, vi } from "vitest";
import { createKataDomainApi } from "../domain/service.js";
import { KATA_OPERATION_NAMES, dispatchKataOperation } from "../domain/operations.js";
import type { KataBackendAdapter } from "../domain/types.js";

function createAdapter(): KataBackendAdapter {
  return {
    getProjectContext: vi.fn(async () => ({
      backend: "github",
      workspacePath: "/repo",
      repository: { owner: "kata-sh", name: "uat" },
    })),
    upsertProject: vi.fn(async (input) => ({
      backend: "github",
      workspacePath: "/repo",
      repository: { owner: "kata-sh", name: "uat" },
      title: input.title,
      description: input.description,
    })),
    listMilestones: vi.fn(async () => []),
    getActiveMilestone: vi.fn(async () => null),
    createMilestone: vi.fn(async (input) => ({
      id: "M001",
      title: input.title,
      goal: input.goal,
      status: "active",
      active: true,
    })),
    completeMilestone: vi.fn(async (input) => ({
      id: input.milestoneId,
      title: "M001",
      goal: "Ship v1",
      status: "done",
      active: false,
    })),
    listSlices: vi.fn(async () => []),
    createSlice: vi.fn(async (input) => ({
      id: "S001",
      milestoneId: input.milestoneId,
      title: input.title,
      goal: input.goal,
      status: "todo",
      order: input.order ?? 0,
    })),
    updateSliceStatus: vi.fn(async (input) => ({
      id: input.sliceId,
      milestoneId: "M001",
      title: "Slice",
      goal: "Slice goal",
      status: input.status,
      order: 0,
    })),
    listTasks: vi.fn(async () => []),
    createTask: vi.fn(async (input) => ({
      id: "T001",
      sliceId: input.sliceId,
      title: input.title,
      description: input.description,
      status: "todo",
      verificationState: "pending",
    })),
    updateTaskStatus: vi.fn(async (input) => ({
      id: input.taskId,
      sliceId: "S001",
      title: "Task",
      description: "Task description",
      status: input.status,
      verificationState: input.verificationState ?? "pending",
    })),
    listArtifacts: vi.fn(async () => []),
    readArtifact: vi.fn(async () => null),
    writeArtifact: vi.fn(async (input) => ({
      id: `${input.scopeType}:${input.scopeId}:${input.artifactType}`,
      ...input,
      updatedAt: "2026-04-28T00:00:00.000Z",
      provenance: { backend: "github", backendId: "comment:1" },
    })),
    openPullRequest: vi.fn(async () => ({
      id: "PR1",
      url: "https://github.com/kata-sh/uat/pull/1",
      branch: "feature",
      base: "main",
      status: "open",
      mergeReady: false,
    })),
    getExecutionStatus: vi.fn(async () => ({ queueDepth: 0, activeWorkers: 0, escalations: [] })),
    checkHealth: vi.fn(async () => ({
      ok: true,
      backend: "github",
      checks: [{ name: "github-project", status: "ok", message: "Project found" }],
    })),
  };
}

describe("Phase A domain contract", () => {
  it("exposes every operation needed by the acceptance chain", () => {
    expect(KATA_OPERATION_NAMES).toEqual([
      "project.getContext",
      "project.upsert",
      "milestone.list",
      "milestone.getActive",
      "milestone.create",
      "milestone.complete",
      "slice.list",
      "slice.create",
      "slice.updateStatus",
      "task.list",
      "task.create",
      "task.updateStatus",
      "artifact.list",
      "artifact.read",
      "artifact.write",
      "execution.getStatus",
      "health.check",
    ]);
  });

  it("dispatches lifecycle operations through the adapter", async () => {
    const api = createKataDomainApi(createAdapter());

    await expect(dispatchKataOperation(api, "project.upsert", {
      title: "Todo UAT",
      description: "Phase A validation app",
    })).resolves.toMatchObject({ title: "Todo UAT" });

    await expect(dispatchKataOperation(api, "milestone.create", {
      title: "M001",
      goal: "Build first todo app milestone",
    })).resolves.toMatchObject({ id: "M001", active: true });

    await expect(dispatchKataOperation(api, "slice.create", {
      milestoneId: "M001",
      title: "S001 Todo CRUD",
      goal: "Create basic todo CRUD",
    })).resolves.toMatchObject({ id: "S001", status: "todo" });

    await expect(dispatchKataOperation(api, "task.create", {
      sliceId: "S001",
      title: "T001 Create todo form",
      description: "Implement the form",
    })).resolves.toMatchObject({ id: "T001", verificationState: "pending" });
  });
});
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-contract.vitest.test.ts
```

Expected: fail because `domain/operations.js`, lifecycle adapter methods, and API methods do not exist.

- [ ] **Step 3: Extend `apps/cli/src/domain/types.ts`**

Add these exports after `KataProjectContext` and before `KataBackendAdapter`:

```typescript
export interface KataProjectUpsertInput {
  title: string;
  description: string;
}

export interface KataMilestoneCreateInput {
  title: string;
  goal: string;
}

export interface KataMilestoneCompleteInput {
  milestoneId: string;
  summary: string;
}

export interface KataSliceCreateInput {
  milestoneId: string;
  title: string;
  goal: string;
  order?: number;
}

export interface KataSliceUpdateStatusInput {
  sliceId: string;
  status: KataSlice["status"];
}

export interface KataTaskCreateInput {
  sliceId: string;
  title: string;
  description: string;
}

export interface KataTaskUpdateStatusInput {
  taskId: string;
  status: KataTask["status"];
  verificationState?: KataTask["verificationState"];
}

export interface KataHealthCheck {
  name: string;
  status: "ok" | "warn" | "invalid";
  message: string;
}

export interface KataHealthReport {
  ok: boolean;
  backend: KataBackendKind;
  checks: KataHealthCheck[];
}
```

Update `KataProjectContext` so it can carry project metadata:

```typescript
export interface KataProjectContext {
  backend: KataBackendKind;
  workspacePath: string;
  repository?: {
    owner: string;
    name: string;
  };
  title?: string;
  description?: string;
}
```

Add these methods to `KataBackendAdapter`:

```typescript
  upsertProject(input: KataProjectUpsertInput): Promise<KataProjectContext>;
  listMilestones(): Promise<KataMilestone[]>;
  createMilestone(input: KataMilestoneCreateInput): Promise<KataMilestone>;
  completeMilestone(input: KataMilestoneCompleteInput): Promise<KataMilestone>;
  createSlice(input: KataSliceCreateInput): Promise<KataSlice>;
  updateSliceStatus(input: KataSliceUpdateStatusInput): Promise<KataSlice>;
  createTask(input: KataTaskCreateInput): Promise<KataTask>;
  updateTaskStatus(input: KataTaskUpdateStatusInput): Promise<KataTask>;
  checkHealth(): Promise<KataHealthReport>;
```

- [ ] **Step 4: Extend `apps/cli/src/domain/service.ts`**

Replace the return object in `createKataDomainApi` with:

```typescript
  return {
    project: {
      getContext: () => adapter.getProjectContext(),
      upsert: (input: KataProjectUpsertInput) => adapter.upsertProject(input),
    },
    milestone: {
      list: () => adapter.listMilestones(),
      getActive: () => adapter.getActiveMilestone(),
      create: (input: KataMilestoneCreateInput) => adapter.createMilestone(input),
      complete: (input: KataMilestoneCompleteInput) => adapter.completeMilestone(input),
    },
    slice: {
      list: (input: KataSliceListInput) => adapter.listSlices(input),
      create: (input: KataSliceCreateInput) => adapter.createSlice(input),
      updateStatus: (input: KataSliceUpdateStatusInput) => adapter.updateSliceStatus(input),
    },
    task: {
      list: (input: KataTaskListInput) => adapter.listTasks(input),
      create: (input: KataTaskCreateInput) => adapter.createTask(input),
      updateStatus: (input: KataTaskUpdateStatusInput) => adapter.updateTaskStatus(input),
    },
    artifact: {
      list: (input: KataArtifactListInput) => adapter.listArtifacts(input),
      read: (input: KataArtifactReadInput) => adapter.readArtifact(input),
      write: (input: KataArtifactWriteInput) => adapter.writeArtifact(input),
    },
    pr: {
      open: (input: KataOpenPullRequestInput) => adapter.openPullRequest(input),
    },
    execution: {
      getStatus: () => adapter.getExecutionStatus(),
    },
    health: {
      check: () => adapter.checkHealth(),
    },
  };
```

Add the imported input types at the top of the file.

- [ ] **Step 5: Create `apps/cli/src/domain/operations.ts`**

```typescript
import type { createKataDomainApi } from "./service.js";

export const KATA_OPERATION_NAMES = [
  "project.getContext",
  "project.upsert",
  "milestone.list",
  "milestone.getActive",
  "milestone.create",
  "milestone.complete",
  "slice.list",
  "slice.create",
  "slice.updateStatus",
  "task.list",
  "task.create",
  "task.updateStatus",
  "artifact.list",
  "artifact.read",
  "artifact.write",
  "execution.getStatus",
  "health.check",
] as const;

export type KataOperationName = (typeof KATA_OPERATION_NAMES)[number];
export type KataDomainApi = ReturnType<typeof createKataDomainApi>;

export function isKataOperationName(operation: string): operation is KataOperationName {
  return KATA_OPERATION_NAMES.includes(operation as KataOperationName);
}

export async function dispatchKataOperation(
  api: KataDomainApi,
  operation: KataOperationName,
  payload: Record<string, unknown> = {},
): Promise<unknown> {
  switch (operation) {
    case "project.getContext":
      return api.project.getContext();
    case "project.upsert":
      return api.project.upsert(payload as never);
    case "milestone.list":
      return api.milestone.list();
    case "milestone.getActive":
      return api.milestone.getActive();
    case "milestone.create":
      return api.milestone.create(payload as never);
    case "milestone.complete":
      return api.milestone.complete(payload as never);
    case "slice.list":
      return api.slice.list(payload as never);
    case "slice.create":
      return api.slice.create(payload as never);
    case "slice.updateStatus":
      return api.slice.updateStatus(payload as never);
    case "task.list":
      return api.task.list(payload as never);
    case "task.create":
      return api.task.create(payload as never);
    case "task.updateStatus":
      return api.task.updateStatus(payload as never);
    case "artifact.list":
      return api.artifact.list(payload as never);
    case "artifact.read":
      return api.artifact.read(payload as never);
    case "artifact.write":
      return api.artifact.write(payload as never);
    case "execution.getStatus":
      return api.execution.getStatus();
    case "health.check":
      return api.health.check();
  }
}
```

- [ ] **Step 6: Run the Phase A contract test**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-contract.vitest.test.ts
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add apps/cli/src/domain apps/cli/src/tests/phase-a-contract.vitest.test.ts
git commit -m "feat(cli): expand phase a domain contract"
```

## Task 2: Add Workflow-Friendly CLI Operation Runner

**Files:**
- Create: `apps/cli/src/commands/call.ts`
- Modify: `apps/cli/src/cli.ts`
- Modify: `apps/cli/src/transports/json.ts`
- Test: `apps/cli/src/tests/phase-a-contract.vitest.test.ts`

- [ ] **Step 1: Extend the test for operation dispatch through transport**

Append to `apps/cli/src/tests/phase-a-contract.vitest.test.ts`:

```typescript
import { runJsonCommand } from "../transports/json.js";

describe("Phase A operation transport", () => {
  it("routes new lifecycle operations through runJsonCommand", async () => {
    const api = createKataDomainApi(createAdapter());
    const result = await runJsonCommand({
      operation: "milestone.create",
      payload: { title: "M001", goal: "Ship first milestone" },
    }, api);

    expect(JSON.parse(result)).toMatchObject({
      ok: true,
      data: { id: "M001", active: true },
    });
  });
});
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-contract.vitest.test.ts
```

Expected: fail because `runJsonCommand` does not route `milestone.create`.

- [ ] **Step 3: Replace operation routing in `apps/cli/src/transports/json.ts`**

Replace the operation list and branch chain with:

```typescript
import {
  KATA_OPERATION_NAMES,
  dispatchKataOperation,
  isKataOperationName,
  type KataDomainApi,
} from "../domain/operations.js";

type JsonPayload = Record<string, unknown>;

interface JsonCommandRequest {
  operation: string;
  payload?: JsonPayload;
}

export const SUPPORTED_JSON_OPERATIONS = KATA_OPERATION_NAMES;

export function isSupportedJsonOperation(operation: string) {
  return isKataOperationName(operation);
}

export async function runJsonCommand(input: JsonCommandRequest, api: KataDomainApi) {
  if (!isKataOperationName(input.operation)) {
    return JSON.stringify({
      ok: false,
      error: { code: "UNKNOWN", message: `Unsupported operation: ${input.operation}` },
    });
  }

  const data = await dispatchKataOperation(api, input.operation, input.payload ?? {});
  return JSON.stringify({ ok: true, data });
}
```

- [ ] **Step 4: Create `apps/cli/src/commands/call.ts`**

```typescript
import { readFile } from "node:fs/promises";
import { createKataDomainApi } from "../domain/service.js";
import { isKataOperationName, dispatchKataOperation } from "../domain/operations.js";
import { resolveBackend } from "../backends/resolve-backend.js";

export interface RunCallInput {
  operation: string;
  inputPath?: string;
  cwd: string;
}

export async function runCall(input: RunCallInput): Promise<string> {
  if (!isKataOperationName(input.operation)) {
    return JSON.stringify({
      ok: false,
      error: { code: "UNKNOWN", message: `Unsupported operation: ${input.operation}` },
    });
  }

  let payload: Record<string, unknown> = {};
  if (input.inputPath) {
    const raw = await readFile(input.inputPath, "utf8");
    const parsed = JSON.parse(raw) as unknown;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      payload = parsed as Record<string, unknown>;
    }
  }

  const adapter = await resolveBackend({ workspacePath: input.cwd });
  const data = await dispatchKataOperation(createKataDomainApi(adapter), input.operation, payload);
  return JSON.stringify({ ok: true, data }, null, 2);
}
```

- [ ] **Step 5: Wire `kata call` in `apps/cli/src/cli.ts`**

Add after the `doctor` branch:

```typescript
  if (command === "call") {
    const operation = rest[0];
    const inputFlagIndex = rest.findIndex((value) => value === "--input");
    const inputPath = inputFlagIndex >= 0 ? rest[inputFlagIndex + 1] : undefined;
    if (!operation) {
      writeJsonError("Missing operation. Usage: kata call <operation> --input <request.json>");
      return;
    }

    try {
      const { runCall } = await import("./commands/call.js");
      process.stdout.write(`${await runCall({ operation, inputPath, cwd: process.cwd() })}\n`);
    } catch (error) {
      process.stdout.write(`${JSON.stringify({ ok: false, error: toJsonRuntimeError(error) })}\n`);
    }
    return;
  }
```

Update usage text:

```typescript
    "  kata setup",
    "  kata doctor",
    "  kata call <operation> --input <request.json>",
    "  kata json <request.json>",
```

- [ ] **Step 6: Run the CLI tests**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-contract.vitest.test.ts
pnpm --dir apps/cli run typecheck
```

Expected: both pass.

- [ ] **Step 7: Commit**

```bash
git add apps/cli/src/commands/call.ts apps/cli/src/cli.ts apps/cli/src/transports/json.ts apps/cli/src/tests/phase-a-contract.vitest.test.ts
git commit -m "feat(cli): add typed operation runner"
```

## Task 3: Implement Real GitHub HTTP Client and Project Field Manager

**Files:**
- Create: `apps/cli/src/backends/github-projects-v2/client.ts`
- Create: `apps/cli/src/backends/github-projects-v2/project-fields.ts`
- Create: `apps/cli/src/tests/github-projects-v2.client.vitest.test.ts`

- [ ] **Step 1: Write the failing HTTP client test**

Create `apps/cli/src/tests/github-projects-v2.client.vitest.test.ts`:

```typescript
import { describe, expect, it, vi } from "vitest";
import { createGithubClient } from "../backends/github-projects-v2/client.js";

describe("createGithubClient", () => {
  it("sends authenticated GraphQL requests", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({
      data: { viewer: { login: "gannonhall" } },
    }), { status: 200 }));

    const client = createGithubClient({ token: "ghp_test", fetch });
    const result = await client.graphql<{ viewer: { login: string } }>({
      query: "query { viewer { login } }",
    });

    expect(result.viewer.login).toBe("gannonhall");
    expect(fetch).toHaveBeenCalledWith("https://api.github.com/graphql", expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({
        Authorization: "Bearer ghp_test",
      }),
    }));
  });

  it("sends REST requests to repository endpoints", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({ number: 1 }), { status: 201 }));
    const client = createGithubClient({ token: "ghp_test", fetch });

    const result = await client.rest<{ number: number }>({
      method: "POST",
      path: "/repos/kata-sh/uat/issues",
      body: { title: "Issue" },
    });

    expect(result.number).toBe(1);
    expect(fetch).toHaveBeenCalledWith("https://api.github.com/repos/kata-sh/uat/issues", expect.any(Object));
  });
});
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.client.vitest.test.ts
```

Expected: fail because `client.ts` does not exist.

- [ ] **Step 3: Create `apps/cli/src/backends/github-projects-v2/client.ts`**

```typescript
import { KataDomainError } from "../../domain/errors.js";

type FetchLike = typeof fetch;

export interface GithubClientInput {
  token: string;
  fetch?: FetchLike;
}

export interface GraphqlInput {
  query: string;
  variables?: Record<string, unknown>;
}

export interface RestInput {
  method: "GET" | "POST" | "PATCH" | "DELETE";
  path: string;
  body?: Record<string, unknown>;
}

export function createGithubClient(input: GithubClientInput) {
  const fetchImpl = input.fetch ?? fetch;

  async function parseResponse<T>(response: Response): Promise<T> {
    const text = await response.text();
    const parsed = text.length > 0 ? JSON.parse(text) as unknown : {};
    if (!response.ok) {
      throw new KataDomainError("NETWORK", `GitHub request failed (${response.status}): ${text}`);
    }
    return parsed as T;
  }

  return {
    async graphql<T>(request: GraphqlInput): Promise<T> {
      const response = await fetchImpl("https://api.github.com/graphql", {
        method: "POST",
        headers: {
          Authorization: `Bearer ${input.token}`,
          "Content-Type": "application/json",
          "User-Agent": "@kata-sh/cli",
        },
        body: JSON.stringify(request),
      });
      const parsed = await parseResponse<{ data?: T; errors?: Array<{ message: string }> }>(response);
      if (parsed.errors?.length) {
        throw new KataDomainError("UNKNOWN", parsed.errors.map((error) => error.message).join("; "));
      }
      if (!parsed.data) {
        throw new KataDomainError("UNKNOWN", "GitHub GraphQL response did not include data.");
      }
      return parsed.data;
    },

    async rest<T>(request: RestInput): Promise<T> {
      const response = await fetchImpl(`https://api.github.com${request.path}`, {
        method: request.method,
        headers: {
          Authorization: `Bearer ${input.token}`,
          Accept: "application/vnd.github+json",
          "Content-Type": "application/json",
          "X-GitHub-Api-Version": "2022-11-28",
          "User-Agent": "@kata-sh/cli",
        },
        body: request.body ? JSON.stringify(request.body) : undefined,
      });
      return parseResponse<T>(response);
    },
  };
}
```

- [ ] **Step 4: Create `apps/cli/src/backends/github-projects-v2/project-fields.ts`**

```typescript
import type { createGithubClient } from "./client.js";

export const KATA_PROJECT_FIELDS = {
  status: "Status",
  type: "Kata Type",
  id: "Kata ID",
  parentId: "Kata Parent ID",
  artifactScope: "Kata Artifact Scope",
} as const;

export const KATA_STATUS_OPTIONS = [
  "Backlog",
  "Todo",
  "In Progress",
  "Agent Review",
  "Human Review",
  "Merging",
  "Done",
] as const;

export interface ProjectFieldIndex {
  projectId: string;
  fields: Record<string, { id: string; options?: Record<string, string> }>;
}

export async function loadProjectFieldIndex(input: {
  client: ReturnType<typeof createGithubClient>;
  owner: string;
  repo: string;
  projectNumber: number;
}): Promise<ProjectFieldIndex> {
  const data = await input.client.graphql<any>({
    query: `
      query KataProjectFields($owner: String!, $repo: String!, $number: Int!) {
        repository(owner: $owner, name: $repo) {
          owner { login }
        }
        organization(login: $owner) {
          projectV2(number: $number) {
            id
            fields(first: 50) {
              nodes {
                ... on ProjectV2Field { id name }
                ... on ProjectV2SingleSelectField {
                  id
                  name
                  options { id name }
                }
              }
            }
          }
        }
        user(login: $owner) {
          projectV2(number: $number) {
            id
            fields(first: 50) {
              nodes {
                ... on ProjectV2Field { id name }
                ... on ProjectV2SingleSelectField {
                  id
                  name
                  options { id name }
                }
              }
            }
          }
        }
      }
    `,
    variables: { owner: input.owner, repo: input.repo, number: input.projectNumber },
  });

  const project = data.organization?.projectV2 ?? data.user?.projectV2;
  if (!project?.id) {
    throw new Error(`GitHub Projects v2 project #${input.projectNumber} was not found for ${input.owner}.`);
  }

  const fields: ProjectFieldIndex["fields"] = {};
  for (const field of project.fields.nodes ?? []) {
    if (!field?.name || !field.id) continue;
    fields[field.name] = {
      id: field.id,
      options: Object.fromEntries((field.options ?? []).map((option: any) => [option.name, option.id])),
    };
  }

  return { projectId: project.id, fields };
}
```

- [ ] **Step 5: Run the client test**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.client.vitest.test.ts
pnpm --dir apps/cli run typecheck
```

Expected: both pass.

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/backends/github-projects-v2/client.ts apps/cli/src/backends/github-projects-v2/project-fields.ts apps/cli/src/tests/github-projects-v2.client.vitest.test.ts
git commit -m "feat(cli): add github projects v2 client"
```

## Task 4: Implement GitHub Artifact IO on Issue Comments

**Files:**
- Create: `apps/cli/src/backends/github-projects-v2/artifacts.ts`
- Create/modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`

- [ ] **Step 1: Write the failing artifact test**

Create `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts` with:

```typescript
import { describe, expect, it, vi } from "vitest";
import {
  formatArtifactComment,
  parseArtifactComment,
  upsertArtifactComment,
} from "../backends/github-projects-v2/artifacts.js";

describe("GitHub artifact comments", () => {
  it("formats and parses artifact comments", () => {
    const comment = formatArtifactComment({
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "# Plan",
    });

    expect(parseArtifactComment(comment)).toEqual({
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "# Plan",
    });
  });

  it("updates an existing artifact comment instead of duplicating it", async () => {
    const client = {
      rest: vi.fn(async (request: any) => {
        if (request.method === "GET") {
          return [{
            id: 10,
            body: formatArtifactComment({
              scopeType: "slice",
              scopeId: "S001",
              artifactType: "plan",
              content: "old",
            }),
          }];
        }
        return { id: 10, body: request.body.body };
      }),
    };

    const result = await upsertArtifactComment({
      client: client as any,
      owner: "kata-sh",
      repo: "uat",
      issueNumber: 5,
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "new",
    });

    expect(result.backendId).toBe("comment:10");
    expect(client.rest).toHaveBeenCalledWith(expect.objectContaining({
      method: "PATCH",
      path: "/repos/kata-sh/uat/issues/comments/10",
    }));
  });
});
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
```

Expected: fail because `artifacts.ts` does not exist.

- [ ] **Step 3: Create `apps/cli/src/backends/github-projects-v2/artifacts.ts`**

```typescript
import type { KataArtifactType, KataScopeType } from "../../domain/types.js";
import type { createGithubClient } from "./client.js";

const MARKER_PREFIX = "<!-- kata:artifact ";
const MARKER_SUFFIX = " -->";

export interface ParsedArtifactComment {
  scopeType: KataScopeType;
  scopeId: string;
  artifactType: KataArtifactType;
  content: string;
}

export function formatArtifactComment(input: ParsedArtifactComment): string {
  const marker = JSON.stringify({
    scopeType: input.scopeType,
    scopeId: input.scopeId,
    artifactType: input.artifactType,
  });
  return `${MARKER_PREFIX}${marker}${MARKER_SUFFIX}\n${input.content}`;
}

export function parseArtifactComment(body: string): ParsedArtifactComment | null {
  if (!body.startsWith(MARKER_PREFIX)) return null;
  const markerEnd = body.indexOf(MARKER_SUFFIX);
  if (markerEnd < 0) return null;
  const metadata = JSON.parse(body.slice(MARKER_PREFIX.length, markerEnd)) as {
    scopeType: KataScopeType;
    scopeId: string;
    artifactType: KataArtifactType;
  };
  return {
    ...metadata,
    content: body.slice(markerEnd + MARKER_SUFFIX.length).replace(/^\r?\n/, ""),
  };
}

export async function upsertArtifactComment(input: {
  client: ReturnType<typeof createGithubClient>;
  owner: string;
  repo: string;
  issueNumber: number;
  scopeType: KataScopeType;
  scopeId: string;
  artifactType: KataArtifactType;
  content: string;
}): Promise<{ backendId: string; body: string }> {
  const comments = await input.client.rest<Array<{ id: number; body?: string }>>({
    method: "GET",
    path: `/repos/${input.owner}/${input.repo}/issues/${input.issueNumber}/comments`,
  });
  const body = formatArtifactComment(input);
  const existing = comments.find((comment) => {
    const parsed = parseArtifactComment(comment.body ?? "");
    return parsed?.scopeType === input.scopeType &&
      parsed.scopeId === input.scopeId &&
      parsed.artifactType === input.artifactType;
  });

  if (existing) {
    const updated = await input.client.rest<{ id: number; body: string }>({
      method: "PATCH",
      path: `/repos/${input.owner}/${input.repo}/issues/comments/${existing.id}`,
      body: { body },
    });
    return { backendId: `comment:${updated.id}`, body: updated.body };
  }

  const created = await input.client.rest<{ id: number; body: string }>({
    method: "POST",
    path: `/repos/${input.owner}/${input.repo}/issues/${input.issueNumber}/comments`,
    body: { body },
  });
  return { backendId: `comment:${created.id}`, body: created.body };
}
```

- [ ] **Step 4: Run the artifact test**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/backends/github-projects-v2/artifacts.ts apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts
git commit -m "feat(cli): store artifacts in github issue comments"
```

## Task 5: Replace Production GitHub Adapter Fallback With Real Backend IO

**Files:**
- Modify: `apps/cli/src/backends/github-projects-v2/adapter.ts`
- Modify: `apps/cli/src/backends/resolve-backend.ts`
- Modify: `apps/cli/src/commands/doctor.ts`
- Test: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`

- [ ] **Step 1: Add failing adapter behavior tests**

Append to `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`:

```typescript
import { GithubProjectsV2Adapter } from "../backends/github-projects-v2/adapter.js";

describe("GithubProjectsV2Adapter Phase A behavior", () => {
  it("creates milestones, slices, tasks, and artifacts through real GitHub client calls", async () => {
    const calls: Array<{ method: string; path: string }> = [];
    const client = {
      graphql: vi.fn(async () => ({
        repository: { id: "repo-id", owner: { login: "kata-sh" } },
        organization: {
          projectV2: {
            id: "project-id",
            fields: { nodes: [] },
          },
        },
      })),
      rest: vi.fn(async (request: any) => {
        calls.push({ method: request.method, path: request.path });
        if (request.path.endsWith("/milestones")) return { number: 1, title: request.body.title, description: request.body.description };
        if (request.path.endsWith("/issues")) return { id: 100, number: 5, title: request.body.title, body: request.body.body, state: "open" };
        if (request.path.endsWith("/comments")) return [];
        if (request.path.includes("/comments/")) return { id: 10, body: request.body.body };
        return { id: 10, body: request.body?.body ?? "" };
      }),
    };

    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 1,
      client: client as any,
      workspacePath: "/repo",
    });

    const milestone = await adapter.createMilestone({ title: "M001", goal: "First milestone" });
    const slice = await adapter.createSlice({ milestoneId: milestone.id, title: "S001 Todo CRUD", goal: "Build CRUD" });
    const task = await adapter.createTask({ sliceId: slice.id, title: "T001 Form", description: "Build the form" });
    const artifact = await adapter.writeArtifact({
      scopeType: "slice",
      scopeId: slice.id,
      artifactType: "plan",
      title: "S001 Plan",
      content: "Plan body",
      format: "markdown",
    });

    expect(milestone.id).toBe("M001");
    expect(slice.milestoneId).toBe("M001");
    expect(task.sliceId).toBe("S001");
    expect(artifact.provenance.backend).toBe("github");
    expect(calls.some((call) => call.method === "POST" && call.path === "/repos/kata-sh/uat/milestones")).toBe(true);
    expect(calls.some((call) => call.method === "POST" && call.path === "/repos/kata-sh/uat/issues")).toBe(true);
  });
});
```

- [ ] **Step 2: Run the adapter test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
```

Expected: fail because adapter constructor and methods still use the old injected snapshot shape.

- [ ] **Step 3: Replace `GithubProjectsV2Adapter` constructor shape**

In `apps/cli/src/backends/github-projects-v2/adapter.ts`, define:

```typescript
interface GithubProjectsV2AdapterInput {
  owner: string;
  repo: string;
  projectNumber: number;
  workspacePath: string;
  client: ReturnType<typeof createGithubClient>;
}
```

The adapter should keep these indexes:

```typescript
private issueByKataId = new Map<string, { issueNumber: number; nodeId: string }>();
private activeMilestoneId: string | null = null;
```

- [ ] **Step 4: Implement GitHub ID helpers in the adapter**

Add:

```typescript
function nextKataId(prefix: "M" | "S" | "T", existingCount: number): string {
  return `${prefix}${String(existingCount + 1).padStart(3, "0")}`;
}

function statusTitle(status: KataSlice["status"] | KataTask["status"]): string {
  const map = {
    backlog: "Backlog",
    todo: "Todo",
    in_progress: "In Progress",
    agent_review: "Agent Review",
    human_review: "Human Review",
    merging: "Merging",
    done: "Done",
  } as const;
  return map[status];
}
```

- [ ] **Step 5: Implement adapter helper methods**

Add these helper types and methods inside `GithubProjectsV2Adapter`:

```typescript
interface GithubIssueRecord {
  id: number;
  node_id: string;
  number: number;
  title: string;
  body?: string;
}

function formatEntityBody(input: {
  kataId: string;
  type: "Project" | "Milestone" | "Slice" | "Task";
  body: string;
  parentId?: string;
}): string {
  return `<!-- kata:entity ${JSON.stringify({
    kataId: input.kataId,
    type: input.type,
    ...(input.parentId ? { parentId: input.parentId } : {}),
  })} -->\n${input.body}`;
}

private async createIssue(input: {
  kataId: string;
  type: "Project" | "Milestone" | "Slice" | "Task";
  title: string;
  body: string;
  parentId?: string;
}): Promise<GithubIssueRecord> {
  const issue = await this.input.client.rest<GithubIssueRecord>({
    method: "POST",
    path: `/repos/${this.input.owner}/${this.input.repo}/issues`,
    body: {
      title: input.title,
      body: formatEntityBody(input),
    },
  });
  this.issueByKataId.set(input.kataId, { issueNumber: issue.number, nodeId: issue.node_id });
  return issue;
}

private async ensureProjectTrackingIssue(input: KataProjectUpsertInput): Promise<GithubIssueRecord> {
  const existing = this.issueByKataId.get("PROJECT");
  if (existing) {
    return { id: Number(existing.issueNumber), node_id: existing.nodeId, number: existing.issueNumber, title: input.title };
  }
  return this.createIssue({
    kataId: "PROJECT",
    type: "Project",
    title: `[KATA-PROJECT] ${input.title}`,
    body: input.description,
  });
}
```

- [ ] **Step 6: Implement project and milestone lifecycle methods**

Add these methods to `GithubProjectsV2Adapter`:

```typescript
async upsertProject(input: KataProjectUpsertInput): Promise<KataProjectContext> {
  const issue = await this.ensureProjectTrackingIssue(input);
  this.issueByKataId.set("PROJECT", { issueNumber: issue.number, nodeId: issue.node_id });
  await this.writeArtifact({
    scopeType: "project",
    scopeId: "PROJECT",
    artifactType: "project-brief",
    title: "Project Brief",
    content: input.description,
    format: "markdown",
  });
  return {
    backend: "github",
    workspacePath: this.input.workspacePath,
    repository: { owner: this.input.owner, name: this.input.repo },
    title: input.title,
    description: input.description,
  };
}

async listMilestones(): Promise<KataMilestone[]> {
  const milestones = await this.input.client.rest<Array<{ title: string; description?: string; state: string }>>({
    method: "GET",
    path: `/repos/${this.input.owner}/${this.input.repo}/milestones?state=all`,
  });
  return milestones
    .filter((milestone) => /^M\d{3}\b/.test(milestone.title))
    .map((milestone) => ({
      id: milestone.title.match(/^M\d{3}/)?.[0] ?? "M000",
      title: milestone.title,
      goal: milestone.description ?? "",
      status: milestone.state === "closed" ? "done" : "planned",
      active: milestone.state === "open",
    }));
}

async getActiveMilestone(): Promise<KataMilestone | null> {
  const milestones = await this.listMilestones();
  return milestones.find((milestone) => milestone.active) ?? null;
}

async createMilestone(input: KataMilestoneCreateInput): Promise<KataMilestone> {
  const existing = await this.listMilestones();
  const id = nextKataId("M", existing.length);
  const milestone = await this.input.client.rest<{ number: number; title: string; description?: string }>({
    method: "POST",
    path: `/repos/${this.input.owner}/${this.input.repo}/milestones`,
    body: { title: `${id} ${input.title}`, description: input.goal },
  });
  await this.createIssue({
    kataId: id,
    type: "Milestone",
    title: `${id} ${input.title}`,
    body: input.goal,
  });
  this.activeMilestoneId = id;
  return { id, title: milestone.title, goal: input.goal, status: "active", active: true };
}

async completeMilestone(input: KataMilestoneCompleteInput): Promise<KataMilestone> {
  await this.writeArtifact({
    scopeType: "milestone",
    scopeId: input.milestoneId,
    artifactType: "summary",
    title: `${input.milestoneId} Summary`,
    content: input.summary,
    format: "markdown",
  });
  this.activeMilestoneId = null;
  return { id: input.milestoneId, title: input.milestoneId, goal: input.summary, status: "done", active: false };
}
```

- [ ] **Step 7: Implement slice, task, and artifact methods**

Add these methods to `GithubProjectsV2Adapter`:

```typescript
async createSlice(input: KataSliceCreateInput): Promise<KataSlice> {
  const existing = await this.listSlices({ milestoneId: input.milestoneId });
  const id = nextKataId("S", existing.length);
  await this.createIssue({
    kataId: id,
    type: "Slice",
    title: `${id} ${input.title}`,
    body: input.goal,
    parentId: input.milestoneId,
  });
  return {
    id,
    milestoneId: input.milestoneId,
    title: input.title,
    goal: input.goal,
    status: "todo",
    order: input.order ?? existing.length,
  };
}

async createTask(input: KataTaskCreateInput): Promise<KataTask> {
  const existing = await this.listTasks({ sliceId: input.sliceId });
  const id = nextKataId("T", existing.length);
  await this.createIssue({
    kataId: id,
    type: "Task",
    title: `${id} ${input.title}`,
    body: input.description,
    parentId: input.sliceId,
  });
  return {
    id,
    sliceId: input.sliceId,
    title: input.title,
    description: input.description,
    status: "todo",
    verificationState: "pending",
  };
}

async updateSliceStatus(input: KataSliceUpdateStatusInput): Promise<KataSlice> {
  return {
    id: input.sliceId,
    milestoneId: this.activeMilestoneId ?? "M000",
    title: input.sliceId,
    goal: statusTitle(input.status),
    status: input.status,
    order: 0,
  };
}

async updateTaskStatus(input: KataTaskUpdateStatusInput): Promise<KataTask> {
  return {
    id: input.taskId,
    sliceId: "S000",
    title: input.taskId,
    description: statusTitle(input.status),
    status: input.status,
    verificationState: input.verificationState ?? "pending",
  };
}

async writeArtifact(input: KataArtifactWriteInput): Promise<KataArtifact> {
  const owner = this.input.owner;
  const repo = this.input.repo;
  const issue = this.issueByKataId.get(input.scopeId) ?? this.issueByKataId.get("PROJECT");
  if (!issue) {
    throw new KataDomainError("NOT_FOUND", `No GitHub issue is known for Kata scope ${input.scopeId}.`);
  }
  const stored = await upsertArtifactComment({
    client: this.input.client,
    owner,
    repo,
    issueNumber: issue.issueNumber,
    scopeType: input.scopeType,
    scopeId: input.scopeId,
    artifactType: input.artifactType,
    content: input.content,
  });
  return {
    id: `${input.scopeType}:${input.scopeId}:${input.artifactType}`,
    scopeType: input.scopeType,
    scopeId: input.scopeId,
    artifactType: input.artifactType,
    title: input.title,
    content: input.content,
    format: input.format,
    updatedAt: new Date().toISOString(),
    provenance: { backend: "github", backendId: stored.backendId },
  };
}
```

- [ ] **Step 8: Remove production local runtime fallback**

In `apps/cli/src/backends/resolve-backend.ts`, remove `createFileRuntimeBackendFactory` from the default GitHub path. For GitHub config, construct:

```typescript
if (config.kind === "github") {
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  if (!token) {
    throw new KataDomainError("UNAUTHORIZED", "GITHUB_TOKEN or GH_TOKEN is required for GitHub Projects v2 backend.");
  }
  return new GithubProjectsV2Adapter({
    owner: config.repoOwner,
    repo: config.repoName,
    projectNumber: config.githubProjectNumber,
    workspacePath: input.workspacePath,
    client: createGithubClient({ token }),
  });
}
```

Keep `runtimeBackendFactory` only inside tests that explicitly construct an adapter. Do not call it from production `resolveBackend`.

- [ ] **Step 9: Run tests and typecheck**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
pnpm --dir apps/cli run typecheck
```

Expected: both pass.

- [ ] **Step 10: Commit**

```bash
git add apps/cli/src/backends apps/cli/src/commands/doctor.ts apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts
git commit -m "feat(cli): use real github projects v2 backend"
```

## Task 6: Rebuild Skill Source Around Progressive Disclosure

**Files:**
- Modify: `apps/cli/skills-src/manifest.json`
- Modify: `apps/cli/scripts/bundle-skills.mjs`
- Create: `apps/cli/skills-src/references/alignment.md`
- Create: `apps/cli/skills-src/scripts/kata-call.mjs`
- Create: `apps/cli/src/tests/phase-a-skill-surface.vitest.test.ts`
- Create: `apps/cli/src/tests/build-skill-bundle.vitest.test.ts`

- [ ] **Step 1: Write the failing skill surface test**

Create `apps/cli/src/tests/phase-a-skill-surface.vitest.test.ts`:

```typescript
import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

const sourceRoot = path.resolve(process.cwd());

describe("Phase A skill surface", () => {
  test("manifest exposes primary workflows and no standalone discuss skills", () => {
    const manifest = JSON.parse(readFileSync(path.join(sourceRoot, "skills-src", "manifest.json"), "utf8"));
    const names = manifest.skills.map((skill) => skill.name).sort();

    expect(names).toEqual([
      "kata-complete-milestone",
      "kata-execute-phase",
      "kata-health",
      "kata-new-milestone",
      "kata-new-project",
      "kata-plan-phase",
      "kata-progress",
      "kata-setup",
      "kata-verify-work",
    ]);
  });

  test("workflow source files do not point at legacy orchestrator runtime paths", () => {
    const manifest = JSON.parse(readFileSync(path.join(sourceRoot, "skills-src", "manifest.json"), "utf8"));
    for (const skill of manifest.skills) {
      const workflow = readFileSync(path.join(sourceRoot, "skills-src", "workflows", `${skill.workflow}.md`), "utf8");
      expect(workflow).not.toContain("kata-tools.cjs");
      expect(workflow).not.toContain("~/.claude/kata-orchestrator");
      expect(workflow).not.toContain(".planning/");
      expect(workflow).not.toContain("/kata:discuss");
    }
  });
});
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-skill-surface.vitest.test.ts
```

Expected before implementation: fail if a standalone discussion skill exists or any Phase A workflow file is missing. Expected after Task 7: pass.

- [ ] **Step 3: Update `apps/cli/skills-src/manifest.json`**

Replace the `skills` array with entries for:

```json
[
  {
    "name": "kata-setup",
    "description": "Bootstrap Kata into Pi and verify @kata-sh/cli can reach the configured backend. Use this whenever the user asks to install Kata, set it up, configure Pi, or check whether Kata is ready.",
    "workflow": "setup",
    "runtimeRequired": false,
    "contractOperations": ["health.check"],
    "setupHint": "Run `npx @kata-sh/cli setup --pi`, then run `npx @kata-sh/cli doctor` from the project repository."
  },
  {
    "name": "kata-new-project",
    "description": "Initialize Kata project-level context and artifacts, then route to kata-new-milestone. Use this whenever the user is starting Kata for a repository or wants to define the project before milestone planning.",
    "workflow": "new-project",
    "contractOperations": ["project.upsert", "artifact.write", "health.check"],
    "setupHint": "Run `npx @kata-sh/cli setup --pi` once, then verify with `npx @kata-sh/cli doctor`."
  },
  {
    "name": "kata-new-milestone",
    "description": "Create the next Kata milestone and seed roadmap/requirements artifacts. Use this after kata-new-project, after completing a milestone, or whenever the user wants to start the next milestone cycle.",
    "workflow": "new-milestone",
    "contractOperations": ["project.getContext", "milestone.create", "artifact.write"],
    "setupHint": "Run `npx @kata-sh/cli doctor` before creating a milestone."
  },
  {
    "name": "kata-plan-phase",
    "description": "Plan the next vertical slice in the active milestone. Use this whenever the user asks Kata to plan work, create a slice, break down tasks, or prepare execution.",
    "workflow": "plan-phase",
    "contractOperations": ["project.getContext", "milestone.getActive", "slice.create", "task.create", "artifact.write"],
    "setupHint": "Run `npx @kata-sh/cli doctor` before planning."
  },
  {
    "name": "kata-execute-phase",
    "description": "Execute planned slice tasks through the active Kata backend state. Use this whenever the user wants Kata to carry out planned work.",
    "workflow": "execute-phase",
    "contractOperations": ["project.getContext", "milestone.getActive", "slice.list", "task.list", "task.updateStatus", "artifact.read", "artifact.write"],
    "setupHint": "Run `npx @kata-sh/cli doctor` before execution."
  },
  {
    "name": "kata-verify-work",
    "description": "Verify completed work and record UAT/verification artifacts. Use this whenever the user asks Kata to validate, test, or accept completed work.",
    "workflow": "verify-work",
    "contractOperations": ["project.getContext", "task.list", "task.updateStatus", "artifact.list", "artifact.read", "artifact.write"],
    "setupHint": "Run `npx @kata-sh/cli doctor` before verification."
  },
  {
    "name": "kata-complete-milestone",
    "description": "Complete the active milestone and record summary artifacts. Use this when the milestone is verified and the user wants to close it before starting the next one.",
    "workflow": "complete-milestone",
    "contractOperations": ["milestone.getActive", "milestone.complete", "artifact.write"],
    "setupHint": "Run `npx @kata-sh/cli doctor` before completing a milestone."
  },
  {
    "name": "kata-progress",
    "description": "Summarize current Kata project, milestone, slice, task, and execution state. Use this when the user asks where things stand or what to do next.",
    "workflow": "progress",
    "contractOperations": ["project.getContext", "milestone.getActive", "slice.list", "task.list", "artifact.list", "execution.getStatus"],
    "setupHint": "Run `npx @kata-sh/cli doctor` if progress cannot read backend state."
  },
  {
    "name": "kata-health",
    "description": "Run Kata health checks for Pi, CLI, configured GitHub Projects v2 backend, and required Project fields. Use this whenever setup or backend state seems questionable.",
    "workflow": "health",
    "contractOperations": ["health.check", "project.getContext"],
    "setupHint": "Run `npx @kata-sh/cli doctor` from the project repository."
  }
]
```

- [ ] **Step 4: Create shared alignment reference**

Create `apps/cli/skills-src/references/alignment.md`:

```markdown
# Integrated Alignment Pattern

Every primary Kata workflow begins by choosing an alignment depth.

## Depths

- `fast`: ask only for missing required inputs.
- `guided`: ask concise questions that reduce execution risk.
- `deep`: explore tradeoffs, constraints, acceptance criteria, and sequencing before writing backend state.

## Rules

1. Keep alignment inside the active workflow.
2. Do not route to standalone discuss commands.
3. Persist durable decisions through @kata-sh/cli artifact operations.
4. Prefer `guided` when the user does not specify a depth.
```

- [ ] **Step 5: Create thin CLI wrapper script**

Create `apps/cli/skills-src/scripts/kata-call.mjs`:

```javascript
#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";

const args = process.argv.slice(2);
const localCli = process.env.KATA_CLI_ROOT
  ? `${process.env.KATA_CLI_ROOT}/dist/loader.js`
  : null;

const command = localCli && existsSync(localCli)
  ? ["node", localCli]
  : ["npx", "--yes", "@kata-sh/cli"];

const result = spawnSync(command[0], [...command.slice(1), "call", ...args], {
  stdio: "inherit",
  env: process.env,
});

process.exit(result.status ?? 1);
```

- [ ] **Step 6: Update CLI build script to source new workflows and scripts**

In `apps/cli/scripts/bundle-skills.mjs`, read workflows from:

```javascript
path.join(cliRoot, "skills-src", "workflows", `${skill.workflow}.md`)
```

Copy shared references and scripts into each skill:

```javascript
await fs.copyFile(
  path.join(sourceRoot, "skills-src", "references", "alignment.md"),
  path.join(referencesDir, "alignment.md"),
);
await fs.cp(
  path.join(sourceRoot, "skills-src", "scripts"),
  path.join(skillDir, "scripts"),
  { recursive: true },
);
```

- [ ] **Step 7: Run skill tests**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-skill-surface.vitest.test.ts
pnpm --dir apps/cli exec vitest run src/tests/build-skill-bundle.vitest.test.ts
```

Expected: fail until the workflow files are created in Task 7.

- [ ] **Step 8: Commit manifest and generator changes after Task 7 passes**

Defer the commit until Task 7 creates workflow source files and tests pass.

## Task 7: Write Phase A Portable Workflow References

**Files:**
- Create: `apps/cli/skills-src/workflows/setup.md`
- Create: `apps/cli/skills-src/workflows/new-project.md`
- Create: `apps/cli/skills-src/workflows/new-milestone.md`
- Create: `apps/cli/skills-src/workflows/plan-phase.md`
- Create: `apps/cli/skills-src/workflows/execute-phase.md`
- Create: `apps/cli/skills-src/workflows/verify-work.md`
- Create: `apps/cli/skills-src/workflows/complete-milestone.md`
- Create: `apps/cli/skills-src/workflows/progress.md`
- Create: `apps/cli/skills-src/workflows/health.md`

- [ ] **Step 1: Create `setup.md`**

```markdown
# Setup Workflow

1. Confirm the user is running from the project repository.
2. Run `npx @kata-sh/cli setup --pi` if Pi skills are not installed.
3. Run `npx @kata-sh/cli doctor`.
4. If doctor reports invalid GitHub configuration, ask the user to fix `.kata/preferences.md`.
5. If doctor reports missing GitHub auth, ask for `GITHUB_TOKEN` or `GH_TOKEN`.
6. Continue only after backend health is valid.
```

- [ ] **Step 2: Create `new-project.md`**

```markdown
# New Project Workflow

Use integrated alignment before writing state.

## Alignment Overlay

- `fast`: ask for project name and one-sentence outcome.
- `guided`: ask for project name, outcome, target users, constraints, and acceptance signal.
- `deep`: additionally explore risks, non-goals, and first milestone candidates.

## Runtime Flow

1. Run health check through `scripts/kata-call.mjs health.check`.
2. Create project context with `project.upsert`.
3. Write project-level artifacts:
   - `project-brief`
   - `requirements`
4. Do not create a milestone.
5. End by telling the user the next step is `kata-new-milestone`.

## Backend IO

All durable writes use `scripts/kata-call.mjs`.
```

- [ ] **Step 3: Create `new-milestone.md`**

```markdown
# New Milestone Workflow

Use integrated alignment before creating the milestone.

## Alignment Overlay

- `fast`: ask for milestone title and goal.
- `guided`: ask for title, goal, success criteria, likely slices, and constraints.
- `deep`: additionally inspect project artifacts and discuss sequencing tradeoffs.

## Runtime Flow

1. Read project context with `project.getContext`.
2. Create the milestone with `milestone.create`.
3. Write milestone artifacts:
   - `requirements`
   - `roadmap`
4. Present the first planning prompt: run `kata-plan-phase`.
```

- [ ] **Step 4: Create `plan-phase.md`**

```markdown
# Plan Phase Workflow

Use integrated alignment before creating slice/task state.

## Alignment Overlay

- `fast`: plan the next obvious vertical slice.
- `guided`: confirm slice outcome, user-visible behavior, test evidence, and task boundaries.
- `deep`: compare alternate slices and choose the best next vertical increment.

## Runtime Flow

1. Read project context with `project.getContext`.
2. Read active milestone with `milestone.getActive`.
3. Create one vertical slice with `slice.create`.
4. Create executable tasks with `task.create`.
5. Write slice artifacts:
   - `phase-context`
   - `plan`
   - `verification`
6. End by telling the user the next step is `kata-execute-phase`.
```

- [ ] **Step 5: Create `execute-phase.md`**

```markdown
# Execute Phase Workflow

Use integrated alignment before mutating task status.

## Alignment Overlay

- `fast`: execute the next todo task.
- `guided`: confirm task order and verification command before editing code.
- `deep`: inspect risks and dependencies before executing.

## Runtime Flow

1. Read active milestone with `milestone.getActive`.
2. List slices with `slice.list`.
3. List tasks with `task.list`.
4. For each selected task:
   - mark `in_progress` with `task.updateStatus`
   - perform the code work in the repository
   - run verification commands
   - mark `done` or leave `in_progress` with failure evidence
5. Write execution artifacts:
   - `summary`
   - `verification`
6. End by telling the user the next step is `kata-verify-work`.
```

- [ ] **Step 6: Create `verify-work.md`**

```markdown
# Verify Work Workflow

Use integrated alignment to choose verification depth.

## Alignment Overlay

- `fast`: run the known verification command and summarize result.
- `guided`: run tests, inspect relevant app behavior, and record pass/fail evidence.
- `deep`: perform exploratory UAT and capture gaps as follow-up tasks.

## Runtime Flow

1. Read project context with `project.getContext`.
2. List tasks with `task.list`.
3. Read verification artifacts with `artifact.read`.
4. Run verification/UAT.
5. Write `uat` artifact.
6. Update task verification state with `task.updateStatus`.
7. If the milestone is complete, route to `kata-complete-milestone`.
```

- [ ] **Step 7: Create `complete-milestone.md`**

```markdown
# Complete Milestone Workflow

Use integrated alignment before closing the milestone.

## Alignment Overlay

- `fast`: confirm the active milestone should close.
- `guided`: summarize delivered slices, verification evidence, and remaining risks.
- `deep`: include retrospective notes and next milestone candidates.

## Runtime Flow

1. Read active milestone with `milestone.getActive`.
2. Write milestone `summary` and `retrospective` artifacts.
3. Complete the milestone with `milestone.complete`.
4. End by telling the user the next step is `kata-new-milestone`.
```

- [ ] **Step 8: Create `progress.md` and `health.md`**

`progress.md`:

```markdown
# Progress Workflow

1. Read project context with `project.getContext`.
2. Read active milestone with `milestone.getActive`.
3. List slices with `slice.list`.
4. List tasks for each slice with `task.list`.
5. Read execution status with `execution.getStatus`.
6. Summarize current state and recommend the next primary workflow.
```

`health.md`:

```markdown
# Health Workflow

1. Run `health.check`.
2. Explain invalid checks with concrete fix commands.
3. Confirm Pi skills and CLI backend access are ready before workflow execution.
```

- [ ] **Step 9: Build skills and run tests**

Run:

```bash
pnpm --dir apps/cli run build
pnpm --dir apps/cli exec vitest run src/tests/phase-a-skill-surface.vitest.test.ts
pnpm --dir apps/cli exec vitest run src/tests/build-skill-bundle.vitest.test.ts
```

Expected: all pass.

- [ ] **Step 10: Build CLI bundle with generated skills**

Run:

```bash
pnpm --dir apps/cli run build
```

Expected: pass and `apps/cli/skills` contains the nine Phase A skills.

- [ ] **Step 11: Commit**

```bash
git add apps/cli/skills-src apps/cli/scripts/bundle-skills.mjs apps/cli/src/tests apps/cli/skills
git commit -m "feat(skills): define phase a portable workflows"
```

## Task 8: Strengthen Pi Setup and Doctor for Real Backend Readiness

**Files:**
- Modify: `apps/cli/src/commands/setup.ts`
- Modify: `apps/cli/src/commands/doctor.ts`
- Modify: `apps/cli/src/tests/setup-source.vitest.test.ts`

- [ ] **Step 1: Add a failing doctor test for missing token**

Append to `apps/cli/src/tests/setup-source.vitest.test.ts`:

```typescript
import { runDoctor } from "../commands/doctor.js";

describe("GitHub backend readiness", () => {
  it("reports invalid when GitHub mode lacks token", async () => {
    const cwd = makeTempWorkspace({
      preferences: `---
workflow:
  mode: github
github:
  repoOwner: kata-sh
  repoName: uat
  stateMode: projects_v2
  githubProjectNumber: 1
---`,
    });

    const report = await runDoctor({
      cwd,
      env: { HOME: process.env.HOME },
      packageVersion: "0.0.0-test",
    });

    expect(report.status).toBe("invalid");
    expect(report.checks.some((check) => check.name === "github-auth" && check.status === "invalid")).toBe(true);
  });
});
```

If `makeTempWorkspace` is not exported in the existing test file, define it in that file with `mkdtempSync`, `mkdirSync`, and `writeFileSync`.

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/setup-source.vitest.test.ts
```

Expected: fail because doctor does not validate GitHub token readiness.

- [ ] **Step 3: Add GitHub auth/project checks in `doctor.ts`**

After backend config parses as GitHub, add:

```typescript
const token = env.GITHUB_TOKEN || env.GH_TOKEN;
checks.push({
  name: "github-auth",
  status: token ? "ok" : "invalid",
  message: token ? "GitHub token is present" : "GITHUB_TOKEN or GH_TOKEN is required for GitHub Projects v2 backend",
  ...(token ? {} : { action: "Export GITHUB_TOKEN with repo and project access before running Kata workflows." }),
});
```

If `token` exists, instantiate the GitHub adapter and call `checkHealth()`:

```typescript
const adapter = await resolveBackend({ workspacePath: cwd });
const health = await adapter.checkHealth();
for (const check of health.checks) {
  checks.push({ name: `backend-${check.name}`, status: check.status, message: check.message });
}
```

- [ ] **Step 4: Run setup/doctor tests**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/setup-source.vitest.test.ts
pnpm --dir apps/cli run typecheck
```

Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/commands/setup.ts apps/cli/src/commands/doctor.ts apps/cli/src/tests/setup-source.vitest.test.ts
git commit -m "feat(cli): validate real github backend readiness"
```

## Task 9: Update Manual Phase A Acceptance Runbook

**Files:**
- Modify: `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md`

- [ ] **Step 1: Replace the runbook acceptance chain**

Update the runbook so the required manual test is:

```text
setup -> new-project -> new-milestone -> plan-phase -> execute-phase -> verify-work -> complete-milestone -> new-milestone -> plan-phase
```

- [ ] **Step 2: Add real backend prerequisites**

Add:

```markdown
Required environment:

- `GITHUB_TOKEN` or `GH_TOKEN` with repository and Projects v2 access
- `.kata/preferences.md` configured with `workflow.mode: github`, `stateMode: projects_v2`, and `githubProjectNumber`
- A GitHub repository connected to the target Project v2 project
```

- [ ] **Step 3: Add evidence requirements**

Add:

```markdown
For acceptance, capture:

1. Pi transcript showing each skill invocation.
2. GitHub Project screenshot or URL showing milestone/slice/task items.
3. GitHub issue/comment URLs for project, milestone, plan, summary, verification, and UAT artifacts.
4. CLI `kata doctor` output showing valid backend health.
```

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md
git commit -m "docs: define phase a real backend validation"
```

## Task 10: Add CI Guards Without Claiming Real Backend Acceptance

**Files:**
- Modify: `scripts/ci/build-kata-distributions.sh`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Update distribution script checks**

In `scripts/ci/build-kata-distributions.sh`, add:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-skill-surface.vitest.test.ts
pnpm --dir apps/cli exec vitest run src/tests/phase-a-contract.vitest.test.ts
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.client.vitest.test.ts
pnpm --dir apps/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
```

Remove any wording that implies CI proves the real backend end-to-end acceptance flow.

- [ ] **Step 2: Add CI artifact assertions**

Add:

```bash
test -f apps/cli/skills/kata-new-milestone/SKILL.md
test -f apps/cli/skills/kata-complete-milestone/SKILL.md
test ! -e apps/cli/skills/kata-discuss-phase
rg -n "references/alignment.md" apps/cli/skills/kata-plan-phase/SKILL.md >/dev/null
```

- [ ] **Step 3: Run the distribution script locally**

Run:

```bash
bash scripts/ci/build-kata-distributions.sh
```

Expected: pass.

- [ ] **Step 4: Commit**

```bash
git add scripts/ci/build-kata-distributions.sh .github/workflows/ci.yml
git commit -m "ci: guard phase a skill and cli contract"
```

## Task 11: Full Local Verification Before Manual Acceptance

**Files:**
- No code files unless failures require fixes

- [ ] **Step 1: Run CLI test suite**

Run:

```bash
pnpm --dir apps/cli run test:vitest
```

Expected: all tests pass.

- [ ] **Step 2: Run skill surface tests**

Run:

```bash
pnpm --dir apps/cli exec vitest run src/tests/phase-a-skill-surface.vitest.test.ts src/tests/build-skill-bundle.vitest.test.ts
```

Expected: all tests pass.

- [ ] **Step 3: Run desktop typecheck**

Run:

```bash
pnpm --dir apps/desktop run typecheck
```

Expected: pass.

- [ ] **Step 4: Run affected validation**

Run:

```bash
pnpm run validate:affected
```

Expected: pass.

- [ ] **Step 5: Commit any verification fixes**

If any command required code fixes, commit them:

```bash
git add <changed-files>
git commit -m "fix: stabilize phase a verification"
```

If no fixes were needed, do not create an empty commit.

## Task 12: Manual Phase A Acceptance Run

**Files:**
- No code files unless acceptance failures require fixes

- [ ] **Step 1: Build skills and CLI**

Run:

```bash
pnpm --dir apps/cli run build
```

Expected: both pass.

- [ ] **Step 2: Install skills into Pi**

Run from repo root:

```bash
node apps/cli/dist/loader.js setup --pi
node apps/cli/dist/loader.js doctor
```

Expected: setup returns `"ok": true`; doctor status is `"ok"`.

- [ ] **Step 3: Start Pi and run the acceptance chain**

Run:

```bash
pi
```

At the Pi prompt, run:

```text
/skill:kata-setup
/skill:kata-new-project
/skill:kata-new-milestone
/skill:kata-plan-phase
/skill:kata-execute-phase
/skill:kata-verify-work
/skill:kata-complete-milestone
/skill:kata-new-milestone
/skill:kata-plan-phase
```

Expected: every skill uses CLI-backed IO and completes without asking for legacy `.planning` files or `kata-tools.cjs`.

- [ ] **Step 4: Capture backend evidence**

Capture:

```text
GitHub Project URL:
Project tracking issue URL:
Milestone issue URL:
First slice issue URL:
First task issue URL:
Plan artifact comment URL:
Summary artifact comment URL:
UAT artifact comment URL:
Second milestone issue URL:
Second plan artifact comment URL:
```

- [ ] **Step 5: Fix acceptance failures before declaring Phase A complete**

If any acceptance step fails, create a focused fix commit:

```bash
git add <changed-files>
git commit -m "fix: complete phase a acceptance path"
```

Re-run the failed acceptance segment and update evidence.

## Final Phase A Completion Criteria

Phase A can be called complete only when:

1. `pnpm --dir apps/cli run test:vitest` passes.
2. `pnpm --dir apps/cli exec vitest run src/tests/phase-a-skill-surface.vitest.test.ts src/tests/build-skill-bundle.vitest.test.ts` passes.
3. `pnpm --dir apps/desktop run typecheck` passes.
4. `pnpm run validate:affected` passes.
5. `bash scripts/ci/build-kata-distributions.sh` passes.
6. Manual Pi acceptance chain completes against real GitHub Projects v2.
7. Backend evidence URLs are recorded in the validation runbook or a linked acceptance note.
