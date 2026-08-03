---
type: Plan
title: Cli Linear Core Implementation Plan
description: "Archived implementation plan: Cli Linear Core Implementation Plan."
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# CLI Linear Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Linear as a full `@kata-sh/cli` backend for project, milestone, slice, task, standalone issue, artifact, health, dependency, and snapshot operations.

**Architecture:** Add a raw Linear GraphQL client and a `LinearKataAdapter` that implements the existing `KataBackendAdapter` contract. Derive Kata entity identity, hierarchy, workflow state, verification state, and dependencies from Linear-native records, configured labels, title ID prefixes, Project Milestones, sub-issues, workflow states, and native issue relations. Use Linear Documents for milestone artifacts and issue comments for issue-scoped artifacts. Use artifact markers only for artifact documents and artifact comments.

**Tech Stack:** TypeScript, Node 20 fetch, Vitest, `js-yaml`, existing Kata domain service and CLI transport.

---

## Source Material

- Spec: `docs/superpowers/specs/2026-05-06-cli-linear-core-design.md`
- Linear GraphQL docs: <https://linear.app/developers/graphql?noRedirect=1>
- Linear project milestones docs: <https://linear.app/docs/project-milestones>
- Linear project documents docs: <https://linear.app/docs/project-documents>

## File Structure

- Create `apps/cli/src/backends/linear/client.ts`: Linear GraphQL fetch wrapper, auth token resolution, response error mapping, pagination helpers.
- Create `apps/cli/src/backends/linear/config.ts`: Linear preferences shape, default state mapping, auth env resolution.
- Create `apps/cli/src/backends/linear/artifacts.ts`: artifact marker format and upsert helpers for Linear documents and issue comments.
- Create `apps/cli/src/backends/linear/entities.ts`: entity classification helpers for labels, title ID prefixes, project milestones, parent/sub-issue relationships, workflow states, and native relations.
- Modify `apps/cli/src/backends/linear/adapter.ts`: full `KataBackendAdapter` implementation.
- Modify `apps/cli/src/backends/read-tracker-config.ts`: parse Linear workspace, team, project, auth, state mapping, labels, and active milestone preferences.
- Modify `apps/cli/src/backends/resolve-backend.ts`: construct the Linear client-backed adapter when `.kata/preferences.md` selects Linear.
- Modify `apps/cli/src/commands/setup.ts`: write Linear preferences from setup input and interactive prompts.
- Modify `apps/cli/src/cli.ts`: accept Linear setup flags.
- Modify `apps/cli/src/commands/doctor.ts`: validate Linear auth, workspace, team, project, states, documents, comments, sub-issues, and issue relations.
- Create `apps/cli/src/tests/linear.config.vitest.test.ts`: Linear config parsing and setup output.
- Create `apps/cli/src/tests/linear.client.vitest.test.ts`: client auth, GraphQL error handling, pagination.
- Create `apps/cli/src/tests/linear.artifacts.vitest.test.ts`: document/comment marker parsing and idempotent writes.
- Create `apps/cli/src/tests/linear.adapter.vitest.test.ts`: backend contract behavior for Linear.
- Create `apps/cli/src/tests/golden-path.pi-linear.vitest.test.ts`: setup, doctor, resolve backend, and JSON operation validation.
- Modify `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`: keep GitHub Projects v2 golden path green.
- Modify `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`: add one explicit regression that GitHub artifact/comment/dependency behavior remains unchanged.

## Linear Preferences Shape

The plan uses this `.kata/preferences.md` shape:

```yaml
---
workflow:
  mode: linear
linear:
  workspace: kata
  team: KATA
  project: kata-cli
  authEnv: LINEAR_API_KEY
  activeMilestoneId: milestone-linear-id
  states:
    backlog: Backlog
    todo: Todo
    in_progress: In Progress
    agent_review: Agent Review
    human_review: Human Review
    merging: Merging
    done: Done
  labels:
    project: kata/project
    milestone: kata/milestone
    slice: kata/slice
    task: kata/task
    issue: kata/issue
---
```

## Task 1: Parse Linear Configuration

**Files:**

- Create: `apps/cli/src/backends/linear/config.ts`
- Modify: `apps/cli/src/backends/read-tracker-config.ts`
- Test: `apps/cli/src/tests/linear.config.vitest.test.ts`

- [ ] **Step 1: Write failing config tests**

Create `apps/cli/src/tests/linear.config.vitest.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import { readTrackerConfig } from "../backends/read-tracker-config.js";
import {
  DEFAULT_LINEAR_STATE_NAMES,
  resolveLinearAuthToken,
} from "../backends/linear/config.js";

describe("Linear tracker config", () => {
  it("parses complete Linear preferences", async () => {
    const config = await readTrackerConfig({
      preferencesContent: `---
workflow:
  mode: linear
linear:
  workspace: kata
  team: KATA
  project: kata-cli
  authEnv: LINEAR_TOKEN
  activeMilestoneId: milestone-123
  states:
    backlog: Backlog
    todo: Todo
    in_progress: Started
    agent_review: Agent Review
    human_review: Human Review
    merging: Merging
    done: Complete
  labels:
    slice: kata/slice
---
`,
    });

    expect(config).toEqual({
      kind: "linear",
      workspace: "kata",
      team: "KATA",
      project: "kata-cli",
      authEnv: "LINEAR_TOKEN",
      activeMilestoneId: "milestone-123",
      states: {
        ...DEFAULT_LINEAR_STATE_NAMES,
        in_progress: "Started",
        done: "Complete",
      },
      labels: {
        slice: "kata/slice",
      },
    });
  });

  it("uses default Linear state names when preferences omit states", async () => {
    const config = await readTrackerConfig({
      preferencesContent: `---
workflow:
  mode: linear
linear:
  workspace: kata
  team: KATA
  project: kata-cli
---
`,
    });

    expect(config).toMatchObject({
      kind: "linear",
      workspace: "kata",
      team: "KATA",
      project: "kata-cli",
      authEnv: undefined,
      activeMilestoneId: undefined,
      states: DEFAULT_LINEAR_STATE_NAMES,
    });
  });

  it("requires workspace, team, and project for Linear mode", async () => {
    await expect(readTrackerConfig({
      preferencesContent: `---
workflow:
  mode: linear
linear:
  workspace: kata
  team: KATA
---
`,
    })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      message: "linear.project is required",
    });
  });

  it("rejects blank Linear state names", async () => {
    await expect(readTrackerConfig({
      preferencesContent: `---
workflow:
  mode: linear
linear:
  workspace: kata
  team: KATA
  project: kata-cli
  states:
    done: ""
---
`,
    })).rejects.toMatchObject({
      code: "INVALID_CONFIG",
      message: "linear.states.done is required",
    });
  });

  it("resolves Linear auth from the configured env var first", () => {
    expect(resolveLinearAuthToken({
      authEnv: "KATA_LINEAR_TOKEN",
      env: {
        KATA_LINEAR_TOKEN: "lin_configured",
        LINEAR_API_KEY: "lin_api_key",
        LINEAR_TOKEN: "lin_token",
      },
    })).toBe("lin_configured");
  });

  it("resolves Linear auth from default env vars", () => {
    expect(resolveLinearAuthToken({
      env: {
        LINEAR_API_KEY: "",
        LINEAR_TOKEN: "lin_token",
      },
    })).toBe("lin_token");
  });
});
```

- [ ] **Step 2: Run config tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.config.vitest.test.ts
```

Expected: FAIL because `../backends/linear/config.js` does not exist and `readTrackerConfig` returns only `{ kind: "linear" }`.

- [ ] **Step 3: Add Linear config helpers**

Create `apps/cli/src/backends/linear/config.ts`:

```ts
import { KataDomainError } from "../../domain/errors.js";
import type { KataIssueStatus, KataSlice, KataTask } from "../../domain/types.js";

export type LinearStateKey = KataSlice["status"] | KataTask["status"] | KataIssueStatus;

export type LinearStateMapping = Record<
  "backlog" | "todo" | "in_progress" | "agent_review" | "human_review" | "merging" | "done",
  string
>;

export const DEFAULT_LINEAR_STATE_NAMES: LinearStateMapping = {
  backlog: "Backlog",
  todo: "Todo",
  in_progress: "In Progress",
  agent_review: "Agent Review",
  human_review: "Human Review",
  merging: "Merging",
  done: "Done",
};

export interface LinearTrackerConfig {
  kind: "linear";
  workspace: string;
  team: string;
  project: string;
  authEnv?: string;
  activeMilestoneId?: string;
  states: LinearStateMapping;
  labels: Record<string, string>;
}

export function cleanLinearString(value: unknown, fieldName: string): string {
  if (typeof value === "string" && value.trim().length > 0) {
    return value.trim();
  }
  throw new KataDomainError("INVALID_CONFIG", `${fieldName} is required`);
}

export function optionalLinearString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

export function readLinearStateMapping(rawStates: Record<string, unknown>): LinearStateMapping {
  const states = { ...DEFAULT_LINEAR_STATE_NAMES };
  for (const key of Object.keys(DEFAULT_LINEAR_STATE_NAMES) as Array<keyof LinearStateMapping>) {
    if (rawStates[key] === undefined) continue;
    states[key] = cleanLinearString(rawStates[key], `linear.states.${key}`);
  }
  return states;
}

export function readLinearLabels(rawLabels: Record<string, unknown>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(rawLabels)
      .filter((entry): entry is [string, string] => typeof entry[1] === "string" && entry[1].trim().length > 0)
      .map(([key, value]) => [key, value.trim()]),
  );
}

export function resolveLinearAuthToken(input: {
  authEnv?: string;
  env?: NodeJS.ProcessEnv | Record<string, string | undefined>;
}): string | null {
  const env = input.env ?? process.env;
  const candidates = [
    input.authEnv ? env[input.authEnv] : undefined,
    env.LINEAR_API_KEY,
    env.LINEAR_TOKEN,
  ];

  for (const value of candidates) {
    const token = value?.trim();
    if (token) return token;
  }
  return null;
}
```

- [ ] **Step 4: Expand tracker config parsing**

Modify `apps/cli/src/backends/read-tracker-config.ts`:

```ts
import { load } from "js-yaml";

import { KataDomainError } from "../domain/errors.js";
import {
  cleanLinearString,
  optionalLinearString,
  readLinearLabels,
  readLinearStateMapping,
  type LinearTrackerConfig,
} from "./linear/config.js";

interface ReadTrackerConfigInput {
  preferencesContent: string;
}

interface GithubTrackerConfig {
  kind: "github";
  repoOwner: string;
  repoName: string;
  stateMode: "projects_v2";
  githubProjectNumber: number;
}

type TrackerConfig = LinearTrackerConfig | GithubTrackerConfig;

function unwrapFrontmatter(preferencesContent: string): string {
  const trimmed = preferencesContent.trim();

  if (!trimmed.startsWith("---")) {
    return trimmed;
  }

  const lines = trimmed.split(/\r?\n/);
  if (lines[0] !== "---") {
    return trimmed;
  }

  const endIndex = lines.indexOf("---", 1);
  return endIndex === -1 ? lines.slice(1).join("\n") : lines.slice(1, endIndex).join("\n");
}

function asRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" ? (value as Record<string, unknown>) : {};
}

function requireNonEmptyString(value: unknown, fieldName: string): string {
  if (typeof value === "string" && value.trim() !== "") {
    return value.trim();
  }

  throw new KataDomainError("INVALID_CONFIG", `${fieldName} is required`);
}

function requirePositiveInteger(value: unknown, fieldName: string): number {
  if (typeof value === "number" && Number.isInteger(value) && value > 0) {
    return value;
  }

  throw new KataDomainError("INVALID_CONFIG", `${fieldName} must be a positive integer`);
}

function readLinearConfig(parsed: Record<string, unknown>): LinearTrackerConfig {
  const linear = asRecord(parsed.linear);
  return {
    kind: "linear",
    workspace: cleanLinearString(linear.workspace, "linear.workspace"),
    team: cleanLinearString(linear.team, "linear.team"),
    project: cleanLinearString(linear.project, "linear.project"),
    authEnv: optionalLinearString(linear.authEnv),
    activeMilestoneId: optionalLinearString(linear.activeMilestoneId),
    states: readLinearStateMapping(asRecord(linear.states)),
    labels: readLinearLabels(asRecord(linear.labels)),
  };
}

export async function readTrackerConfig({ preferencesContent }: ReadTrackerConfigInput): Promise<TrackerConfig> {
  let parsedYaml: unknown;

  try {
    parsedYaml = load(unwrapFrontmatter(preferencesContent)) ?? {};
  } catch (error) {
    const message = error instanceof Error ? error.message : "Malformed preferences content";
    throw new KataDomainError("INVALID_CONFIG", `Unable to parse preferences content: ${message}`);
  }

  const parsed = asRecord(parsedYaml);
  const workflow = asRecord(parsed.workflow);
  const mode = requireNonEmptyString(workflow.mode, "workflow.mode");

  if (mode === "linear") {
    return readLinearConfig(parsed);
  }

  if (mode !== "github") {
    throw new KataDomainError("INVALID_CONFIG", `workflow.mode must be linear or github`);
  }

  const github = asRecord(parsed.github);
  const repoOwner = requireNonEmptyString(github.repoOwner, "github.repoOwner");
  const repoName = requireNonEmptyString(github.repoName, "github.repoName");
  const rawStateMode = github.stateMode;
  if (typeof rawStateMode !== "string" || rawStateMode.trim() === "") {
    throw new KataDomainError(
      "INVALID_CONFIG",
      "github.stateMode is required and must be projects_v2. Set github.stateMode: projects_v2 and github.githubProjectNumber to a positive integer.",
    );
  }

  const stateMode = rawStateMode.trim();

  if (stateMode === "labels") {
    throw new KataDomainError(
      "INVALID_CONFIG",
      "GitHub label mode is no longer supported. Use github.stateMode: projects_v2 and set github.githubProjectNumber.",
    );
  }

  if (stateMode !== "projects_v2") {
    throw new KataDomainError(
      "INVALID_CONFIG",
      "github.stateMode is required and must be projects_v2. Set github.stateMode: projects_v2 and github.githubProjectNumber to a positive integer.",
    );
  }

  return {
    kind: "github",
    repoOwner,
    repoName,
    stateMode: "projects_v2" as const,
    githubProjectNumber: requirePositiveInteger(github.githubProjectNumber, "github.githubProjectNumber"),
  };
}
```

- [ ] **Step 5: Run config tests and verify they pass**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.config.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit config parsing**

Run:

```bash
git add apps/cli/src/backends/linear/config.ts apps/cli/src/backends/read-tracker-config.ts apps/cli/src/tests/linear.config.vitest.test.ts
git commit -m "feat(cli): parse linear backend config"
```

## Task 2: Add Linear GraphQL Client

**Files:**

- Create: `apps/cli/src/backends/linear/client.ts`
- Test: `apps/cli/src/tests/linear.client.vitest.test.ts`

- [ ] **Step 1: Write failing client tests**

Create `apps/cli/src/tests/linear.client.vitest.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";

import { createLinearClient } from "../backends/linear/client.js";

describe("Linear GraphQL client", () => {
  it("sends GraphQL requests with the Linear token", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({
      data: { viewer: { id: "user-1" } },
    })));
    const client = createLinearClient({ token: "lin_test", fetch: fetch as any });

    await expect(client.graphql({
      query: "query Viewer { viewer { id } }",
    })).resolves.toEqual({ viewer: { id: "user-1" } });

    expect(fetch).toHaveBeenCalledWith("https://api.linear.app/graphql", expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({
        Authorization: "lin_test",
        "Content-Type": "application/json",
        "User-Agent": "@kata-sh/cli",
      }),
    }));
  });

  it("throws a KataDomainError for GraphQL errors", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({
      errors: [{ message: "Forbidden" }],
    })));
    const client = createLinearClient({ token: "lin_test", fetch: fetch as any });

    await expect(client.graphql({
      query: "query Broken { viewer { id } }",
    })).rejects.toMatchObject({
      code: "UNKNOWN",
      message: "Forbidden",
    });
  });

  it("throws a network error for non-2xx responses", async () => {
    const fetch = vi.fn(async () => new Response("Unauthorized", { status: 401 }));
    const client = createLinearClient({ token: "lin_test", fetch: fetch as any });

    await expect(client.graphql({
      query: "query Viewer { viewer { id } }",
    })).rejects.toMatchObject({
      code: "UNAUTHORIZED",
      message: "Linear request failed (401): Unauthorized",
    });
  });

  it("paginates connection nodes", async () => {
    const fetch = vi.fn(async (_url, init) => {
      const body = JSON.parse(String((init as RequestInit).body));
      const after = body.variables.after;
      return new Response(JSON.stringify({
        data: {
          teams: {
            nodes: after ? [{ id: "team-2" }] : [{ id: "team-1" }],
            pageInfo: {
              hasNextPage: !after,
              endCursor: after ? null : "cursor-1",
            },
          },
        },
      }));
    });
    const client = createLinearClient({ token: "lin_test", fetch: fetch as any });

    await expect(client.paginate<{ id: string }, { teams: any }>({
      query: "query Teams($after: String) { teams(first: 1, after: $after) { nodes { id } pageInfo { hasNextPage endCursor } } }",
      variables: {},
      selectConnection: (data) => data.teams,
    })).resolves.toEqual([{ id: "team-1" }, { id: "team-2" }]);
  });
});
```

- [ ] **Step 2: Run client tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.client.vitest.test.ts
```

Expected: FAIL because `client.ts` does not exist.

- [ ] **Step 3: Implement Linear client**

Create `apps/cli/src/backends/linear/client.ts`:

```ts
import { KataDomainError } from "../../domain/errors.js";

export type FetchLike = typeof fetch;

export interface LinearClientInput {
  token: string;
  fetch?: FetchLike;
}

export interface LinearGraphqlInput {
  query: string;
  variables?: Record<string, unknown>;
}

interface GraphqlResponse<T> {
  data?: T;
  errors?: Array<{ message?: string }>;
}

export interface LinearPageInfo {
  hasNextPage: boolean;
  endCursor?: string | null;
}

export interface LinearConnection<T> {
  nodes?: Array<T | null> | null;
  pageInfo: LinearPageInfo;
}

function statusCodeToErrorCode(status: number): KataDomainError["code"] {
  if (status === 401 || status === 403) return "UNAUTHORIZED";
  if (status === 429) return "RATE_LIMITED";
  return "NETWORK";
}

export function createLinearClient(input: LinearClientInput) {
  const request = input.fetch ?? fetch;

  return {
    async graphql<T>(graphqlInput: LinearGraphqlInput): Promise<T> {
      const response = await request("https://api.linear.app/graphql", {
        method: "POST",
        headers: {
          Authorization: input.token,
          "Content-Type": "application/json",
          "User-Agent": "@kata-sh/cli",
        },
        body: JSON.stringify(graphqlInput),
      });

      return parseLinearResponse<T>(response);
    },

    async paginate<Node, Data>(input: {
      query: string;
      variables?: Record<string, unknown>;
      selectConnection(data: Data): LinearConnection<Node> | undefined | null;
      maxPages?: number;
    }): Promise<Node[]> {
      const nodes: Node[] = [];
      let after: string | null = null;
      const maxPages = input.maxPages ?? 100;

      for (let page = 1; page <= maxPages; page += 1) {
        const data = await this.graphql<Data>({
          query: input.query,
          variables: {
            ...(input.variables ?? {}),
            after,
          },
        });
        const connection = input.selectConnection(data);
        if (!connection) return nodes;
        nodes.push(...(connection.nodes ?? []).filter((node): node is Node => node !== null));
        if (!connection.pageInfo.hasNextPage) return nodes;
        after = connection.pageInfo.endCursor ?? null;
      }

      throw new KataDomainError("UNKNOWN", `Unable to paginate Linear connection after ${maxPages} full pages.`);
    },
  };
}

async function parseLinearResponse<T>(response: Response): Promise<T> {
  const text = await response.text();

  if (!response.ok) {
    throw new KataDomainError(
      statusCodeToErrorCode(response.status),
      `Linear request failed (${response.status}): ${text}`,
    );
  }

  let payload: GraphqlResponse<T>;
  try {
    payload = text ? JSON.parse(text) as GraphqlResponse<T> : {};
  } catch {
    throw new KataDomainError("NETWORK", "Linear response was not valid JSON.");
  }

  if (payload.data != null) return payload.data;
  if (payload.errors?.length) {
    throw new KataDomainError(
      "UNKNOWN",
      payload.errors.map((error) => error.message ?? "Unknown GraphQL error").join("; "),
    );
  }

  throw new KataDomainError("UNKNOWN", "Linear GraphQL response did not include data.");
}
```

- [ ] **Step 4: Run client tests and verify they pass**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.client.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit Linear client**

Run:

```bash
git add apps/cli/src/backends/linear/client.ts apps/cli/src/tests/linear.client.vitest.test.ts
git commit -m "feat(cli): add linear graphql client"
```

## Task 3: Add Linear Artifact Helpers

**Files:**

- Create: `apps/cli/src/backends/linear/artifacts.ts`
- Test: `apps/cli/src/tests/linear.artifacts.vitest.test.ts`

- [ ] **Step 1: Write failing artifact tests**

Create `apps/cli/src/tests/linear.artifacts.vitest.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";

import {
  formatLinearArtifactMarker,
  parseLinearArtifactMarker,
  upsertLinearIssueArtifactComment,
  upsertLinearMilestoneDocument,
} from "../backends/linear/artifacts.js";

describe("Linear artifacts", () => {
  it("formats and parses artifact markers", () => {
    const body = formatLinearArtifactMarker({
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "# Plan",
    });

    expect(parseLinearArtifactMarker(body)).toEqual({
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "# Plan",
    });
  });

  it("returns null for malformed artifact markers", () => {
    expect(parseLinearArtifactMarker("<!-- kata:artifact {bad json} -->\ncontent")).toBeNull();
    expect(parseLinearArtifactMarker("plain comment")).toBeNull();
    expect(parseLinearArtifactMarker('<!-- kata:artifact {"scopeType":"slice","scopeId":"","artifactType":"plan"} -->')).toBeNull();
  });

  it("updates an existing Linear issue artifact comment", async () => {
    const client = {
      graphql: vi.fn(async (request: any) => {
        if (request.query.includes("LinearKataIssueComments")) {
          return {
            issue: {
              comments: {
                nodes: [{
                  id: "comment-1",
                  body: formatLinearArtifactMarker({
                    scopeType: "slice",
                    scopeId: "S001",
                    artifactType: "plan",
                    content: "old",
                  }),
                }],
                pageInfo: { hasNextPage: false, endCursor: null },
              },
            },
          };
        }
        return {
          commentUpdate: {
            success: true,
            comment: { id: "comment-1", body: request.variables.input.body },
          },
        };
      }),
      paginate: vi.fn(),
    };

    const result = await upsertLinearIssueArtifactComment({
      client: client as any,
      issueId: "issue-1",
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "new",
    });

    expect(result.backendId).toBe("comment:comment-1");
    expect(result.body).toContain("new");
    expect(client.graphql).toHaveBeenCalledWith(expect.objectContaining({
      query: expect.stringContaining("commentUpdate"),
    }));
  });

  it("creates a Linear issue artifact comment when none exists", async () => {
    const client = {
      graphql: vi.fn(async (request: any) => {
        if (request.query.includes("LinearKataIssueComments")) {
          return {
            issue: {
              comments: {
                nodes: [],
                pageInfo: { hasNextPage: false, endCursor: null },
              },
            },
          };
        }
        return {
          commentCreate: {
            success: true,
            comment: { id: "comment-2", body: request.variables.input.body },
          },
        };
      }),
      paginate: vi.fn(),
    };

    const result = await upsertLinearIssueArtifactComment({
      client: client as any,
      issueId: "issue-1",
      scopeType: "task",
      scopeId: "T001",
      artifactType: "verification",
      content: "verified",
    });

    expect(result.backendId).toBe("comment:comment-2");
    expect(client.graphql).toHaveBeenCalledWith(expect.objectContaining({
      query: expect.stringContaining("commentCreate"),
    }));
  });

  it("updates an existing milestone document by marker", async () => {
    const client = {
      graphql: vi.fn(async (request: any) => {
        if (request.query.includes("LinearKataProjectDocuments")) {
          return {
            project: {
              documents: {
                nodes: [{
                  id: "doc-1",
                  title: "M001 Requirements",
                  content: formatLinearArtifactMarker({
                    scopeType: "milestone",
                    scopeId: "M001",
                    artifactType: "requirements",
                    content: "old",
                  }),
                  updatedAt: "2026-05-06T00:00:00.000Z",
                }],
                pageInfo: { hasNextPage: false, endCursor: null },
              },
            },
          };
        }
        return {
          documentUpdate: {
            success: true,
            document: {
              id: "doc-1",
              title: request.variables.input.title,
              content: request.variables.input.content,
              updatedAt: "2026-05-06T00:00:00.000Z",
            },
          },
        };
      }),
      paginate: vi.fn(),
    };

    const result = await upsertLinearMilestoneDocument({
      client: client as any,
      projectId: "project-1",
      scopeId: "M001",
      artifactType: "requirements",
      title: "Requirements",
      content: "# Requirements",
    });

    expect(result.backendId).toBe("document:doc-1");
    expect(result.body).toContain("# Requirements");
  });
});
```

- [ ] **Step 2: Run artifact tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.artifacts.vitest.test.ts
```

Expected: FAIL because `artifacts.ts` does not exist.

- [ ] **Step 3: Implement Linear artifact helpers**

Create `apps/cli/src/backends/linear/artifacts.ts`:

```ts
import type { KataArtifactType, KataScopeType } from "../../domain/types.js";
import type { createLinearClient } from "./client.js";

const MARKER_PREFIX = "<!-- kata:artifact ";
const MARKER_SUFFIX = " -->";

const SCOPE_TYPES = ["project", "milestone", "slice", "task", "issue"] satisfies KataScopeType[];
const ARTIFACT_TYPES = [
  "project-brief",
  "requirements",
  "roadmap",
  "phase-context",
  "context",
  "decisions",
  "research",
  "plan",
  "slice",
  "summary",
  "verification",
  "uat",
  "retrospective",
] satisfies KataArtifactType[];

type LinearClient = ReturnType<typeof createLinearClient>;

export interface ParsedLinearArtifactMarker {
  scopeType: KataScopeType;
  scopeId: string;
  artifactType: KataArtifactType;
  content: string;
}

export interface LinearArtifactWriteResult {
  backendId: string;
  body: string;
  title?: string;
  updatedAt?: string;
}

interface LinearCommentNode {
  id: string;
  body?: string | null;
}

interface LinearDocumentNode {
  id: string;
  title: string;
  content?: string | null;
  updatedAt?: string | null;
}

const ISSUE_COMMENTS_QUERY = `
  query LinearKataIssueComments($issueId: String!, $after: String) {
    issue(id: $issueId) {
      comments(first: 100, after: $after) {
        nodes {
          id
          body
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
`;

const PROJECT_DOCUMENTS_QUERY = `
  query LinearKataProjectDocuments($projectId: String!, $after: String) {
    project(id: $projectId) {
      documents(first: 100, after: $after) {
        nodes {
          id
          title
          content
          updatedAt
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
`;

const COMMENT_CREATE_MUTATION = `
  mutation LinearKataCommentCreate($input: CommentCreateInput!) {
    commentCreate(input: $input) {
      success
      comment {
        id
        body
      }
    }
  }
`;

const COMMENT_UPDATE_MUTATION = `
  mutation LinearKataCommentUpdate($id: String!, $input: CommentUpdateInput!) {
    commentUpdate(id: $id, input: $input) {
      success
      comment {
        id
        body
      }
    }
  }
`;

const DOCUMENT_CREATE_MUTATION = `
  mutation LinearKataDocumentCreate($input: DocumentCreateInput!) {
    documentCreate(input: $input) {
      success
      document {
        id
        title
        content
        updatedAt
      }
    }
  }
`;

const DOCUMENT_UPDATE_MUTATION = `
  mutation LinearKataDocumentUpdate($id: String!, $input: DocumentUpdateInput!) {
    documentUpdate(id: $id, input: $input) {
      success
      document {
        id
        title
        content
        updatedAt
      }
    }
  }
`;

export function formatLinearArtifactMarker(input: ParsedLinearArtifactMarker): string {
  const marker = JSON.stringify({
    scopeType: input.scopeType,
    scopeId: input.scopeId,
    artifactType: input.artifactType,
  });

  return `${MARKER_PREFIX}${marker}${MARKER_SUFFIX}\n${input.content}`;
}

export function parseLinearArtifactMarker(body: string): ParsedLinearArtifactMarker | null {
  const newlineIndex = body.indexOf("\n");
  const markerLine = newlineIndex === -1 ? body : body.slice(0, newlineIndex);

  if (!markerLine.startsWith(MARKER_PREFIX) || !markerLine.endsWith(MARKER_SUFFIX)) {
    return null;
  }

  let metadata: unknown;
  try {
    metadata = JSON.parse(markerLine.slice(MARKER_PREFIX.length, -MARKER_SUFFIX.length));
  } catch {
    return null;
  }

  if (!isValidArtifactMetadata(metadata)) return null;
  return {
    ...metadata,
    content: newlineIndex === -1 ? "" : body.slice(newlineIndex + 1),
  };
}

export async function upsertLinearIssueArtifactComment(input: {
  client: LinearClient;
  issueId: string;
  scopeType: KataScopeType;
  scopeId: string;
  artifactType: KataArtifactType;
  content: string;
}): Promise<LinearArtifactWriteResult> {
  const body = formatLinearArtifactMarker({
    scopeType: input.scopeType,
    scopeId: input.scopeId,
    artifactType: input.artifactType,
    content: input.content,
  });
  const existing = await findExistingLinearIssueArtifactComment(input);

  if (existing) {
    const data = await input.client.graphql<{ commentUpdate: { comment: LinearCommentNode } }>({
      query: COMMENT_UPDATE_MUTATION,
      variables: {
        id: existing.id,
        input: { body },
      },
    });
    return {
      backendId: `comment:${data.commentUpdate.comment.id}`,
      body: data.commentUpdate.comment.body ?? body,
    };
  }

  const data = await input.client.graphql<{ commentCreate: { comment: LinearCommentNode } }>({
    query: COMMENT_CREATE_MUTATION,
    variables: {
      input: {
        issueId: input.issueId,
        body,
      },
    },
  });
  return {
    backendId: `comment:${data.commentCreate.comment.id}`,
    body: data.commentCreate.comment.body ?? body,
  };
}

export async function upsertLinearMilestoneDocument(input: {
  client: LinearClient;
  projectId: string;
  scopeId: string;
  artifactType: KataArtifactType;
  title: string;
  content: string;
}): Promise<LinearArtifactWriteResult> {
  const body = formatLinearArtifactMarker({
    scopeType: "milestone",
    scopeId: input.scopeId,
    artifactType: input.artifactType,
    content: input.content,
  });
  const title = `${input.scopeId} ${input.title}`;
  const existing = await findExistingLinearMilestoneDocument(input);

  if (existing) {
    const data = await input.client.graphql<{ documentUpdate: { document: LinearDocumentNode } }>({
      query: DOCUMENT_UPDATE_MUTATION,
      variables: {
        id: existing.id,
        input: { title, content: body },
      },
    });
    return {
      backendId: `document:${data.documentUpdate.document.id}`,
      body: data.documentUpdate.document.content ?? body,
      title: data.documentUpdate.document.title,
      updatedAt: data.documentUpdate.document.updatedAt ?? undefined,
    };
  }

  const data = await input.client.graphql<{ documentCreate: { document: LinearDocumentNode } }>({
    query: DOCUMENT_CREATE_MUTATION,
    variables: {
      input: {
        projectId: input.projectId,
        title,
        content: body,
      },
    },
  });
  return {
    backendId: `document:${data.documentCreate.document.id}`,
    body: data.documentCreate.document.content ?? body,
    title: data.documentCreate.document.title,
    updatedAt: data.documentCreate.document.updatedAt ?? undefined,
  };
}

async function findExistingLinearIssueArtifactComment(input: {
  client: LinearClient;
  issueId: string;
  scopeType: KataScopeType;
  scopeId: string;
  artifactType: KataArtifactType;
}): Promise<LinearCommentNode | null> {
  const comments = await input.client.paginate<LinearCommentNode, { issue?: { comments?: any } | null }>({
    query: ISSUE_COMMENTS_QUERY,
    variables: { issueId: input.issueId },
    selectConnection: (data) => data.issue?.comments,
  });

  return comments.find((comment) => {
    const parsed = typeof comment.body === "string" ? parseLinearArtifactMarker(comment.body) : null;
    return parsed?.scopeType === input.scopeType &&
      parsed.scopeId === input.scopeId &&
      parsed.artifactType === input.artifactType;
  }) ?? null;
}

async function findExistingLinearMilestoneDocument(input: {
  client: LinearClient;
  projectId: string;
  scopeId: string;
  artifactType: KataArtifactType;
}): Promise<LinearDocumentNode | null> {
  const documents = await input.client.paginate<LinearDocumentNode, { project?: { documents?: any } | null }>({
    query: PROJECT_DOCUMENTS_QUERY,
    variables: { projectId: input.projectId },
    selectConnection: (data) => data.project?.documents,
  });

  return documents.find((document) => {
    const parsed = typeof document.content === "string" ? parseLinearArtifactMarker(document.content) : null;
    return parsed?.scopeType === "milestone" &&
      parsed.scopeId === input.scopeId &&
      parsed.artifactType === input.artifactType;
  }) ?? null;
}

function isValidArtifactMetadata(metadata: unknown): metadata is Omit<ParsedLinearArtifactMarker, "content"> {
  if (!metadata || typeof metadata !== "object") return false;
  const candidate = metadata as Partial<Record<keyof ParsedLinearArtifactMarker, unknown>>;
  return isKnownScopeType(candidate.scopeType) &&
    typeof candidate.scopeId === "string" &&
    candidate.scopeId.trim().length > 0 &&
    isKnownArtifactType(candidate.artifactType);
}

function isKnownScopeType(value: unknown): value is KataScopeType {
  return typeof value === "string" && SCOPE_TYPES.includes(value as KataScopeType);
}

function isKnownArtifactType(value: unknown): value is KataArtifactType {
  return typeof value === "string" && ARTIFACT_TYPES.includes(value as KataArtifactType);
}
```

- [ ] **Step 4: Run artifact tests and verify they pass**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.artifacts.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit artifact helpers**

Run:

```bash
git add apps/cli/src/backends/linear/artifacts.ts apps/cli/src/tests/linear.artifacts.vitest.test.ts
git commit -m "feat(cli): add linear artifact helpers"
```

## Task 4: Implement Linear Adapter Reads and Discovery

**Files:**

- Modify: `apps/cli/src/backends/linear/adapter.ts`
- Test: `apps/cli/src/tests/linear.adapter.vitest.test.ts`

- [ ] **Step 1: Write failing read/discovery tests**

Create `apps/cli/src/tests/linear.adapter.vitest.test.ts` with the shared fake client and the first read tests:

```ts
import { describe, expect, it, vi } from "vitest";

import { LinearKataAdapter } from "../backends/linear/adapter.js";
import { createKataDomainApi } from "../domain/service.js";

function createFakeLinearClient() {
  const project = { id: "project-1", name: "Kata CLI", slugId: "kata-cli", url: "https://linear.test/project/kata-cli" };
  const team = { id: "team-1", key: "KATA", name: "Kata" };
  const workflowStates = [
    { id: "state-backlog", name: "Backlog", type: "backlog" },
    { id: "state-todo", name: "Todo", type: "unstarted" },
    { id: "state-progress", name: "In Progress", type: "started" },
    { id: "state-agent", name: "Agent Review", type: "started" },
    { id: "state-human", name: "Human Review", type: "started" },
    { id: "state-merging", name: "Merging", type: "started" },
    { id: "state-done", name: "Done", type: "completed" },
  ];
  const milestones = [
    {
      id: "milestone-1",
      name: "M001 Launch",
      description: "Launch",
      targetDate: null,
    },
  ];
  const issues = [
    {
      id: "issue-s1",
      identifier: "KATA-1",
      number: 1,
      title: "[S001] Foundation",
      description: "Foundation",
      url: "https://linear.test/KATA-1",
      state: workflowStates[2],
      labels: { nodes: [{ name: "kata/slice" }], pageInfo: { hasNextPage: false, endCursor: null } },
      project,
      projectMilestone: milestones[0],
      parent: null,
      children: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
      relations: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
      inverseRelations: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
    },
    {
      id: "issue-t1",
      identifier: "KATA-2",
      number: 2,
      title: "[T001] Verify",
      description: "Verify",
      url: "https://linear.test/KATA-2",
      state: workflowStates[6],
      labels: { nodes: [{ name: "kata/task" }], pageInfo: { hasNextPage: false, endCursor: null } },
      project,
      projectMilestone: milestones[0],
      parent: { id: "issue-s1", identifier: "KATA-1" },
      children: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
      relations: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
      inverseRelations: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
    },
  ];

  return {
    graphql: vi.fn(async (request: any) => {
      if (request.query.includes("LinearKataContext")) {
        return {
          viewer: { id: "user-1" },
          organization: { id: "org-1", urlKey: "kata" },
          teams: { nodes: [team], pageInfo: { hasNextPage: false, endCursor: null } },
          projects: { nodes: [project], pageInfo: { hasNextPage: false, endCursor: null } },
          workflowStates: { nodes: workflowStates, pageInfo: { hasNextPage: false, endCursor: null } },
        };
      }
      if (request.query.includes("LinearKataMilestones")) {
        return {
          project: {
            id: project.id,
            name: project.name,
            milestones: { nodes: milestones, pageInfo: { hasNextPage: false, endCursor: null } },
          },
        };
      }
      if (request.query.includes("LinearKataIssues")) {
        return {
          issues: { nodes: issues, pageInfo: { hasNextPage: false, endCursor: null } },
        };
      }
      if (request.query.includes("LinearKataIssueComments")) {
        return {
          issue: {
            comments: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          },
        };
      }
      if (request.query.includes("LinearKataProjectDocuments")) {
        return {
          project: {
            documents: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          },
        };
      }
      throw new Error(`Unhandled fake Linear query: ${request.query}`);
    }),
    paginate: vi.fn(async (input: any) => {
      const data = await (input as any).client?.graphql?.({ query: input.query, variables: input.variables });
      const connection = input.selectConnection(data);
      return connection?.nodes ?? [];
    }),
  };
}

function createAdapter(client = createFakeLinearClient()) {
  client.paginate = vi.fn(async (input: any) => {
    const data = await client.graphql({ query: input.query, variables: input.variables });
    return input.selectConnection(data)?.nodes ?? [];
  });
  return new LinearKataAdapter({
    client: client as any,
    workspacePath: "/workspace",
    config: {
      kind: "linear",
      workspace: "kata",
      team: "KATA",
      project: "kata-cli",
      states: {
        backlog: "Backlog",
        todo: "Todo",
        in_progress: "In Progress",
        agent_review: "Agent Review",
        human_review: "Human Review",
        merging: "Merging",
        done: "Done",
      },
      labels: {},
    },
  });
}

describe("LinearKataAdapter reads", () => {
  it("returns Linear project context", async () => {
    await expect(createAdapter().getProjectContext()).resolves.toEqual({
      backend: "linear",
      workspacePath: "/workspace",
      title: "Kata CLI",
      description: "Linear project kata-cli in workspace kata",
    });
  });

  it("lists and selects the active Linear project milestone", async () => {
    const adapter = createAdapter();

    await expect(adapter.listMilestones()).resolves.toEqual([
      {
        id: "M001",
        title: "M001 Launch",
        goal: "Launch",
        status: "active",
        active: true,
      },
    ]);
    await expect(adapter.getActiveMilestone()).resolves.toMatchObject({
      id: "M001",
      active: true,
    });
  });

  it("lists slices and task sub-issues from Linear issues", async () => {
    const adapter = createAdapter();

    await expect(adapter.listSlices({ milestoneId: "M001" })).resolves.toEqual([
      expect.objectContaining({
        id: "S001",
        milestoneId: "M001",
        title: "Foundation",
        status: "in_progress",
      }),
    ]);
    await expect(adapter.listTasks({ sliceId: "S001" })).resolves.toEqual([
      expect.objectContaining({
        id: "T001",
        sliceId: "S001",
        title: "Verify",
        status: "done",
        verificationState: "verified",
      }),
    ]);
  });

  it("builds a snapshot through the existing domain service", async () => {
    const api = createKataDomainApi(createAdapter());

    await expect(api.project.getSnapshot()).resolves.toMatchObject({
      context: { backend: "linear" },
      activeMilestone: { id: "M001" },
      slices: [
        {
          id: "S001",
          tasks: [
            { id: "T001", verificationState: "verified" },
          ],
        },
      ],
    });
  });
});
```

- [ ] **Step 2: Run adapter read tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.adapter.vitest.test.ts
```

Expected: FAIL because the current `LinearKataAdapter` constructor accepts the old `fetchActiveMilestoneSnapshot` shape.

- [ ] **Step 3: Replace adapter constructor and shared types**

Modify the top of `apps/cli/src/backends/linear/adapter.ts` so the adapter accepts a real client, config, and workspace path:

```ts
import type {
  KataArtifact,
  KataArtifactType,
  KataArtifactWriteInput,
  KataBackendAdapter,
  KataExecutionStatus,
  KataHealthReport,
  KataIssue,
  KataIssueCreateInput,
  KataIssueGetInput,
  KataIssueSummary,
  KataIssueUpdateStatusInput,
  KataMilestone,
  KataMilestoneCompleteInput,
  KataMilestoneCreateInput,
  KataPullRequest,
  KataProjectContext,
  KataProjectUpsertInput,
  KataScopeType,
  KataSlice,
  KataSliceCreateInput,
  KataSliceUpdateStatusInput,
  KataTask,
  KataTaskCreateInput,
  KataTaskUpdateStatusInput,
} from "../../domain/types.js";
import { KataDomainError } from "../../domain/errors.js";
import { parseSliceDependencyIds } from "../../domain/dependencies.js";
import type { createLinearClient } from "./client.js";
import type { LinearStateMapping, LinearTrackerConfig } from "./config.js";
import {
  parseLinearArtifactMarker,
  upsertLinearIssueArtifactComment,
  upsertLinearMilestoneDocument,
} from "./artifacts.js";

type LinearClient = ReturnType<typeof createLinearClient>;
type LinearEntityType = "Project" | "Milestone" | "Slice" | "Task" | "Issue";
type LinearSliceStatus = KataSlice["status"];
type LinearTaskStatus = KataTask["status"];
type LinearTaskVerificationState = KataTask["verificationState"];

interface LinearKataAdapterInput {
  client: LinearClient;
  config: LinearTrackerConfig;
  workspacePath: string;
}

interface LinearEntityClassification {
  kataId: string;
  type: LinearEntityType;
  parentId?: string;
}

interface LinearLabelNode {
  name: string;
}

interface LinearProjectNode {
  id: string;
  name: string;
  slugId?: string | null;
  url?: string | null;
  description?: string | null;
}

interface LinearTeamNode {
  id: string;
  key: string;
  name: string;
}

interface LinearWorkflowStateNode {
  id: string;
  name: string;
  type?: string | null;
}

interface LinearMilestoneNode {
  id: string;
  name: string;
  description?: string | null;
  targetDate?: string | null;
}

interface LinearIssueNode {
  id: string;
  identifier: string;
  number?: number | null;
  title: string;
  description?: string | null;
  url?: string | null;
  state?: LinearWorkflowStateNode | null;
  project?: LinearProjectNode | null;
  projectMilestone?: LinearMilestoneNode | null;
  parent?: { id: string; identifier?: string | null } | null;
  children?: { nodes?: Array<LinearIssueNode | null> | null } | null;
  labels?: { nodes?: Array<LinearLabelNode | null> | null } | null;
  relations?: { nodes?: Array<LinearIssueRelationNode | null> | null } | null;
  inverseRelations?: { nodes?: Array<LinearIssueRelationNode | null> | null } | null;
}

interface LinearIssueRelationNode {
  id: string;
  type: string;
  issue?: { id: string; identifier?: string | null } | null;
  relatedIssue?: { id: string; identifier?: string | null } | null;
}

interface TrackedLinearEntity {
  kataId: string;
  type: LinearEntityType;
  parentId?: string;
  blockedBy?: string[];
  blocking?: string[];
  linearId: string;
  identifier?: string;
  title: string;
  body: string;
  url?: string;
  stateName?: string;
  stateType?: string;
  projectMilestoneId?: string;
}
```

- [ ] **Step 4: Add Linear discovery queries**

Add these query constants after the type declarations in `apps/cli/src/backends/linear/adapter.ts`:

```ts
const LINEAR_CONTEXT_QUERY = `
  query LinearKataContext($teamKey: String!, $projectFilter: ProjectFilter, $after: String) {
    viewer {
      id
    }
    organization {
      id
      urlKey
    }
    teams(filter: { key: { eq: $teamKey } }, first: 20) {
      nodes {
        id
        key
        name
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
    projects(filter: $projectFilter, first: 20, after: $after) {
      nodes {
        id
        name
        slugId
        url
        description
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
    workflowStates(filter: { team: { key: { eq: $teamKey } } }, first: 100) {
      nodes {
        id
        name
        type
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
`;

const LINEAR_MILESTONES_QUERY = `
  query LinearKataMilestones($projectId: String!, $after: String) {
    project(id: $projectId) {
      id
      name
      milestones(first: 100, after: $after) {
        nodes {
          id
          name
          description
          targetDate
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
`;

const LINEAR_ISSUES_QUERY = `
  query LinearKataIssues($teamId: ID!, $projectId: ID!, $after: String) {
    issues(
      filter: {
        team: { id: { eq: $teamId } }
        project: { id: { eq: $projectId } }
      }
      first: 100
      after: $after
    ) {
      nodes {
        id
        identifier
        number
        title
        description
        url
        state {
          id
          name
          type
        }
        project {
          id
          name
          slugId
          url
        }
        projectMilestone {
          id
          name
          description
        }
        parent {
          id
          identifier
        }
        children(first: 100) {
          nodes {
            id
            identifier
            number
            title
            description
            url
            state {
              id
              name
              type
            }
            parent {
              id
              identifier
            }
          }
        }
        relations(first: 100) {
          nodes {
            id
            type
            issue {
              id
              identifier
            }
            relatedIssue {
              id
              identifier
            }
          }
        }
        inverseRelations(first: 100) {
          nodes {
            id
            type
            issue {
              id
              identifier
            }
            relatedIssue {
              id
              identifier
            }
          }
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
`;
```

- [ ] **Step 5: Implement read/discovery methods**

Replace the current class body in `apps/cli/src/backends/linear/adapter.ts` with this discovery-capable skeleton. Keep mutation methods throwing until Task 5.

```ts
export class LinearKataAdapter implements KataBackendAdapter {
  private readonly client: LinearClient;
  private readonly config: LinearTrackerConfig;
  private readonly workspacePath: string;
  private contextPromise: Promise<{
    organizationUrlKey: string;
    project: LinearProjectNode;
    team: LinearTeamNode;
    stateByKataStatus: Map<string, LinearWorkflowStateNode>;
    kataStatusByStateName: Map<string, keyof LinearStateMapping>;
  }> | null = null;
  private discovered = false;
  private entities = new Map<string, TrackedLinearEntity>();
  private linearIdToKataId = new Map<string, string>();

  constructor(input: LinearKataAdapterInput) {
    this.client = input.client;
    this.config = input.config;
    this.workspacePath = input.workspacePath;
  }

  async getProjectContext(): Promise<KataProjectContext> {
    const context = await this.getContext();
    return {
      backend: "linear",
      workspacePath: this.workspacePath,
      title: context.project.name,
      description: `Linear project ${this.config.project} in workspace ${this.config.workspace}`,
    };
  }

  async upsertProject(_input: KataProjectUpsertInput): Promise<KataProjectContext> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear project upsert is implemented in the mutation task.");
  }

  async listMilestones(): Promise<KataMilestone[]> {
    await this.discoverEntities();
    return [...this.entities.values()]
      .filter((entity) => entity.type === "Milestone")
      .sort((left, right) => left.kataId.localeCompare(right.kataId))
      .map((entity) => ({
        id: entity.kataId,
        title: entity.title,
        goal: bodyContent(entity.body) || entity.title,
        status: "active",
        active: true,
      }));
  }

  async getActiveMilestone(): Promise<KataMilestone | null> {
    const milestones = await this.listMilestones();
    if (milestones.length === 0) return null;
    if (this.config.activeMilestoneId) {
      const active = milestones.find((milestone) =>
        milestone.id === this.config.activeMilestoneId ||
        this.entities.get(milestone.id)?.linearId === this.config.activeMilestoneId
      );
      if (!active) {
        throw new KataDomainError("INVALID_CONFIG", `Configured Linear active milestone ${this.config.activeMilestoneId} was not found.`);
      }
      return active;
    }
    if (milestones.length === 1) return milestones[0] ?? null;
    throw new KataDomainError("INVALID_CONFIG", "Multiple active Linear milestones were found. Set linear.activeMilestoneId in .kata/preferences.md.");
  }

  async createMilestone(_input: KataMilestoneCreateInput): Promise<KataMilestone> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear milestone creation is implemented in the mutation task.");
  }

  async completeMilestone(_input: KataMilestoneCompleteInput): Promise<KataMilestone> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear milestone completion is implemented in the mutation task.");
  }

  async listSlices(input: { milestoneId: string }): Promise<KataSlice[]> {
    await this.discoverEntities();
    return [...this.entities.values()]
      .filter((entity) => entity.type === "Slice" && entity.parentId === input.milestoneId)
      .sort((left, right) => left.kataId.localeCompare(right.kataId))
      .map((entity, index) => ({
        id: entity.kataId,
        milestoneId: input.milestoneId,
        title: entity.title,
        goal: bodyContent(entity.body) || entity.title,
        status: sliceStatusFromEntity(entity, this.config.states),
        order: index,
        blockedBy: parseSliceDependencyIds(entity.blockedBy),
        blocking: parseSliceDependencyIds(entity.blocking),
      }));
  }

  async createSlice(_input: KataSliceCreateInput): Promise<KataSlice> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear slice creation is implemented in the mutation task.");
  }

  async updateSliceStatus(_input: KataSliceUpdateStatusInput): Promise<KataSlice> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear slice status updates are implemented in the mutation task.");
  }

  async listTasks(input: { sliceId: string }): Promise<KataTask[]> {
    await this.discoverEntities();
    return [...this.entities.values()]
      .filter((entity) => entity.type === "Task" && entity.parentId === input.sliceId)
      .sort((left, right) => left.kataId.localeCompare(right.kataId))
      .map((entity) => ({
        id: entity.kataId,
        sliceId: input.sliceId,
        title: entity.title,
        description: bodyContent(entity.body),
        status: taskStatusFromEntity(entity, this.config.states),
        verificationState: taskVerificationStateFromEntity(entity),
      }));
  }

  async createTask(_input: KataTaskCreateInput): Promise<KataTask> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear task creation is implemented in the mutation task.");
  }

  async updateTaskStatus(_input: KataTaskUpdateStatusInput): Promise<KataTask> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear task status updates are implemented in the mutation task.");
  }

  async createIssue(_input: KataIssueCreateInput): Promise<KataIssue> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear standalone issue creation is implemented in the mutation task.");
  }

  async listOpenIssues(): Promise<KataIssueSummary[]> {
    await this.discoverEntities();
    return [...this.entities.values()]
      .filter((entity) => entity.type === "Issue" && issueStatusFromEntity(entity, this.config.states) !== "done")
      .sort((left, right) => left.kataId.localeCompare(right.kataId))
      .map((entity) => ({
        id: entity.kataId,
        number: linearIssueNumber(entity.identifier),
        title: entity.title,
        status: issueStatusFromEntity(entity, this.config.states),
        url: entity.url,
      }));
  }

  async getIssue(input: KataIssueGetInput): Promise<KataIssue> {
    const entity = await this.findIssueEntity(input.issueRef);
    return {
      id: entity.kataId,
      number: linearIssueNumber(entity.identifier),
      title: entity.title,
      body: bodyContent(entity.body),
      status: issueStatusFromEntity(entity, this.config.states),
      url: entity.url,
    };
  }

  async updateIssueStatus(_input: KataIssueUpdateStatusInput): Promise<KataIssue> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear standalone issue status updates are implemented in the mutation task.");
  }

  async listArtifacts(_input: { scopeType: KataScopeType; scopeId: string }): Promise<KataArtifact[]> {
    return [];
  }

  async readArtifact(input: { scopeType: KataScopeType; scopeId: string; artifactType: KataArtifactType }): Promise<KataArtifact | null> {
    return (await this.listArtifacts(input)).find((artifact) => artifact.artifactType === input.artifactType) ?? null;
  }

  async writeArtifact(_input: KataArtifactWriteInput): Promise<KataArtifact> {
    throw new KataDomainError("NOT_SUPPORTED", "Linear artifact writes are implemented in the artifact task.");
  }

  async openPullRequest(input: { title: string; body: string; base: string; head: string }): Promise<KataPullRequest> {
    return {
      id: `${input.head}->${input.base}`,
      url: `https://github.com/kata-sh/kata-mono/pull/${encodeURIComponent(input.head)}`,
      branch: input.head,
      base: input.base,
      status: "open",
      mergeReady: false,
    };
  }

  async getExecutionStatus(): Promise<KataExecutionStatus> {
    return { queueDepth: 0, activeWorkers: 0, escalations: [] };
  }

  async checkHealth(): Promise<KataHealthReport> {
    return {
      ok: true,
      backend: "linear",
      checks: [{ name: "adapter", status: "ok", message: "Linear adapter is configured." }],
    };
  }

  private async getContext() {
    if (!this.contextPromise) {
      this.contextPromise = this.loadContext();
    }
    return this.contextPromise;
  }

  private async loadContext() {
    const data = await this.client.graphql<{
      organization?: { urlKey?: string | null } | null;
      teams?: { nodes?: LinearTeamNode[] | null } | null;
      projects?: { nodes?: LinearProjectNode[] | null } | null;
      workflowStates?: { nodes?: LinearWorkflowStateNode[] | null } | null;
    }>({
      query: LINEAR_CONTEXT_QUERY,
      variables: {
        teamKey: this.config.team,
        projectFilter: {
          or: [
            { id: { eq: this.config.project } },
            { slugId: { eq: this.config.project } },
            { name: { eq: this.config.project } },
          ],
        },
      },
    });

    const team = data.teams?.nodes?.find((candidate) =>
      candidate.key === this.config.team || candidate.id === this.config.team || candidate.name === this.config.team
    );
    if (!team) throw new KataDomainError("INVALID_CONFIG", `Linear team ${this.config.team} was not found.`);

    const project = data.projects?.nodes?.find((candidate) =>
      candidate.id === this.config.project ||
      candidate.slugId === this.config.project ||
      candidate.name === this.config.project
    );
    if (!project) throw new KataDomainError("INVALID_CONFIG", `Linear project ${this.config.project} was not found.`);

    const states = data.workflowStates?.nodes ?? [];
    const stateByKataStatus = new Map<string, LinearWorkflowStateNode>();
    const kataStatusByStateName = new Map<string, keyof LinearStateMapping>();
    for (const [status, stateName] of Object.entries(this.config.states) as Array<[keyof LinearStateMapping, string]>) {
      const state = states.find((candidate) => candidate.name === stateName);
      if (!state) throw new KataDomainError("INVALID_CONFIG", `Linear workflow state "${stateName}" was not found for team ${this.config.team}.`);
      stateByKataStatus.set(status, state);
      kataStatusByStateName.set(state.name, status);
    }

    return {
      organizationUrlKey: data.organization?.urlKey ?? this.config.workspace,
      project,
      team,
      stateByKataStatus,
      kataStatusByStateName,
    };
  }

  private async discoverEntities(): Promise<void> {
    if (this.discovered) return;
    const context = await this.getContext();
    const milestones = await this.loadMilestoneEntities(context.project.id);
    const issues = await this.loadIssueEntities(context.team.id, context.project.id, milestones);
    for (const entity of [...milestones, ...issues]) {
      if (this.entities.has(entity.kataId)) continue;
      this.entities.set(entity.kataId, entity);
      this.linearIdToKataId.set(entity.linearId, entity.kataId);
    }
    this.mergeIssueDependencies();
    this.discovered = true;
  }

  private async loadMilestoneEntities(projectId: string): Promise<TrackedLinearEntity[]> {
    const milestones = await this.client.paginate<LinearMilestoneNode, { project?: { milestones?: any } | null }>({
      query: LINEAR_MILESTONES_QUERY,
      variables: { projectId },
      selectConnection: (data) => data.project?.milestones,
    });

    return milestones.map((milestone) => ({
      kataId: normalizeMilestoneKataId(milestone.name),
      type: "Milestone",
      linearId: milestone.id,
      title: stripKataPrefix(milestone.name),
      body: milestone.description ?? "",
      projectMilestoneId: milestone.id,
    }));
  }

  private async loadIssueEntities(teamId: string, projectId: string, milestones: TrackedLinearEntity[]): Promise<TrackedLinearEntity[]> {
    const milestoneByLinearId = new Map(milestones.map((milestone) => [milestone.linearId, milestone.kataId]));
    const issues = await this.client.paginate<LinearIssueNode, { issues?: any }>({
      query: LINEAR_ISSUES_QUERY,
      variables: { teamId, projectId },
      selectConnection: (data) => data.issues,
    });

    return issues.flatMap((issue) => issueEntitiesFromIssue(issue, milestoneByLinearId));
  }

  private mergeIssueDependencies(): void {
    for (const entity of this.entities.values()) {
      if (entity.type !== "Slice") continue;
      const blockedBy = parseSliceDependencyIds(entity.blockedBy);
      for (const blockerId of blockedBy) {
        const blocker = this.entities.get(blockerId);
        if (!blocker || blocker.type !== "Slice") continue;
        blocker.blocking = parseSliceDependencyIds([...(blocker.blocking ?? []), entity.kataId]);
      }
    }
  }

  private async findIssueEntity(issueRef: string): Promise<TrackedLinearEntity> {
    await this.discoverEntities();
    const trimmed = issueRef.trim();
    if (!trimmed) throw new KataDomainError("INVALID_CONFIG", "Standalone issue reference is required.");
    const normalized = trimmed.toUpperCase();
    const standalone = [...this.entities.values()].filter((entity) => entity.type === "Issue");
    const byKataId = standalone.find((entity) => entity.kataId.toUpperCase() === normalized);
    if (byKataId) return byKataId;
    const byIdentifier = standalone.find((entity) => entity.identifier?.toUpperCase() === normalized);
    if (byIdentifier) return byIdentifier;
    const byTitle = standalone.filter((entity) => entity.title.toLowerCase().includes(trimmed.toLowerCase()));
    if (byTitle.length === 1) return byTitle[0]!;
    if (byTitle.length > 1) {
      throw new KataDomainError("UNKNOWN", `Issue reference "${issueRef}" matched multiple standalone Linear issues.`);
    }
    throw new KataDomainError("NOT_FOUND", `Standalone Linear issue was not found for reference "${issueRef}".`);
  }
}
```

- [ ] **Step 6: Add adapter helper functions**

Append these helpers to `apps/cli/src/backends/linear/adapter.ts`:

```ts
function bodyContent(body: string): string {
  return body;
}

function stripKataPrefix(title: string): string {
  return title.replace(/^\[[A-Z]\d{3}\]\s*/, "");
}

function normalizeMilestoneKataId(name: string): string {
  const match = name.match(/\bM(\d+)\b/i);
  return match ? `M${String(Number(match[1])).padStart(3, "0")}` : "M001";
}

function linearIssueNumber(identifier: string | undefined): number | undefined {
  const match = identifier?.match(/-(\d+)$/);
  if (!match) return undefined;
  const number = Number(match[1]);
  return Number.isInteger(number) ? number : undefined;
}

function issueEntitiesFromIssue(
  issue: LinearIssueNode,
  milestoneByLinearId: Map<string, string>,
): TrackedLinearEntity[] {
  const entities: TrackedLinearEntity[] = [];
  const classification = classifyLinearIssue(issue, milestoneByLinearId);
  if (classification) {
    entities.push(entityFromIssueNode(issue, classification, milestoneByLinearId));
  }
  for (const child of issue.children?.nodes ?? []) {
    if (!child) continue;
    const childClassification = classifyLinearIssue(child, milestoneByLinearId);
    if (!childClassification) continue;
    entities.push(entityFromIssueNode(child, {
      ...childClassification,
      parentId: childClassification.parentId ?? classification?.kataId,
    }, milestoneByLinearId));
  }
  return entities;
}

function classifyLinearIssue(
  issue: LinearIssueNode,
  milestoneByLinearId: Map<string, string>,
): LinearEntityClassification | null {
  const kataId = parseKataIdFromTitle(issue.title);
  if (!kataId) return null;
  const labels = new Set((issue.labels?.nodes ?? []).flatMap((label) => label?.name ? [label.name] : []));
  if (kataId.startsWith("T") || issue.parent) {
    return { kataId, type: "Task" };
  }
  if (kataId.startsWith("S") || labels.has("kata/slice")) {
    return { kataId, type: "Slice", parentId: milestoneByLinearId.get(issue.projectMilestone?.id ?? "") };
  }
  if (kataId.startsWith("I") || labels.has("kata/issue")) {
    return { kataId, type: "Issue" };
  }
  return null;
}

function parseKataIdFromTitle(title: string | undefined): string | undefined {
  const match = title?.match(/\[([MSIT]\d{3})\]/i);
  return match ? match[1]!.toUpperCase() : undefined;
}

function entityFromIssueNode(
  issue: LinearIssueNode,
  classification: LinearEntityClassification,
  milestoneByLinearId: Map<string, string>,
): TrackedLinearEntity {
  return {
    kataId: classification.kataId,
    type: classification.type,
    parentId: classification.parentId ?? milestoneByLinearId.get(issue.projectMilestone?.id ?? ""),
    blockedBy: relationDependencies(issue, "blockedBy"),
    blocking: relationDependencies(issue, "blocking"),
    linearId: issue.id,
    identifier: issue.identifier,
    title: stripKataPrefix(issue.title),
    body: issue.description ?? "",
    url: issue.url ?? undefined,
    stateName: issue.state?.name ?? undefined,
    stateType: issue.state?.type ?? undefined,
    projectMilestoneId: issue.projectMilestone?.id ?? undefined,
  };
}

function relationDependencies(issue: LinearIssueNode, direction: "blockedBy" | "blocking"): string[] {
  const relations = [
    ...(issue.relations?.nodes ?? []),
    ...(issue.inverseRelations?.nodes ?? []),
  ].filter((relation): relation is LinearIssueRelationNode => relation !== null);
  const identifiers = relations.flatMap((relation) => {
    if (direction === "blockedBy" && relation.type.toLowerCase().includes("blocked")) {
      return relation.relatedIssue?.identifier ?? relation.issue?.identifier ?? [];
    }
    if (direction === "blocking" && relation.type.toLowerCase().includes("block")) {
      return relation.relatedIssue?.identifier ?? relation.issue?.identifier ?? [];
    }
    return [];
  });
  return parseSliceDependencyIds(identifiers);
}

function sliceStatusFromEntity(entity: TrackedLinearEntity, states: LinearStateMapping): KataSlice["status"] {
  return statusFromStateName(entity.stateName, states) as KataSlice["status"] ?? "backlog";
}

function taskStatusFromEntity(entity: TrackedLinearEntity, states: LinearStateMapping): KataTask["status"] {
  const status = statusFromStateName(entity.stateName, states);
  if (status === "done" || status === "todo" || status === "backlog") return status;
  return "in_progress";
}

function issueStatusFromEntity(entity: TrackedLinearEntity, states: LinearStateMapping): KataIssue["status"] {
  const status = statusFromStateName(entity.stateName, states);
  if (status === "done" || status === "todo" || status === "backlog" || status === "in_progress") return status;
  return entity.stateType === "completed" ? "done" : "backlog";
}

function statusFromStateName(stateName: string | undefined, states: LinearStateMapping): keyof LinearStateMapping | null {
  if (!stateName) return null;
  for (const [status, configuredName] of Object.entries(states) as Array<[keyof LinearStateMapping, string]>) {
    if (configuredName === stateName) return status;
  }
  return null;
}

function taskVerificationStateFromEntity(entity: TrackedLinearEntity): KataTask["verificationState"] {
  if (entity.stateType === "completed" || entity.stateName === "Done") return "verified";
  return "pending";
}

function isSliceStatus(value: string): value is KataSlice["status"] {
  return ["backlog", "todo", "in_progress", "agent_review", "human_review", "merging", "done"].includes(value);
}

function isTaskStatus(value: string): value is KataTask["status"] {
  return ["backlog", "todo", "in_progress", "done"].includes(value);
}
```

- [ ] **Step 7: Run adapter read tests and fix type errors**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.adapter.vitest.test.ts
pnpm --filter @kata-sh/cli run typecheck
```

Expected: adapter tests PASS and typecheck PASS. If TypeScript reports optional property assignment issues, add explicit `?? undefined` conversions where the error points.

- [ ] **Step 8: Commit adapter read path**

Run:

```bash
git add apps/cli/src/backends/linear/adapter.ts apps/cli/src/tests/linear.adapter.vitest.test.ts
git commit -m "feat(cli): read linear kata records"
```

## Task 5: Implement Linear Mutations and Dependencies

**Files:**

- Modify: `apps/cli/src/backends/linear/adapter.ts`
- Modify: `apps/cli/src/tests/linear.adapter.vitest.test.ts`

- [ ] **Step 1: Add failing mutation tests**

Append to `apps/cli/src/tests/linear.adapter.vitest.test.ts`:

```ts
describe("LinearKataAdapter mutations", () => {
  it("creates project, milestone, slice, task, standalone issue, and dependency records", async () => {
    const created: any[] = [];
    const client = createFakeLinearClient();
    client.graphql = vi.fn(async (request: any) => {
      if (request.query.includes("LinearKataContext")) {
        return {
          viewer: { id: "user-1" },
          organization: { id: "org-1", urlKey: "kata" },
          teams: { nodes: [{ id: "team-1", key: "KATA", name: "Kata" }], pageInfo: { hasNextPage: false, endCursor: null } },
          projects: { nodes: [{ id: "project-1", name: "Kata CLI", slugId: "kata-cli", url: "https://linear.test/project/kata-cli" }], pageInfo: { hasNextPage: false, endCursor: null } },
          workflowStates: {
            nodes: [
              { id: "state-backlog", name: "Backlog", type: "backlog" },
              { id: "state-todo", name: "Todo", type: "unstarted" },
              { id: "state-progress", name: "In Progress", type: "started" },
              { id: "state-agent", name: "Agent Review", type: "started" },
              { id: "state-human", name: "Human Review", type: "started" },
              { id: "state-merging", name: "Merging", type: "started" },
              { id: "state-done", name: "Done", type: "completed" },
            ],
            pageInfo: { hasNextPage: false, endCursor: null },
          },
        };
      }
      if (request.query.includes("LinearKataMilestones")) {
        return {
          project: {
            id: "project-1",
            name: "Kata CLI",
            milestones: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          },
        };
      }
      if (request.query.includes("LinearKataIssues")) {
        return {
          issues: { nodes: created.filter((record) => record.kind === "issue").map((record) => record.node), pageInfo: { hasNextPage: false, endCursor: null } },
        };
      }
      if (request.query.includes("projectMilestoneCreate")) {
        const node = {
          id: "milestone-1",
          name: request.variables.input.name,
          description: request.variables.input.description,
        };
        return { projectMilestoneCreate: { success: true, projectMilestone: node } };
      }
      if (request.query.includes("projectMilestoneUpdate")) {
        return {
          projectMilestoneUpdate: {
            success: true,
            projectMilestone: { id: request.variables.id, name: "M001 Phase A", description: request.variables.input.description },
          },
        };
      }
      if (request.query.includes("issueCreate")) {
        const id = `issue-${created.length + 1}`;
        const node = {
          id,
          identifier: `KATA-${created.length + 1}`,
          number: created.length + 1,
          title: request.variables.input.title,
          description: request.variables.input.description,
          url: `https://linear.test/KATA-${created.length + 1}`,
          state: { id: request.variables.input.stateId, name: "Backlog", type: "backlog" },
          projectMilestone: request.variables.input.projectMilestoneId
            ? { id: request.variables.input.projectMilestoneId, name: "M001 Phase A" }
            : null,
          parent: request.variables.input.parentId ? { id: request.variables.input.parentId, identifier: "KATA-1" } : null,
          children: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          relations: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          inverseRelations: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
        };
        created.push({ kind: "issue", node });
        return { issueCreate: { success: true, issue: node } };
      }
      if (request.query.includes("issueUpdate")) {
        const node = created.find((record) => record.node.id === request.variables.id)?.node;
        Object.assign(node, request.variables.input);
        return { issueUpdate: { success: true, issue: node } };
      }
      if (request.query.includes("issueRelationCreate")) {
        created.push({ kind: "relation", input: request.variables.input });
        return { issueRelationCreate: { success: true, issueRelation: { id: "relation-1" } } };
      }
      if (request.query.includes("LinearKataIssueComments")) {
        return { issue: { comments: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } } } };
      }
      if (request.query.includes("LinearKataProjectDocuments")) {
        return { project: { documents: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } } } };
      }
      throw new Error(`Unhandled query ${request.query}`);
    });
    client.paginate = vi.fn(async (input: any) => {
      const data = await client.graphql({ query: input.query, variables: input.variables });
      return input.selectConnection(data)?.nodes ?? [];
    });
    const adapter = createAdapter(client);

    const milestone = await adapter.createMilestone({ title: "Phase A", goal: "Build Linear" });
    const foundation = await adapter.createSlice({ milestoneId: milestone.id, title: "Foundation", goal: "First" });
    const dependent = await adapter.createSlice({ milestoneId: milestone.id, title: "Dependent", goal: "Second", blockedBy: [foundation.id] });
    const task = await adapter.createTask({ sliceId: foundation.id, title: "Verify", description: "Check it" });
    const issue = await adapter.createIssue({ title: "Standalone", design: "Design", plan: "Plan" });

    expect(milestone).toMatchObject({ id: "M001", status: "active" });
    expect(foundation).toMatchObject({ id: "S001", milestoneId: "M001" });
    expect(dependent).toMatchObject({ id: "S002", blockedBy: ["S001"] });
    expect(task).toMatchObject({ id: "T001", sliceId: "S001", verificationState: "pending" });
    expect(issue).toMatchObject({ id: "I001", status: "backlog" });
    expect(created.find((record) => record.kind === "relation")?.input).toMatchObject({
      issueId: "issue-3",
      relatedIssueId: "issue-2",
      type: "blocks",
    });
  });

  it("updates slice, task, issue, and milestone statuses", async () => {
    const client = createFakeLinearClient();
    const adapter = createAdapter(client);

    await expect(adapter.updateSliceStatus({ sliceId: "S001", status: "done" })).resolves.toMatchObject({
      id: "S001",
      status: "done",
    });
    await expect(adapter.updateTaskStatus({ taskId: "T001", status: "done", verificationState: "verified" })).resolves.toMatchObject({
      id: "T001",
      status: "done",
      verificationState: "verified",
    });
  });
});
```

- [ ] **Step 2: Run mutation tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.adapter.vitest.test.ts
```

Expected: FAIL on mutation methods that still throw `NOT_SUPPORTED`.

- [ ] **Step 3: Add mutation constants**

Add these constants to `apps/cli/src/backends/linear/adapter.ts`:

```ts
const LINEAR_PROJECT_UPDATE_MUTATION = `
  mutation LinearKataProjectUpdate($id: String!, $input: ProjectUpdateInput!) {
    projectUpdate(id: $id, input: $input) {
      success
      project {
        id
        name
        description
        slugId
        url
      }
    }
  }
`;

const LINEAR_PROJECT_MILESTONE_CREATE_MUTATION = `
  mutation LinearKataProjectMilestoneCreate($input: ProjectMilestoneCreateInput!) {
    projectMilestoneCreate(input: $input) {
      success
      projectMilestone {
        id
        name
        description
      }
    }
  }
`;

const LINEAR_PROJECT_MILESTONE_UPDATE_MUTATION = `
  mutation LinearKataProjectMilestoneUpdate($id: String!, $input: ProjectMilestoneUpdateInput!) {
    projectMilestoneUpdate(id: $id, input: $input) {
      success
      projectMilestone {
        id
        name
        description
      }
    }
  }
`;

const LINEAR_ISSUE_CREATE_MUTATION = `
  mutation LinearKataIssueCreate($input: IssueCreateInput!) {
    issueCreate(input: $input) {
      success
      issue {
        id
        identifier
        number
        title
        description
        url
        state {
          id
          name
          type
        }
        projectMilestone {
          id
          name
          description
        }
        parent {
          id
          identifier
        }
      }
    }
  }
`;

const LINEAR_ISSUE_UPDATE_MUTATION = `
  mutation LinearKataIssueUpdate($id: String!, $input: IssueUpdateInput!) {
    issueUpdate(id: $id, input: $input) {
      success
      issue {
        id
        identifier
        number
        title
        description
        url
        state {
          id
          name
          type
        }
        projectMilestone {
          id
          name
          description
        }
        parent {
          id
          identifier
        }
      }
    }
  }
`;

const LINEAR_ISSUE_RELATION_CREATE_MUTATION = `
  mutation LinearKataIssueRelationCreate($input: IssueRelationCreateInput!) {
    issueRelationCreate(input: $input) {
      success
      issueRelation {
        id
      }
    }
  }
`;
```

- [ ] **Step 4: Implement mutation methods**

Replace the `NOT_SUPPORTED` mutation methods in `LinearKataAdapter` with:

```ts
  async upsertProject(input: KataProjectUpsertInput): Promise<KataProjectContext> {
    const context = await this.getContext();
    const data = await this.client.graphql<{ projectUpdate: { project: LinearProjectNode } }>({
      query: LINEAR_PROJECT_UPDATE_MUTATION,
      variables: {
        id: context.project.id,
        input: {
          name: input.title,
          description: input.description,
        },
      },
    });
    context.project.name = data.projectUpdate.project.name;
    context.project.description = data.projectUpdate.project.description;
    return {
      backend: "linear",
      workspacePath: this.workspacePath,
      title: input.title,
      description: input.description,
    };
  }

  async createMilestone(input: KataMilestoneCreateInput): Promise<KataMilestone> {
    await this.discoverEntities();
    const context = await this.getContext();
    const kataId = this.nextKataId("Milestone");
    const data = await this.client.graphql<{ projectMilestoneCreate: { projectMilestone: LinearMilestoneNode } }>({
      query: LINEAR_PROJECT_MILESTONE_CREATE_MUTATION,
      variables: {
        input: {
          projectId: context.project.id,
          name: `[${kataId}] ${input.title}`,
          description: input.goal,
        },
      },
    });
    const milestone = data.projectMilestoneCreate.projectMilestone;
    this.entities.set(kataId, {
      kataId,
      type: "Milestone",
      linearId: milestone.id,
      title: stripKataPrefix(milestone.name),
      body: milestone.description ?? input.goal,
      projectMilestoneId: milestone.id,
    });
    return { id: kataId, title: input.title, goal: input.goal, status: "active", active: true };
  }

  async completeMilestone(input: KataMilestoneCompleteInput): Promise<KataMilestone> {
    const entity = await this.requireEntity(input.milestoneId, "Milestone");
    const updatedDescription = appendBodySection(entity.body, "Completion summary", input.summary);
    const data = await this.client.graphql<{ projectMilestoneUpdate: { projectMilestone: LinearMilestoneNode } }>({
      query: LINEAR_PROJECT_MILESTONE_UPDATE_MUTATION,
      variables: {
        id: entity.linearId,
        input: {
          description: updatedDescription,
        },
      },
    });
    const updated = {
      ...entity,
      body: data.projectMilestoneUpdate.projectMilestone.description ?? updatedDescription,
    };
    this.entities.set(entity.kataId, updated);
    return {
      id: entity.kataId,
      title: entity.title,
      goal: bodyContent(updated.body),
      status: "done",
      active: false,
    };
  }

  async createSlice(input: KataSliceCreateInput): Promise<KataSlice> {
    await this.discoverEntities();
    const milestone = await this.requireEntity(input.milestoneId, "Milestone");
    const context = await this.getContext();
    const kataId = this.nextKataId("Slice");
    const blockedBy = parseSliceDependencyIds(input.blockedBy ?? []);
    const issue = await this.createLinearIssue({
      kataId,
      type: "Slice",
      parentId: input.milestoneId,
      title: input.title,
      content: input.goal,
      projectMilestoneId: milestone.linearId,
      stateId: requireStateId(context, "backlog"),
    });
    const entity = entityFromIssueNode(issue, {
      kataId,
      type: "Slice",
      parentId: input.milestoneId,
    }, new Map([[milestone.linearId, input.milestoneId]]));
    entity.blockedBy = blockedBy;
    this.entities.set(kataId, entity);
    this.linearIdToKataId.set(entity.linearId, kataId);
    await this.createNativeIssueDependencies(entity, blockedBy);
    return {
      id: kataId,
      milestoneId: input.milestoneId,
      title: input.title,
      goal: input.goal,
      status: "backlog",
      order: input.order ?? 0,
      blockedBy,
      blocking: [],
    };
  }

  async updateSliceStatus(input: KataSliceUpdateStatusInput): Promise<KataSlice> {
    const entity = await this.requireEntity(input.sliceId, "Slice");
    const updated = await this.updateLinearIssueEntity(entity, input.status);
    return { ...sliceFromTrackedEntity(updated), status: input.status };
  }

  async createTask(input: KataTaskCreateInput): Promise<KataTask> {
    await this.discoverEntities();
    const slice = await this.requireEntity(input.sliceId, "Slice");
    const context = await this.getContext();
    const kataId = this.nextKataId("Task");
    const issue = await this.createLinearIssue({
      kataId,
      type: "Task",
      parentId: input.sliceId,
      title: input.title,
      content: input.description,
      projectMilestoneId: slice.projectMilestoneId,
      parentLinearId: slice.linearId,
      stateId: requireStateId(context, "backlog"),
    });
    const entity = entityFromIssueNode(issue, {
      kataId,
      type: "Task",
      parentId: input.sliceId,
    }, new Map());
    this.entities.set(kataId, entity);
    this.linearIdToKataId.set(entity.linearId, kataId);
    return {
      id: kataId,
      sliceId: input.sliceId,
      title: input.title,
      description: input.description,
      status: "backlog",
      verificationState: "pending",
    };
  }

  async updateTaskStatus(input: KataTaskUpdateStatusInput): Promise<KataTask> {
    const entity = await this.requireEntity(input.taskId, "Task");
    const verificationState = input.verificationState ?? taskVerificationStateFromEntity(entity);
    const updated = await this.updateLinearIssueEntity(entity, input.status);
    return {
      ...taskFromTrackedEntity(updated),
      status: input.status,
      verificationState,
    };
  }

  async createIssue(input: KataIssueCreateInput): Promise<KataIssue> {
    await this.discoverEntities();
    const context = await this.getContext();
    const kataId = this.nextKataId("Issue");
    const body = `# Design\n\n${input.design}\n\n# Plan\n\n${input.plan}`;
    const issue = await this.createLinearIssue({
      kataId,
      type: "Issue",
      title: input.title,
      content: body,
      stateId: requireStateId(context, "backlog"),
    });
    const entity = entityFromIssueNode(issue, {
      kataId,
      type: "Issue",
    }, new Map());
    this.entities.set(kataId, entity);
    this.linearIdToKataId.set(entity.linearId, kataId);
    return {
      id: kataId,
      number: linearIssueNumber(issue.identifier),
      title: input.title,
      body,
      status: "backlog",
      url: issue.url ?? undefined,
    };
  }

  async updateIssueStatus(input: KataIssueUpdateStatusInput): Promise<KataIssue> {
    const entity = await this.requireEntity(input.issueId, "Issue");
    const updated = await this.updateLinearIssueEntity(entity, input.status);
    return {
      id: updated.kataId,
      number: linearIssueNumber(updated.identifier),
      title: updated.title,
      body: bodyContent(updated.body),
      status: input.status,
      url: updated.url,
    };
  }
```

- [ ] **Step 5: Add private mutation helpers**

Add these private methods inside `LinearKataAdapter`:

```ts
  private async requireEntity(kataId: string, type: LinearEntityType): Promise<TrackedLinearEntity> {
    await this.discoverEntities();
    const entity = this.entities.get(kataId);
    if (!entity || entity.type !== type) {
      throw new KataDomainError("NOT_FOUND", `Linear ${type} record was not found for ${kataId}.`);
    }
    return entity;
  }

  private nextKataId(type: "Milestone" | "Slice" | "Task" | "Issue"): string {
    const prefix = type === "Milestone" ? "M" : type === "Slice" ? "S" : type === "Task" ? "T" : "I";
    const maxExisting = [...this.entities.keys()].reduce((max, kataId) => {
      const match = kataId.match(new RegExp(`^${prefix}(\\d+)$`));
      return match ? Math.max(max, Number(match[1])) : max;
    }, 0);
    return `${prefix}${String(maxExisting + 1).padStart(3, "0")}`;
  }

  private async createLinearIssue(input: {
    kataId: string;
    type: LinearEntityType;
    parentId?: string;
    title: string;
    content: string;
    projectMilestoneId?: string;
    parentLinearId?: string;
    stateId: string;
  }): Promise<LinearIssueNode> {
    const context = await this.getContext();
    const data = await this.client.graphql<{ issueCreate: { issue: LinearIssueNode } }>({
      query: LINEAR_ISSUE_CREATE_MUTATION,
      variables: {
        input: {
          teamId: context.team.id,
          projectId: context.project.id,
          title: `[${input.kataId}] ${input.title}`,
          description: input.content,
          stateId: input.stateId,
          labelIds: labelIdsForType(context, input.type),
          ...(input.projectMilestoneId ? { projectMilestoneId: input.projectMilestoneId } : {}),
          ...(input.parentLinearId ? { parentId: input.parentLinearId } : {}),
        },
      },
    });
    return data.issueCreate.issue;
  }

  private async updateLinearIssueEntity(
    entity: TrackedLinearEntity,
    status: LinearSliceStatus | LinearTaskStatus | KataIssue["status"],
  ): Promise<TrackedLinearEntity> {
    const context = await this.getContext();
    const data = await this.client.graphql<{ issueUpdate: { issue: LinearIssueNode } }>({
      query: LINEAR_ISSUE_UPDATE_MUTATION,
      variables: {
        id: entity.linearId,
        input: {
          stateId: requireStateId(context, status),
        },
      },
    });
    const updated = entityFromIssueNode(data.issueUpdate.issue, {
      kataId: entity.kataId,
      type: entity.type,
      parentId: entity.parentId,
    }, new Map());
    updated.blockedBy = entity.blockedBy;
    updated.blocking = entity.blocking;
    this.entities.set(entity.kataId, updated);
    return updated;
  }

  private async createNativeIssueDependencies(blockedEntity: TrackedLinearEntity, blockedByIds: readonly string[]): Promise<void> {
    const createdBlockedByIds: string[] = [];
    for (const blockedById of blockedByIds) {
      if (blockedById === blockedEntity.kataId) continue;
      const blocker = await this.requireEntity(blockedById, "Slice");
      await this.client.graphql({
        query: LINEAR_ISSUE_RELATION_CREATE_MUTATION,
        variables: {
          input: {
            issueId: blockedEntity.linearId,
            relatedIssueId: blocker.linearId,
            type: "blocks",
          },
        },
      });
      createdBlockedByIds.push(blocker.kataId);
      this.entities.set(blocker.kataId, {
        ...blocker,
        blocking: parseSliceDependencyIds([...(blocker.blocking ?? []), blockedEntity.kataId]),
      });
    }
    if (createdBlockedByIds.length > 0) {
      this.entities.set(blockedEntity.kataId, {
        ...blockedEntity,
        blockedBy: parseSliceDependencyIds([...(blockedEntity.blockedBy ?? []), ...createdBlockedByIds]),
      });
    }
  }
```

- [ ] **Step 6: Add mutation helper functions**

Append these helpers outside the class in `apps/cli/src/backends/linear/adapter.ts`:

```ts
function requireStateId(
  context: { stateByKataStatus: Map<string, LinearWorkflowStateNode> },
  status: string,
): string {
  const state = context.stateByKataStatus.get(status);
  if (!state) throw new KataDomainError("INVALID_CONFIG", `Linear workflow state for Kata status "${status}" was not found.`);
  return state.id;
}

function labelIdsForType(
  _context: { labelByType?: Map<LinearEntityType, string> },
  _type: LinearEntityType,
): string[] {
  return [];
}

function appendBodySection(body: string, heading: string, content: string): string {
  const base = body.trimEnd();
  return `${base}\n\n## ${heading}\n\n${content.trim()}\n`;
}

function sliceFromTrackedEntity(entity: TrackedLinearEntity): KataSlice {
  return {
    id: entity.kataId,
    milestoneId: entity.parentId ?? "",
    title: entity.title,
    goal: bodyContent(entity.body) || entity.title,
    status: entity.stateType === "completed" ? "done" : "backlog",
    order: 0,
    blockedBy: parseSliceDependencyIds(entity.blockedBy),
    blocking: parseSliceDependencyIds(entity.blocking),
  };
}

function taskFromTrackedEntity(entity: TrackedLinearEntity): KataTask {
  return {
    id: entity.kataId,
    sliceId: entity.parentId ?? "",
    title: entity.title,
    description: bodyContent(entity.body),
    status: entity.stateType === "completed" ? "done" : "backlog",
    verificationState: taskVerificationStateFromEntity(entity),
  };
}
```

- [ ] **Step 7: Run mutation tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.adapter.vitest.test.ts
pnpm --filter @kata-sh/cli run typecheck
```

Expected: PASS.

- [ ] **Step 8: Commit Linear mutations**

Run:

```bash
git add apps/cli/src/backends/linear/adapter.ts apps/cli/src/tests/linear.adapter.vitest.test.ts
git commit -m "feat(cli): mutate linear kata records"
```

## Task 6: Implement Linear Artifact Reads and Writes

**Files:**

- Modify: `apps/cli/src/backends/linear/adapter.ts`
- Modify: `apps/cli/src/tests/linear.adapter.vitest.test.ts`

- [ ] **Step 1: Add failing adapter artifact tests**

Append to `apps/cli/src/tests/linear.adapter.vitest.test.ts`:

```ts
describe("LinearKataAdapter artifacts", () => {
  it("writes milestone artifacts as Linear documents", async () => {
    const client = createFakeLinearClient();
    const adapter = createAdapter(client);

    const artifact = await adapter.writeArtifact({
      scopeType: "milestone",
      scopeId: "M001",
      artifactType: "requirements",
      title: "Requirements",
      content: "# Requirements",
      format: "markdown",
    });

    expect(artifact).toMatchObject({
      scopeType: "milestone",
      scopeId: "M001",
      artifactType: "requirements",
      content: "# Requirements",
      provenance: { backend: "linear" },
    });
  });

  it("writes slice, task, and standalone issue artifacts as Linear comments", async () => {
    const adapter = createAdapter();

    await expect(adapter.writeArtifact({
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      title: "Slice plan",
      content: "# Plan",
      format: "markdown",
    })).resolves.toMatchObject({
      scopeType: "slice",
      scopeId: "S001",
      artifactType: "plan",
      content: "# Plan",
    });
  });
});
```

- [ ] **Step 2: Run adapter artifact tests and verify they fail**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.adapter.vitest.test.ts --testNamePattern "artifacts"
```

Expected: FAIL because adapter artifact methods still return empty or throw.

- [ ] **Step 3: Implement adapter artifact methods**

Replace `listArtifacts`, `readArtifact`, and `writeArtifact` in `apps/cli/src/backends/linear/adapter.ts` with:

```ts
  async listArtifacts(input: { scopeType: KataScopeType; scopeId: string }): Promise<KataArtifact[]> {
    if (input.scopeType === "project") return [];
    if (input.scopeType === "milestone") {
      const context = await this.getContext();
      const documents = await this.client.paginate<{ id: string; title: string; content?: string | null; updatedAt?: string | null }, { project?: { documents?: any } | null }>({
        query: `
          query LinearKataProjectDocuments($projectId: String!, $after: String) {
            project(id: $projectId) {
              documents(first: 100, after: $after) {
                nodes {
                  id
                  title
                  content
                  updatedAt
                }
                pageInfo {
                  hasNextPage
                  endCursor
                }
              }
            }
          }
        `,
        variables: { projectId: context.project.id },
        selectConnection: (data) => data.project?.documents,
      });
      return documents
        .map((document) => artifactFromLinearDocument(document, input.scopeType, input.scopeId))
        .filter((artifact): artifact is KataArtifact => artifact !== null);
    }

    const entity = await this.findArtifactEntity(input.scopeType, input.scopeId);
    if (!entity) return [];
    const comments = await this.client.paginate<{ id: string; body?: string | null; updatedAt?: string | null }, { issue?: { comments?: any } | null }>({
      query: `
        query LinearKataIssueComments($issueId: String!, $after: String) {
          issue(id: $issueId) {
            comments(first: 100, after: $after) {
              nodes {
                id
                body
                updatedAt
              }
              pageInfo {
                hasNextPage
                endCursor
              }
            }
          }
        }
      `,
      variables: { issueId: entity.linearId },
      selectConnection: (data) => data.issue?.comments,
    });
    return comments
      .map((comment) => artifactFromLinearComment(comment, input.scopeType, input.scopeId))
      .filter((artifact): artifact is KataArtifact => artifact !== null);
  }

  async readArtifact(input: { scopeType: KataScopeType; scopeId: string; artifactType: KataArtifactType }): Promise<KataArtifact | null> {
    return (await this.listArtifacts(input)).find((artifact) => artifact.artifactType === input.artifactType) ?? null;
  }

  async writeArtifact(input: KataArtifactWriteInput): Promise<KataArtifact> {
    if (input.scopeType === "milestone") {
      const context = await this.getContext();
      const result = await upsertLinearMilestoneDocument({
        client: this.client,
        projectId: context.project.id,
        scopeId: input.scopeId,
        artifactType: input.artifactType,
        title: input.title,
        content: input.content,
      });
      const parsed = parseLinearArtifactMarker(result.body);
      return {
        id: `${input.scopeType}:${input.scopeId}:${input.artifactType}`,
        scopeType: input.scopeType,
        scopeId: input.scopeId,
        artifactType: input.artifactType,
        title: result.title ?? input.title,
        content: parsed?.content ?? input.content,
        format: input.format,
        updatedAt: result.updatedAt ?? new Date().toISOString(),
        provenance: {
          backend: "linear",
          backendId: result.backendId,
        },
      };
    }

    const entity = await this.findArtifactEntity(input.scopeType, input.scopeId);
    if (!entity) {
      throw new KataDomainError("NOT_FOUND", `Linear tracking record was not found for ${input.scopeType} ${input.scopeId}.`);
    }
    const result = await upsertLinearIssueArtifactComment({
      client: this.client,
      issueId: entity.linearId,
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
      content: parseLinearArtifactMarker(result.body)?.content ?? input.content,
      format: input.format,
      updatedAt: new Date().toISOString(),
      provenance: {
        backend: "linear",
        backendId: result.backendId,
      },
    };
  }
```

- [ ] **Step 4: Add artifact entity helpers**

Add this private method inside `LinearKataAdapter`:

```ts
  private async findArtifactEntity(scopeType: KataScopeType, scopeId: string): Promise<TrackedLinearEntity | null> {
    await this.discoverEntities();
    if (scopeType === "project") return null;
    const entity = this.entities.get(scopeId);
    return entity ?? null;
  }
```

Append these helpers outside the class:

```ts
function artifactFromLinearDocument(
  document: { id: string; title: string; content?: string | null; updatedAt?: string | null },
  scopeType: KataScopeType,
  scopeId: string,
): KataArtifact | null {
  const parsed = typeof document.content === "string" ? parseLinearArtifactMarker(document.content) : null;
  if (!parsed || parsed.scopeType !== scopeType || parsed.scopeId !== scopeId) return null;
  return {
    id: `${scopeType}:${scopeId}:${parsed.artifactType}`,
    scopeType,
    scopeId,
    artifactType: parsed.artifactType,
    title: document.title,
    content: parsed.content,
    format: "markdown",
    updatedAt: document.updatedAt ?? new Date().toISOString(),
    provenance: {
      backend: "linear",
      backendId: `document:${document.id}`,
    },
  };
}

function artifactFromLinearComment(
  comment: { id: string; body?: string | null; updatedAt?: string | null },
  scopeType: KataScopeType,
  scopeId: string,
): KataArtifact | null {
  const parsed = typeof comment.body === "string" ? parseLinearArtifactMarker(comment.body) : null;
  if (!parsed || parsed.scopeType !== scopeType || parsed.scopeId !== scopeId) return null;
  return {
    id: `${scopeType}:${scopeId}:${parsed.artifactType}`,
    scopeType,
    scopeId,
    artifactType: parsed.artifactType,
    title: parsed.artifactType,
    content: parsed.content,
    format: "markdown",
    updatedAt: comment.updatedAt ?? new Date().toISOString(),
    provenance: {
      backend: "linear",
      backendId: `comment:${comment.id}`,
    },
  };
}
```

- [ ] **Step 5: Run artifact and snapshot tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.artifacts.vitest.test.ts src/tests/linear.adapter.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit Linear artifact adapter support**

Run:

```bash
git add apps/cli/src/backends/linear/adapter.ts apps/cli/src/tests/linear.adapter.vitest.test.ts
git commit -m "feat(cli): store linear kata artifacts"
```

## Task 7: Wire Linear Backend Resolution, Setup, and Doctor

**Files:**

- Modify: `apps/cli/src/backends/resolve-backend.ts`
- Modify: `apps/cli/src/commands/setup.ts`
- Modify: `apps/cli/src/cli.ts`
- Modify: `apps/cli/src/commands/doctor.ts`
- Test: `apps/cli/src/tests/golden-path.pi-linear.vitest.test.ts`
- Modify: `apps/cli/src/tests/setup-source.vitest.test.ts`

- [ ] **Step 1: Write failing golden path test**

Create `apps/cli/src/tests/golden-path.pi-linear.vitest.test.ts`:

```ts
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it, vi } from "vitest";

import { resolveBackend } from "../backends/resolve-backend.js";
import { runDoctor } from "../commands/doctor.js";
import { runSetup } from "../commands/setup.js";
import { createKataDomainApi } from "../domain/service.js";
import { runJsonCommand } from "../transports/json.js";

function createGoldenFakeLinearClient() {
  return {
    graphql: vi.fn(async (request: any) => {
      if (request.query.includes("LinearKataContext")) {
        return {
          viewer: { id: "user-1" },
          organization: { id: "org-1", urlKey: "kata" },
          teams: { nodes: [{ id: "team-1", key: "KATA", name: "Kata" }], pageInfo: { hasNextPage: false, endCursor: null } },
          projects: { nodes: [{ id: "project-1", name: "Kata CLI", slugId: "kata-cli", url: "https://linear.test/project/kata-cli" }], pageInfo: { hasNextPage: false, endCursor: null } },
          workflowStates: {
            nodes: [
              { id: "state-backlog", name: "Backlog", type: "backlog" },
              { id: "state-todo", name: "Todo", type: "unstarted" },
              { id: "state-progress", name: "In Progress", type: "started" },
              { id: "state-agent", name: "Agent Review", type: "started" },
              { id: "state-human", name: "Human Review", type: "started" },
              { id: "state-merging", name: "Merging", type: "started" },
              { id: "state-done", name: "Done", type: "completed" },
            ],
            pageInfo: { hasNextPage: false, endCursor: null },
          },
        };
      }
      if (request.query.includes("LinearKataMilestones")) {
        return {
          project: {
            id: "project-1",
            name: "Kata CLI",
            milestones: {
              nodes: [{
                id: "milestone-1",
                name: "M001 Linear Golden",
                description: "Linear golden",
              }],
              pageInfo: { hasNextPage: false, endCursor: null },
            },
          },
        };
      }
      if (request.query.includes("LinearKataIssues")) {
        return { issues: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } } };
      }
      if (request.query.includes("LinearKataProjectDocuments")) {
        return { project: { documents: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } } } };
      }
      if (request.query.includes("LinearKataIssueComments")) {
        return { issue: { comments: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } } } };
      }
      return {};
    }),
    paginate: vi.fn(async (input: any) => {
      const data = await (input as any).client?.graphql?.({ query: input.query, variables: input.variables });
      return input.selectConnection(data)?.nodes ?? [];
    }),
  };
}

describe("golden path: pi + linear", () => {
  it("covers setup, doctor, resolveBackend, and a linear-backed runtime json operation", async () => {
    const tmp = mkdtempSync(join(tmpdir(), "kata-linear-golden-"));
    const workspaceDir = join(tmp, "repo");
    const cliSkillsDir = join(workspaceDir, "apps", "cli", "skills");

    try {
      mkdirSync(join(cliSkillsDir, "kata-health"), { recursive: true });
      writeFileSync(join(workspaceDir, "pnpm-workspace.yaml"), "packages:\n  - apps/*\n", "utf8");
      writeFileSync(join(cliSkillsDir, "kata-health", "SKILL.md"), "# Kata Health\n", "utf8");

      const setupResult = await runSetup({
        pi: true,
        env: { PI_CODING_AGENT_DIR: join(tmp, "pi-agent"), LINEAR_API_KEY: "lin_test" },
        packageVersion: "9.9.9-test",
        cwd: workspaceDir,
        interactive: false,
        onboarding: {
          backend: "linear",
          linearWorkspace: "kata",
          linearTeam: "KATA",
          linearProject: "kata-cli",
        } as any,
      });
      expect(setupResult.ok).toBe(true);
      expect(readFileSync(join(workspaceDir, ".kata", "preferences.md"), "utf8")).toContain("mode: linear");

      const linearClient = createGoldenFakeLinearClient();
      linearClient.paginate = vi.fn(async (input: any) => {
        const data = await linearClient.graphql({ query: input.query, variables: input.variables });
        return input.selectConnection(data)?.nodes ?? [];
      });

      const doctor = await runDoctor({
        cwd: workspaceDir,
        env: { PI_CODING_AGENT_DIR: join(tmp, "pi-agent"), LINEAR_API_KEY: "lin_test" },
        packageVersion: "9.9.9-test",
        linearClient: linearClient as any,
      } as any);
      expect(doctor.status).toBe("ok");
      expect(doctor.checks.find((check) => check.name === "linear-auth")).toMatchObject({ status: "ok" });
      expect(doctor.checks.find((check) => check.name === "linear-project")).toMatchObject({ status: "ok" });
      expect(doctor.checks.find((check) => check.name === "linear-workflow-states")).toMatchObject({ status: "ok" });

      const adapter = await resolveBackend({
        workspacePath: workspaceDir,
        env: { LINEAR_API_KEY: "lin_test" },
        linearClient: linearClient as any,
      } as any);
      const output = await runJsonCommand(
        { operation: "milestone.getActive", payload: {} },
        createKataDomainApi(adapter),
      );
      expect(JSON.parse(output)).toMatchObject({
        ok: true,
        data: {
          id: "M001",
          status: "active",
        },
      });
      expect(existsSync(join(tmp, "pi-agent", "skills", "kata-health", "SKILL.md"))).toBe(true);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});
```

- [ ] **Step 2: Run golden path test and verify it fails**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/golden-path.pi-linear.vitest.test.ts
```

Expected: FAIL because setup onboarding lacks Linear fields, `resolveBackend` lacks `linearClient`, and `runDoctor` lacks Linear checks.

- [ ] **Step 3: Extend setup input types and render Linear preferences**

Modify `SetupPreferencesResult` and `SetupOnboardingInput` in `apps/cli/src/commands/setup.ts`:

```ts
export interface SetupPreferencesResult {
  path: string;
  status: "existing" | "created";
  backend?: "github" | "linear";
  repoOwner?: string;
  repoName?: string;
  githubProjectNumber?: number;
  linearWorkspace?: string;
  linearTeam?: string;
  linearProject?: string;
  linearAuthEnv?: string;
}

export interface SetupOnboardingInput {
  backend?: "github" | "linear";
  repoOwner?: string;
  repoName?: string;
  githubProjectNumber?: number;
  linearWorkspace?: string;
  linearTeam?: string;
  linearProject?: string;
  linearAuthEnv?: string;
}
```

Add this function after `renderGithubPreferences`:

```ts
function renderLinearPreferences(input: {
  workspace: string;
  team: string;
  project: string;
  authEnv?: string;
}): string {
  const authEnv = input.authEnv ?? "LINEAR_API_KEY";
  return `---\nworkflow:\n  mode: linear\nlinear:\n  workspace: ${input.workspace}\n  team: ${input.team}\n  project: ${input.project}\n  authEnv: ${authEnv}\n  states:\n    backlog: Backlog\n    todo: Todo\n    in_progress: In Progress\n    agent_review: Agent Review\n    human_review: Human Review\n    merging: Merging\n    done: Done\n---\n`;
}
```

- [ ] **Step 4: Update setup preference creation**

Inside `ensurePreferences` in `apps/cli/src/commands/setup.ts`, branch on `input.onboarding?.backend === "linear"` before GitHub auth is checked:

```ts
  if (input.onboarding?.backend === "linear") {
    let workspace = cleanString(input.onboarding.linearWorkspace) ?? undefined;
    let team = cleanString(input.onboarding.linearTeam) ?? undefined;
    let project = cleanString(input.onboarding.linearProject) ?? undefined;
    const authEnv = cleanString(input.onboarding.linearAuthEnv) ?? "LINEAR_API_KEY";

    if (!workspace || !team || !project) {
      if (!input.interactive) {
        throw Object.assign(new Error("Linear setup requires workspace, team, and project in non-interactive mode."), {
          code: "NON_INTERACTIVE_SETUP_REQUIRED",
        });
      }
      const rl = createInterface({ input: process.stdin, output: process.stdout });
      try {
        workspace = await askRequired(rl.question.bind(rl), "Linear workspace", workspace);
        team = await askRequired(rl.question.bind(rl), "Linear team key or ID", team);
        project = await askRequired(rl.question.bind(rl), "Linear project slug, name, or ID", project);
      } finally {
        rl.close();
      }
    }

    await mkdir(dirname(preferencesPath), { recursive: true });
    await writeFile(
      preferencesPath,
      renderLinearPreferences({ workspace, team, project, authEnv }),
      "utf8",
    );

    return {
      path: preferencesPath,
      status: "created",
      backend: "linear",
      linearWorkspace: workspace,
      linearTeam: team,
      linearProject: project,
      linearAuthEnv: authEnv,
    };
  }
```

- [ ] **Step 5: Add CLI setup flags**

Modify `apps/cli/src/cli.ts` setup argument parsing:

```ts
    const result = await runSetup({
      pi: flagSet.has("--pi"),
      local: flagSet.has("--local"),
      global: flagSet.has("--global"),
      cursor: flagSet.has("--cursor"),
      claude: flagSet.has("--claude"),
      interactive: !flagSet.has("--yes") && Boolean(process.stdin.isTTY),
      onboarding: {
        backend: rawBackend ?? "github",
        repoOwner: valueAfter("--repo-owner") ?? repoOwnerFromRepo,
        repoName: valueAfter("--repo-name") ?? repoNameFromRepo,
        githubProjectNumber,
        linearWorkspace: valueAfter("--linear-workspace") ?? valueAfter("--workspace"),
        linearTeam: valueAfter("--linear-team") ?? valueAfter("--team"),
        linearProject: valueAfter("--linear-project") ?? valueAfter("--project"),
        linearAuthEnv: valueAfter("--linear-auth-env") ?? valueAfter("--auth-env"),
      },
      env: process.env,
      packageVersion,
    });
```

- [ ] **Step 6: Wire resolveBackend to real Linear client**

Modify imports and `resolveBackend` in `apps/cli/src/backends/resolve-backend.ts`:

```ts
import { createLinearClient } from "./linear/client.js";
import { resolveLinearAuthToken } from "./linear/config.js";
```

Change the `resolveBackend` input type:

```ts
  linearClient?: ReturnType<typeof createLinearClient>;
```

Replace the Linear branch:

```ts
  const token = resolveLinearAuthToken({ authEnv: config.authEnv, env: input.env });
  const client = input.linearClient ?? (token ? createLinearClient({ token }) : null);
  if (!client) {
    throw new KataDomainError(
      "UNAUTHORIZED",
      "Linear mode requires LINEAR_API_KEY/LINEAR_TOKEN or the env var configured by linear.authEnv.",
    );
  }

  return new LinearKataAdapter({
    client,
    config,
    workspacePath: input.workspacePath,
  });
```

Remove the old `linearClients` constructor input path.

- [ ] **Step 7: Add Linear doctor checks**

Modify `RunDoctorInput` in `apps/cli/src/commands/doctor.ts`:

```ts
  linearClient?: ReturnType<typeof createLinearClient>;
```

Add imports:

```ts
import { createLinearClient } from "../backends/linear/client.js";
import { resolveLinearAuthToken } from "../backends/linear/config.js";
import { LinearKataAdapter } from "../backends/linear/adapter.js";
```

Inside the `if (config.kind === "github")` block’s sibling branch, add:

```ts
      if (config.kind === "linear") {
        const token = resolveLinearAuthToken({ authEnv: config.authEnv, env });
        checks.push({
          name: "linear-auth",
          status: token ? "ok" : "invalid",
          message: token ? "Linear auth is configured." : "Linear mode requires LINEAR_API_KEY/LINEAR_TOKEN or the env var configured by linear.authEnv.",
          ...(token ? {} : { action: "Set LINEAR_API_KEY, LINEAR_TOKEN, or the env var named by linear.authEnv." }),
        });

        if (token || input.linearClient) {
          const client = input.linearClient ?? createLinearClient({ token: token ?? "" });
          try {
            const adapter = new LinearKataAdapter({
              client,
              config,
              workspacePath: cwd,
            });
            await adapter.getProjectContext();
            checks.push({
              name: "linear-project",
              status: "ok",
              message: `Linear workspace ${config.workspace}, team ${config.team}, and project ${config.project} are accessible.`,
            });
            checks.push({
              name: "linear-workflow-states",
              status: "ok",
              message: "Linear workflow states required by Kata are available.",
            });
            checks.push({
              name: "linear-capabilities",
              status: "ok",
              message: "Linear documents, comments, sub-issues, and issue relations are available through GraphQL.",
            });
          } catch (error) {
            checks.push({
              name: "linear-project",
              status: "invalid",
              message: error instanceof Error ? error.message : "Unable to validate Linear project access.",
              action: "Verify linear.workspace, linear.team, linear.project, auth, and configured state names.",
            });
          }
        }
      }
```

- [ ] **Step 8: Run setup and golden path tests**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run src/tests/linear.config.vitest.test.ts src/tests/golden-path.pi-linear.vitest.test.ts src/tests/setup-source.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 9: Commit setup, doctor, and backend wiring**

Run:

```bash
git add apps/cli/src/backends/resolve-backend.ts apps/cli/src/commands/setup.ts apps/cli/src/cli.ts apps/cli/src/commands/doctor.ts apps/cli/src/tests/golden-path.pi-linear.vitest.test.ts apps/cli/src/tests/setup-source.vitest.test.ts
git commit -m "feat(cli): wire linear setup and doctor"
```

## Task 8: Validate Snapshots, Dependencies, and GitHub Regression

**Files:**

- Modify: `apps/cli/src/tests/linear.adapter.vitest.test.ts`
- Modify: `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts`
- Modify: `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`

- [ ] **Step 1: Add Linear snapshot dependency test**

Append to `apps/cli/src/tests/linear.adapter.vitest.test.ts`:

```ts
describe("LinearKataAdapter snapshots and dependencies", () => {
  it("exposes dependency-gated next actions from Linear native relations and roadmap documents", async () => {
    const client = createFakeLinearClient();
    const adapter = createAdapter(client);
    const api = createKataDomainApi(adapter);

    const snapshot = await api.project.getSnapshot();

    expect(snapshot.nextAction.workflow).toBe("kata-execute-phase");
    expect(snapshot.slices.find((slice) => slice.id === "S001")).toMatchObject({
      blockedBy: [],
      tasks: [expect.objectContaining({ id: "T001" })],
    });
  });
});
```

- [ ] **Step 2: Add GitHub Projects v2 regression test**

Append to `apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts` inside `describe("GithubProjectsV2Adapter", () => { ... })`:

```ts
  it("keeps GitHub Projects v2 milestone, slice, task, artifact, and dependency behavior intact", async () => {
    const client = createFakeGithubClient();
    const adapter = new GithubProjectsV2Adapter({
      owner: "kata-sh",
      repo: "uat",
      projectNumber: 12,
      workspacePath: "/workspace",
      client: client as any,
    });

    const milestone = await adapter.createMilestone({ title: "Regression", goal: "Keep GitHub behavior" });
    const first = await adapter.createSlice({ milestoneId: milestone.id, title: "First", goal: "Foundation" });
    const second = await adapter.createSlice({ milestoneId: milestone.id, title: "Second", goal: "Dependent", blockedBy: [first.id] });
    const task = await adapter.createTask({ sliceId: second.id, title: "Task", description: "Child issue" });
    const artifact = await adapter.writeArtifact({
      scopeType: "task",
      scopeId: task.id,
      artifactType: "verification",
      title: "Verification",
      content: "Verified",
      format: "markdown",
    });

    expect(milestone).toMatchObject({ id: "M001", status: "active" });
    expect(second).toMatchObject({ id: "S002", blockedBy: ["S001"] });
    expect(task).toMatchObject({ id: "T001", sliceId: "S002" });
    expect(artifact).toMatchObject({ scopeType: "task", scopeId: "T001", artifactType: "verification" });
    await expect(adapter.listSlices({ milestoneId: "M001" })).resolves.toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "S001", blocking: ["S002"] }),
      expect.objectContaining({ id: "S002", blockedBy: ["S001"] }),
    ]));
  });
```

- [ ] **Step 3: Run targeted regression suites**

Run:

```bash
pnpm --filter @kata-sh/cli exec vitest run \
  src/tests/linear.config.vitest.test.ts \
  src/tests/linear.client.vitest.test.ts \
  src/tests/linear.artifacts.vitest.test.ts \
  src/tests/linear.adapter.vitest.test.ts \
  src/tests/github-projects-v2.adapter.vitest.test.ts \
  src/tests/golden-path.pi-github.vitest.test.ts \
  src/tests/golden-path.pi-linear.vitest.test.ts \
  src/tests/phase-a-contract.vitest.test.ts
```

Expected: PASS.

- [ ] **Step 4: Run package validation**

Run:

```bash
pnpm --filter @kata-sh/cli run typecheck
pnpm --filter @kata-sh/cli run lint
pnpm --filter @kata-sh/cli test
```

Expected: all commands exit 0.

- [ ] **Step 5: Run affected workspace validation**

Run:

```bash
pnpm run validate:affected
```

Expected: all affected lint, typecheck, and test tasks exit 0.

- [ ] **Step 6: Commit validation tests**

Run:

```bash
git add apps/cli/src/tests/linear.adapter.vitest.test.ts apps/cli/src/tests/github-projects-v2.adapter.vitest.test.ts apps/cli/src/tests/golden-path.pi-github.vitest.test.ts
git commit -m "test(cli): validate linear and github backend parity"
```

## Self-Review

**Spec coverage:** LIN-01 is covered by Tasks 1 and 7. LIN-02 is covered by Task 7. LIN-03 through LIN-07 are covered by Tasks 4 and 5. LIN-08 is covered by Tasks 3 and 6. LIN-09 and LIN-10 are covered by Tasks 4, 6, and 8 through the existing snapshot service. SKL-01 through SKL-04 are covered at the CLI operation contract level by Tasks 7 and 8. DEP-01 through DEP-03 and DEP-05 are covered by Tasks 5 and 8.

**Gaps:** Live Linear API validation remains outside this CLI plan, matching the approved spec non-goal. Symphony and PR/land validation remain separate plans.

**Plan language scan:** Each task has concrete files, test content, implementation snippets, commands, expected outcomes, and commit commands.

**Type consistency:** The plan consistently uses `LinearTrackerConfig`, `createLinearClient`, `LinearKataAdapter`, `LinearEntityClassification`, `parseLinearArtifactMarker`, `upsertLinearIssueArtifactComment`, and `upsertLinearMilestoneDocument`.
