---
type: Plan
title: Github Projects V2 State Source Of Truth Implementation Plan
description: Archived implementation plan: Github Projects V2 State Source Of Truth Implementation Plan.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# GitHub Projects v2 State Source Of Truth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the GitHub Projects v2 CLI adapter derive Kata project state from GitHub issue state and Project v2 fields, with issue bodies used only for user-facing content.

**Architecture:** Project v2 fields identify and classify Kata records. GitHub issue state is authoritative for terminal status, Project v2 `Status` is authoritative for open workflow status, native GitHub dependencies remain authoritative for blockers, and native sub-issues define task membership. The domain snapshot service keeps using adapter-returned slice/task data.

**Tech Stack:** TypeScript, Vitest, GitHub REST and GraphQL clients, existing `KataBackendAdapter`, existing GitHub Projects v2 fake client tests.

---

## File Structure

- Modify `apps/cli/src/backends/github-projects-v2/adapter.ts`
  - Expand Project v2 item GraphQL reads.
  - Build `TrackedEntity` records from Project v2 fields and issue content.
  - Map status from issue state first, then Project v2 `Status`.
  - Discover task parentage from native sub-issues.
  - Stop writing `kata:entity` metadata into issue bodies.
- Modify `apps/cli/src/backends/github-projects-v2/project-fields.ts`
  - Keep the required field list limited to the fields the adapter uses.
  - Ensure `Status` is validated as the Project v2 single-select field with required status options.
- Modify `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
  - Add regression tests for stale body metadata, Project v2 field discovery, clean issue bodies, status updates, and completed milestone snapshots.
  - Update the fake GitHub client to store Project v2 item field values and native sub-issue relationships.
- Modify `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`
  - Keep the golden GitHub Projects v2 path aligned with marker-free bodies and field-based discovery.

## Task 1: Add Failing Snapshot Status Regression Tests

**Files:**

- Modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
- Test: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`

- [ ] **Step 1: Add stale status and completion tests**

Add these tests inside `describe("GithubProjectsV2Adapter", () => { ... })`, after the existing native dependency snapshot test.

```ts
  it("treats closed GitHub slice and task issues as done even when body metadata is stale", async () => {
    const client = createFakeGithubClient({
      issues: [
        githubIssue({
          id: 1,
          node_id: "issue-node-1",
          number: 1,
          title: "[M001] Existing Milestone",
          body: "Existing milestone",
          state: "open",
          milestoneNumber: 1,
        }),
        githubIssue({
          id: 2,
          node_id: "issue-node-2",
          number: 2,
          title: "[S001] Closed Slice",
          body: '<!-- kata:entity {"kataId":"S001","type":"Slice","parentId":"M001","status":"backlog"} -->\nClosed slice body',
          state: "closed",
          milestoneNumber: 1,
        }),
        githubIssue({
          id: 3,
          node_id: "issue-node-3",
          number: 3,
          title: "[T001] Closed Task",
          body: '<!-- kata:entity {"kataId":"T001","type":"Task","parentId":"S001","status":"backlog","verificationState":"pending"} -->\nClosed task body',
          state: "closed",
          milestoneNumber: 1,
        }),
      ],
      projectItems: [
        projectItem({
          itemId: "project-item-1",
          issueNodeId: "issue-node-1",
          issueNumber: 1,
          kataId: "M001",
          kataType: "Milestone",
          artifactScope: "M001",
          status: "Todo",
        }),
        projectItem({
          itemId: "project-item-2",
          issueNodeId: "issue-node-2",
          issueNumber: 2,
          kataId: "S001",
          kataType: "Slice",
          parentId: "M001",
          artifactScope: "S001",
          status: "Backlog",
        }),
        projectItem({
          itemId: "project-item-3",
          issueNodeId: "issue-node-3",
          issueNumber: 3,
          kataId: "T001",
          kataType: "Task",
          parentId: "S001",
          artifactScope: "T001",
          status: "Backlog",
          verificationState: "verified",
        }),
      ],
      subIssuesByParent: new Map([[2, [3]]),
    });
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });
    const api = createKataDomainApi(adapter);

    const snapshot = await api.project.getSnapshot();

    expect(snapshot.slices).toEqual([
      expect.objectContaining({
        id: "S001",
        status: "done",
        tasks: [
          expect.objectContaining({
            id: "T001",
            sliceId: "S001",
            status: "done",
            verificationState: "verified",
          }),
        ],
      }),
    ]);
    expect(snapshot.readiness.allSlicesDone).toBe(true);
    expect(snapshot.readiness.allTasksDone).toBe(true);
    expect(snapshot.nextAction).toMatchObject({
      workflow: "kata-complete-milestone",
      target: { milestoneId: "M001" },
    });
  });

  it("maps open GitHub slice status from Project v2 Status", async () => {
    const client = createFakeGithubClient({
      issues: [
        githubIssue({
          id: 1,
          node_id: "issue-node-1",
          number: 1,
          title: "[M001] Existing Milestone",
          body: "Existing milestone",
          state: "open",
          milestoneNumber: 1,
        }),
        githubIssue({
          id: 2,
          node_id: "issue-node-2",
          number: 2,
          title: "[S001] Active Slice",
          body: '<!-- kata:entity {"kataId":"S001","type":"Slice","parentId":"M001","status":"backlog"} -->\nActive slice body',
          state: "open",
          milestoneNumber: 1,
        }),
      ],
      projectItems: [
        projectItem({
          itemId: "project-item-1",
          issueNodeId: "issue-node-1",
          issueNumber: 1,
          kataId: "M001",
          kataType: "Milestone",
          artifactScope: "M001",
          status: "Todo",
        }),
        projectItem({
          itemId: "project-item-2",
          issueNodeId: "issue-node-2",
          issueNumber: 2,
          kataId: "S001",
          kataType: "Slice",
          parentId: "M001",
          artifactScope: "S001",
          status: "In Progress",
        }),
      ],
    });
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });

    await expect(adapter.listSlices({ milestoneId: "M001" })).resolves.toEqual([
      expect.objectContaining({ id: "S001", status: "in_progress" }),
    ]);
  });
```

- [ ] **Step 2: Add helper builders for the tests**

Add these helpers near the existing `createFakeGithubClient` helper.

```ts
function githubIssue(input: {
  id: number;
  node_id: string;
  number: number;
  title: string;
  body: string;
  state: "open" | "closed";
  milestoneNumber?: number;
}) {
  return {
    id: input.id,
    node_id: input.node_id,
    number: input.number,
    title: input.title,
    body: input.body,
    state: input.state,
    html_url: `https://github.test/kata-sh/uat/issues/${input.number}`,
    milestone: input.milestoneNumber ? { number: input.milestoneNumber } : null,
  };
}

function projectItem(input: {
  itemId: string;
  issueNodeId: string;
  issueNumber: number;
  kataId: string;
  kataType: string;
  parentId?: string;
  artifactScope?: string;
  status?: string;
  verificationState?: string;
}) {
  return {
    id: input.itemId,
    content: {
      id: input.issueNodeId,
      number: input.issueNumber,
    },
    kataId: { text: input.kataId },
    kataType: { text: input.kataType },
    parentId: input.parentId ? { text: input.parentId } : null,
    artifactScope: input.artifactScope ? { text: input.artifactScope } : null,
    status: input.status ? { name: input.status } : null,
    verificationState: input.verificationState ? { text: input.verificationState } : null,
  };
}
```

- [ ] **Step 3: Extend the fake client input type**

Change the `createFakeGithubClient` input type to accept native sub-issues.

```ts
function createFakeGithubClient(
  input: {
    issues?: any[];
    issueListSnapshots?: any[][];
    projectFields?: any[];
    projectItems?: any[];
    nativeDependencies?: Array<{ blocked: number; blocker: number }>;
    subIssuesByParent?: Map<number, number[]>;
  } = {},
) {
```

- [ ] **Step 4: Run the new tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts -t "closed GitHub slice|open GitHub slice"
```

Expected: FAIL because Project v2 item discovery does not read `Kata Type`, `Kata Parent ID`, or `Kata Verification State`, and closed issue state does not override marker status.

- [ ] **Step 5: Keep the failing tests uncommitted**

Do not commit yet. The next tasks make these tests pass, then commit the green slice.

## Task 2: Read Kata Entity Data From Project v2 Fields

**Files:**

- Modify: `apps/cli/src/backends/github-projects-v2/adapter.ts`
- Modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
- Test: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`

- [ ] **Step 1: Expand Project item field types**

In `apps/cli/src/backends/github-projects-v2/adapter.ts`, replace `ProjectItemFields` with:

```ts
interface ProjectItemFields {
  itemId: string;
  kataId?: string;
  kataType?: KataEntityType;
  parentId?: string;
  artifactScope?: string;
  verificationState?: KataTaskVerificationState;
  contentId?: string;
  issueId?: number;
  issueNumber?: number;
  title?: string;
  body?: string;
  state?: string;
  url?: string;
  githubMilestoneNumber?: number;
  status?: string;
}
```

Replace `ProjectItemFieldNode` with:

```ts
interface ProjectItemFieldNode {
  id?: string | null;
  content?: {
    id?: string | null;
    databaseId?: number | null;
    number?: number | null;
    title?: string | null;
    body?: string | null;
    state?: string | null;
    url?: string | null;
    milestone?: { number?: number | null } | null;
  } | null;
  kataId?: ProjectItemTextFieldValue | null;
  kataType?: ProjectItemTextFieldValue | null;
  parentId?: ProjectItemTextFieldValue | null;
  artifactScope?: ProjectItemTextFieldValue | null;
  verificationState?: ProjectItemTextFieldValue | null;
  status?: ProjectItemSingleSelectFieldValue | null;
}
```

- [ ] **Step 2: Expand the Project item GraphQL query**

In both the `organization` and `user` item selections inside `PROJECT_ITEM_FIELDS_QUERY`, change the `content` block and field reads to:

```graphql
            content {
              ... on Issue {
                id
                databaseId
                number
                title
                body
                state
                url
                milestone {
                  number
                }
              }
            }
            kataId: fieldValueByName(name: ${JSON.stringify(KATA_PROJECT_FIELDS.id)}) {
              ... on ProjectV2ItemFieldTextValue {
                text
              }
            }
            kataType: fieldValueByName(name: ${JSON.stringify(KATA_PROJECT_FIELDS.type)}) {
              ... on ProjectV2ItemFieldTextValue {
                text
              }
            }
            parentId: fieldValueByName(name: ${JSON.stringify(KATA_PROJECT_FIELDS.parentId)}) {
              ... on ProjectV2ItemFieldTextValue {
                text
              }
            }
            artifactScope: fieldValueByName(name: ${JSON.stringify(KATA_PROJECT_FIELDS.artifactScope)}) {
              ... on ProjectV2ItemFieldTextValue {
                text
              }
            }
            verificationState: fieldValueByName(name: ${JSON.stringify(KATA_PROJECT_FIELDS.verificationState)}) {
              ... on ProjectV2ItemFieldTextValue {
                text
              }
            }
            status: fieldValueByName(name: ${JSON.stringify(KATA_PROJECT_FIELDS.status)}) {
              ... on ProjectV2ItemFieldSingleSelectValue {
                name
              }
            }
```

- [ ] **Step 3: Parse Project item field nodes**

Replace `projectItemFieldsFromNode` with:

```ts
function projectItemFieldsFromNode(
  node: ProjectItemFieldNode | null,
): ProjectItemFields | null {
  if (!node?.id) return null;
  const kataId = normalizeKataId(textFieldValue(node.kataId));
  const kataType = kataEntityTypeFromField(textFieldValue(node.kataType));
  const contentId = typeof node.content?.id === "string" && node.content.id ? node.content.id : undefined;
  const issueId = typeof node.content?.databaseId === "number" && Number.isFinite(node.content.databaseId)
    ? node.content.databaseId
    : undefined;
  const issueNumber = typeof node.content?.number === "number" && Number.isFinite(node.content.number)
    ? node.content.number
    : undefined;
  const title = typeof node.content?.title === "string" ? node.content.title : undefined;
  const body = typeof node.content?.body === "string" ? node.content.body : undefined;
  const state = normalizeGithubIssueState(node.content?.state);
  const url = typeof node.content?.url === "string" ? node.content.url : undefined;
  const githubMilestoneNumber = typeof node.content?.milestone?.number === "number" && Number.isFinite(node.content.milestone.number)
    ? node.content.milestone.number
    : undefined;
  const parentId = normalizeKataId(textFieldValue(node.parentId));
  const artifactScope = normalizeKataId(textFieldValue(node.artifactScope));
  const verificationState = taskVerificationStateFromField(textFieldValue(node.verificationState));
  const status = singleSelectFieldName(node.status);
  return {
    itemId: node.id,
    ...(kataId ? { kataId } : {}),
    ...(kataType ? { kataType } : {}),
    ...(parentId ? { parentId } : {}),
    ...(artifactScope ? { artifactScope } : {}),
    ...(verificationState ? { verificationState } : {}),
    ...(contentId ? { contentId } : {}),
    ...(issueId !== undefined ? { issueId } : {}),
    ...(issueNumber !== undefined ? { issueNumber } : {}),
    ...(title ? { title } : {}),
    ...(body !== undefined ? { body } : {}),
    ...(state ? { state } : {}),
    ...(url ? { url } : {}),
    ...(githubMilestoneNumber !== undefined ? { githubMilestoneNumber } : {}),
    ...(status ? { status } : {}),
  };
}
```

Add these helpers near the other adapter helper functions:

```ts
function normalizeKataId(value: string): string | undefined {
  const trimmed = value.trim().toUpperCase();
  return trimmed.length > 0 ? trimmed : undefined;
}

function kataEntityTypeFromField(value: string): KataEntityType | undefined {
  const trimmed = value.trim();
  return isKataEntityType(trimmed) ? trimmed : undefined;
}

function taskVerificationStateFromField(value: string): KataTaskVerificationState | undefined {
  const trimmed = value.trim();
  return isKataTaskVerificationState(trimmed) ? trimmed : undefined;
}

function normalizeGithubIssueState(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const normalized = value.trim().toLowerCase();
  if (normalized === "open" || normalized === "closed") return normalized;
  return undefined;
}
```

- [ ] **Step 4: Build entities from Project v2 items**

Add this helper below `entityFromIssue`:

```ts
function entityFromProjectItem(fields: ProjectItemFields): TrackedEntity | null {
  if (
    !fields.kataId ||
    !fields.kataType ||
    fields.issueId === undefined ||
    fields.issueNumber === undefined ||
    !fields.contentId ||
    !fields.title
  ) {
    return null;
  }

  return {
    kataId: fields.kataId,
    type: fields.kataType,
    parentId: fields.parentId,
    status: statusFromProjectFields(fields),
    verificationState: fields.verificationState,
    issueId: fields.issueId,
    issueNumber: fields.issueNumber,
    contentId: fields.contentId,
    title: stripKataPrefix(fields.title),
    body: fields.body ?? "",
    state: fields.state ?? "open",
    url: fields.url,
    githubMilestoneNumber: fields.githubMilestoneNumber,
  };
}
```

- [ ] **Step 5: Make discovery use Project v2 fields**

Replace `discoverEntities` with:

```ts
  private async discoverEntities(): Promise<void> {
    if (this.discovered) return;

    const projectItemFields = await this.loadProjectItemFields();
    const entities = projectItemFields.map(entityFromProjectItem).filter(isTrackedEntity);
    const entitiesWithNativeParents = await this.loadNativeTaskParents(entities);
    const entitiesWithNativeDependencies = await this.loadNativeIssueDependencies(entitiesWithNativeParents);
    for (const entity of entitiesWithNativeDependencies) {
      if (this.entities.has(entity.kataId)) continue;
      this.entities.set(entity.kataId, entity);
    }
    this.discovered = true;
  }
```

Add:

```ts
function isTrackedEntity(value: TrackedEntity | null): value is TrackedEntity {
  return value !== null;
}
```

- [ ] **Step 6: Run the targeted tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts -t "closed GitHub slice|open GitHub slice"
```

Expected: tests still fail until status precedence and task parent discovery are implemented.

- [ ] **Step 7: Keep this change with the failing test slice**

Do not commit yet. Task 3 completes the status and parent derivation needed for the new tests to pass.

## Task 3: Implement Native Status And Task Parent Derivation

**Files:**

- Modify: `apps/cli/src/backends/github-projects-v2/adapter.ts`
- Modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
- Test: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`

- [ ] **Step 1: Add REST fake support for sub-issue reads**

Inside `createFakeGithubClient`, update the sub-issues REST handler:

```ts
      const subIssuesMatch = request.path.match(/^\/repos\/kata-sh\/uat\/issues\/(\d+)\/sub_issues$/);
      if (request.method === "GET" && subIssuesMatch) {
        const parentNumber = Number(subIssuesMatch[1]);
        const childNumbers = input.subIssuesByParent?.get(parentNumber) ?? [];
        return childNumbers
          .map((childNumber) => issues.find((issue) => issue.number === childNumber))
          .filter(Boolean);
      }
      if (request.method === "POST" && subIssuesMatch) {
        const parentNumber = Number(subIssuesMatch[1]);
        const child = issues.find((issue) => issue.id === request.body.sub_issue_id);
        if (child) {
          const existing = input.subIssuesByParent?.get(parentNumber) ?? [];
          input.subIssuesByParent?.set(parentNumber, [...new Set([...existing, child.number])]);
        }
        return {
          parent_issue_number: parentNumber,
          sub_issue_id: request.body.sub_issue_id,
        };
      }
```

- [ ] **Step 2: Add native task parent loading**

In `GithubProjectsV2Adapter`, add:

```ts
  private async loadNativeTaskParents(entities: TrackedEntity[]): Promise<TrackedEntity[]> {
    const slices = entities.filter((entity) => entity.type === "Slice");
    const taskIdByIssueNumber = new Map(
      entities
        .filter((entity) => entity.type === "Task")
        .map((entity) => [entity.issueNumber, entity.kataId]),
    );
    if (slices.length === 0 || taskIdByIssueNumber.size === 0) return entities;

    const parentByTaskId = new Map<string, string>();
    for (const slice of slices) {
      const children = await this.client.rest<GithubIssue[]>({
        method: "GET",
        path: `/repos/${this.owner}/${this.repo}/issues/${slice.issueNumber}/sub_issues`,
      });
      for (const child of children) {
        const taskId = taskIdByIssueNumber.get(child.number);
        if (taskId) parentByTaskId.set(taskId, slice.kataId);
      }
    }

    return entities.map((entity) => {
      if (entity.type !== "Task") return entity;
      const nativeParentId = parentByTaskId.get(entity.kataId);
      return nativeParentId ? { ...entity, parentId: nativeParentId } : entity;
    });
  }
```

- [ ] **Step 3: Add status precedence helpers**

Replace `sliceStatusFromProjectStatusName` with:

```ts
function statusFromProjectFields(fields: ProjectItemFields): KataSliceStatus | KataTaskStatus | undefined {
  if (fields.state === "closed") return "done";
  return statusFromProjectStatusName(fields.status);
}

function statusFromProjectStatusName(value: string | undefined): KataSliceStatus | KataTaskStatus | undefined {
  switch (value) {
    case "Backlog":
      return "backlog";
    case "Todo":
      return "todo";
    case "In Progress":
      return "in_progress";
    case "Agent Review":
    case "Human Review":
    case "Merging":
      return "in_progress";
    case "Done":
      return "done";
    default:
      return undefined;
  }
}

function sliceStatusFromProjectStatusName(value: string | undefined): KataSliceStatus | undefined {
  switch (value) {
    case "Backlog":
      return "backlog";
    case "Todo":
      return "todo";
    case "In Progress":
      return "in_progress";
    case "Agent Review":
      return "agent_review";
    case "Human Review":
      return "human_review";
    case "Merging":
      return "merging";
    case "Done":
      return "done";
    default:
      return undefined;
  }
}
```

Then update `statusFromProjectFields` to preserve full slice statuses:

```ts
function statusFromProjectFields(fields: ProjectItemFields): KataSliceStatus | KataTaskStatus | undefined {
  if (fields.state === "closed") return "done";
  if (fields.kataType === "Slice") return sliceStatusFromProjectStatusName(fields.status);
  return statusFromProjectStatusName(fields.status);
}
```

- [ ] **Step 4: Make entity status readers prefer issue state**

Replace the status reader helpers with:

```ts
function sliceStatusFromEntity(entity: TrackedEntity): KataSliceStatus {
  if (entity.state === "closed") return "done";
  return isKataSliceStatus(entity.status) ? entity.status : "backlog";
}

function taskStatusFromEntity(entity: TrackedEntity): KataTaskStatus {
  if (entity.state === "closed") return "done";
  return isKataTaskStatus(entity.status) ? entity.status : "backlog";
}

function issueStatusFromEntity(entity: TrackedEntity): KataIssue["status"] {
  if (entity.state === "closed") return "done";
  return isKataTaskStatus(entity.status) ? entity.status : "backlog";
}
```

- [ ] **Step 5: Run the targeted tests and verify they pass**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts -t "closed GitHub slice|open GitHub slice"
```

Expected: PASS.

- [ ] **Step 6: Run the full adapter suite**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
```

Expected: existing tests may fail where they still expect marker-based bodies or fake client Project item data. Fix only those test fixtures to include Project v2 fields and sub-issue reads.

- [ ] **Step 7: Commit Project v2 discovery, native state, and parent derivation**

```bash
git add apps/cli/src/backends/github-projects-v2/adapter.ts apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts
git commit -m "fix(cli): derive github project state natively"
```

## Task 4: Stop Writing Entity Metadata Into Issue Bodies

**Files:**

- Modify: `apps/cli/src/backends/github-projects-v2/adapter.ts`
- Modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
- Modify: `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`
- Test: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`

- [ ] **Step 1: Add tests for marker-free bodies and status updates**

Add this test to `GithubProjectsV2Adapter` tests:

```ts
  it("writes user-facing issue bodies without kata entity metadata", async () => {
    const client = createFakeGithubClient();
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });

    await adapter.upsertProject({ title: "Launch Kata", description: "Project brief" });
    const milestone = await adapter.createMilestone({ title: "Phase A", goal: "Milestone goal" });
    const slice = await adapter.createSlice({ milestoneId: milestone.id, title: "Slice", goal: "Slice goal" });
    const task = await adapter.createTask({ sliceId: slice.id, title: "Task", description: "Task details" });
    await adapter.createIssue({ title: "Standalone", design: "Design body", plan: "Plan body" });
    await adapter.updateTaskStatus({ taskId: task.id, status: "done", verificationState: "verified" });

    const createdIssueBodies = client.rest.mock.calls
      .filter(([request]) => request.method === "POST" && request.path === "/repos/kata-sh/uat/issues")
      .map(([request]) => request.body.body);
    expect(createdIssueBodies).toEqual([
      "Project brief",
      "Milestone goal",
      "Slice goal",
      "Task details",
      "# Design\n\nDesign body\n\n# Plan\n\nPlan body",
    ]);

    const statusPatch = client.rest.mock.calls.find(([request]) =>
      request.method === "PATCH" &&
      request.path === "/repos/kata-sh/uat/issues/4"
    )?.[0];
    expect(statusPatch.body).toEqual({ state: "closed" });
  });
```

- [ ] **Step 2: Run the marker-free write test and verify it fails**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts -t "writes user-facing issue bodies"
```

Expected: FAIL because create methods write `<!-- kata:entity ... -->` and status updates patch body metadata.

- [ ] **Step 3: Replace entity body formatting at write sites**

In `upsertProject`, replace:

```ts
    const body = formatEntityBody({
      kataId: "PROJECT",
      type: "Project",
      content: input.description,
    });
```

with:

```ts
    const body = input.description;
```

In `createMilestone`, replace the `body: formatEntityBody(...)` block with:

```ts
      body: input.goal,
```

In `createSlice`, replace the `body: formatEntityBody(...)` block with:

```ts
      body: input.goal,
```

In `createTask`, replace the `body: formatEntityBody(...)` block with:

```ts
      body: input.description,
```

In `createIssue`, replace:

```ts
    const body = formatEntityBody({
      kataId,
      type: "Issue",
      status: "backlog",
      content: formatPlannedIssueBody(input.design, input.plan),
    });
```

with:

```ts
    const body = formatPlannedIssueBody(input.design, input.plan);
```

- [ ] **Step 4: Simplify concurrent task ID retagging**

Change `ensureCreatedEntityHasUniqueId` input type:

```ts
    input: { title: string; formatBody(uniqueKataId: string): string },
```

to:

```ts
    input: { title: string; body: string },
```

Replace the retag update body:

```ts
      body: input.formatBody(uniqueKataId),
```

with:

```ts
      body: input.body,
```

Update the caller in `createTask`:

```ts
    const uniqueEntity = await this.ensureCreatedEntityHasUniqueId(entity, {
      title: input.title,
      body: input.description,
    });
```

- [ ] **Step 5: Stop status updates from patching bodies**

Change `updateEntityStatus` signature:

```ts
    metadata: Pick<EntityMarker, "status" | "verificationState"> = {},
```

to:

```ts
    metadata: { verificationState?: KataTaskVerificationState } = {},
```

Replace the final patch:

```ts
    return this.updateIssueEntity(entity, {
      state: issueState,
      body: updateEntityBodyMarker(entity, metadata),
    });
```

with:

```ts
    return this.updateIssueEntity(entity, {
      state: issueState,
    });
```

Update `updateSliceStatus`, `updateTaskStatus`, and `updateIssueStatus` calls so they pass only verification state when present:

```ts
    const updated = await this.updateEntityStatus(entity, statusOptionForSlice(input.status));
```

```ts
    const updated = await this.updateEntityStatus(entity, statusOptionForTask(input.status), {
      verificationState,
    });
```

```ts
    const updated = await this.updateEntityStatus(entity, statusOptionForIssue(input.status));
```

- [ ] **Step 6: Stop parsing entity markers during issue updates**

Replace this block in `updateIssueEntity`:

```ts
    const updatedMarker = parseEntityMarker(updatedBody);
    const updated = {
      ...entity,
      parentId: updatedMarker?.parentId ?? entity.parentId,
      status: updatedMarker?.status ?? entity.status,
      verificationState: updatedMarker?.verificationState ?? entity.verificationState,
```

with:

```ts
    const updated = {
      ...entity,
```

Keep the existing title, body, and state assignments.

- [ ] **Step 7: Replace body content extraction**

Replace `bodyContent` with:

```ts
function bodyContent(body: string): string {
  return body;
}
```

Delete `formatEntityBody`, `parseEntityMarker`, `EntityMarker`, `ENTITY_MARKER_PREFIX`, `ENTITY_MARKER_SUFFIX`, `isEntityMarker`, and `updateEntityBodyMarker` after TypeScript no longer needs them.

- [ ] **Step 8: Run the marker-free write test**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts -t "writes user-facing issue bodies"
```

Expected: PASS.

- [ ] **Step 9: Run adapter and golden path suites**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts src/tests/golden-path.pi-github.vitest.test.ts
```

Expected: PASS after updating expectations that previously checked for `kata:entity` in issue bodies.

- [ ] **Step 10: Commit marker-free writes**

```bash
git add apps/cli/src/backends/github-projects-v2/adapter.ts apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts apps/cli/src/tests/golden-path.pi-github.vitest.test.ts
git commit -m "refactor(cli): remove github entity body metadata"
```

## Task 5: Tighten Project v2 Field Validation And Health Coverage

**Files:**

- Modify: `apps/cli/src/backends/github-projects-v2/project-fields.ts`
- Modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
- Modify: `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`
- Test: `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`

- [ ] **Step 1: Add field validation regression tests**

Add these tests to `GithubProjectsV2Adapter` tests near the existing field validation test.

```ts
  it("does not require superfluous dependency text fields", async () => {
    const client = createFakeGithubClient({
      projectFields: validProjectFields(),
    });
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });

    await expect(adapter.upsertProject({
      title: "Launch Kata",
      description: "Project brief",
    })).resolves.toMatchObject({
      backend: "github",
      title: "Launch Kata",
    });
  });

  it("requires Status to contain every Kata workflow option", async () => {
    const client = createFakeGithubClient({
      projectFields: validProjectFields({
        statusOptions: validStatusOptions().filter((option) => option.name !== "Done"),
      }),
    });
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });

    await expect(adapter.upsertProject({
      title: "Launch Kata",
      description: "Project brief",
    })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      message: expect.stringContaining('Status" is missing option "Done"'),
    });
  });
```

- [ ] **Step 2: Validate Status options in `project-fields.ts`**

In `validateProjectFieldIndex`, add:

```ts
  const statusField = fields[KATA_PROJECT_FIELDS.status];
  const missingStatusOptions = KATA_STATUS_OPTIONS.filter((option) => !statusField?.options?.[option]);
```

Then include this branch in the thrown message list:

```ts
        ...(missingStatusOptions.length
          ? [
              `GitHub Projects v2 field "${KATA_PROJECT_FIELDS.status}" is missing required options:`,
              formatBulletList(missingStatusOptions),
              "",
            ]
          : []),
```

Update the `if` condition:

```ts
  if (missingFields.length || incorrectlyTypedFields.length || missingStatusOptions.length) {
```

- [ ] **Step 3: Keep required text fields limited to adapter fields**

Confirm `REQUIRED_TEXT_FIELD_NAMES` remains:

```ts
const REQUIRED_TEXT_FIELD_NAMES = [
  KATA_PROJECT_FIELDS.type,
  KATA_PROJECT_FIELDS.id,
  KATA_PROJECT_FIELDS.parentId,
  KATA_PROJECT_FIELDS.artifactScope,
  KATA_PROJECT_FIELDS.verificationState,
] as const;
```

Do not add `Kata Blocking` or `Kata Blocked By`.

- [ ] **Step 4: Add health coverage for missing Project v2 item values**

Add this test to `GithubProjectsV2Adapter` tests:

```ts
  it("warns when Project v2 items are missing required Kata field values", async () => {
    const client = createFakeGithubClient({
      issues: [
        githubIssue({
          id: 1,
          node_id: "issue-node-1",
          number: 1,
          title: "[S001] Missing Type",
          body: "Missing type field",
          state: "open",
          milestoneNumber: 1,
        }),
      ],
      projectItems: [
        {
          id: "project-item-1",
          content: { id: "issue-node-1", number: 1 },
          kataId: { text: "S001" },
          kataType: null,
          status: { name: "Todo" },
        },
      ],
    });
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });

    await expect(adapter.checkHealth()).resolves.toMatchObject({
      ok: false,
      backend: "github",
      checks: expect.arrayContaining([
        expect.objectContaining({
          name: "project-item-fields",
          status: "warn",
          message: expect.stringContaining("1 Project v2 item is missing required Kata field values"),
        }),
      ]),
    });
  });
```

- [ ] **Step 5: Implement health item value checks**

Replace `checkHealth` in `apps/cli/src/backends/github-projects-v2/adapter.ts` with:

```ts
  async checkHealth(): Promise<KataHealthReport> {
    const checks: KataHealthReport["checks"] = [
      {
        name: "adapter",
        status: "ok",
        message: "GitHub Projects v2 adapter is configured.",
      },
    ];

    await this.getFieldIndex();
    const projectItems = await this.loadProjectItemFields();
    const incompleteItems = projectItems.filter(hasIncompleteKataProjectItemFields);
    if (incompleteItems.length > 0) {
      checks.push({
        name: "project-item-fields",
        status: "warn",
        message: `${incompleteItems.length} Project v2 item${incompleteItems.length === 1 ? " is" : "s are"} missing required Kata field values.`,
      });
    }

    return {
      ok: checks.every((check) => check.status === "ok"),
      backend: "github",
      checks,
    };
  }
```

Add this helper near `isProjectItemFields`:

```ts
function hasIncompleteKataProjectItemFields(fields: ProjectItemFields): boolean {
  const hasAnyKataField = Boolean(
    fields.kataId ||
    fields.kataType ||
    fields.parentId ||
    fields.artifactScope ||
    fields.verificationState,
  );
  if (!hasAnyKataField) return false;
  return !fields.kataId || !fields.kataType;
}
```

- [ ] **Step 6: Run field validation and health tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts -t "dependency text fields|Status to contain|missing required Kata field values"
```

Expected: PASS.

- [ ] **Step 7: Run golden path doctor tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/golden-path.pi-github.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 8: Commit validation cleanup**

```bash
git add apps/cli/src/backends/github-projects-v2/project-fields.ts apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts apps/cli/src/tests/golden-path.pi-github.vitest.test.ts
git commit -m "fix(cli): validate github project fields used by kata"
```

## Task 6: Run Full CLI Validation

**Files:**

- No source changes expected.
- Test: full affected CLI validation.

- [ ] **Step 1: Run GitHub Projects v2 adapter tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 2: Run GitHub golden path tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/golden-path.pi-github.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 3: Run all CLI tests**

Run:

```bash
pnpm --filter @kata-sh/cli test
```

Expected: PASS.

- [ ] **Step 4: Run affected validation**

Run:

```bash
pnpm run validate:affected
```

Expected: PASS.

- [ ] **Step 5: Check git status**

Run:

```bash
git status --short
```

Expected: only the intentionally untracked `docs/plans/backend-state-source-of-truth.md` may remain if it still exists. No implementation files should be unstaged.
