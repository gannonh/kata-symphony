---
type: Plan
title: Pi Symphony Extension Slice 1
description: "Archived implementation plan: Pi Symphony Extension Slice 1."
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Pi Symphony Extension Slice 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Slice 1 of `@kata-sh/pi-symphony-extension`: initialize, doctor, start, attach, status, stop, help, and a Pi-native health dashboard.

**Architecture:** Create a self-contained Pi package under `apps/symphony/pi-extension`. Commands and LLM tools share one runtime object that owns extension state, binary resolution, process lifecycle, HTTP client attachment, and dashboard launch. Slice 1 renders health/status only: connection, project link, polling status, issue counts, and owned-process metadata.

**Tech Stack:** TypeScript, Pi extension APIs from `@earendil-works/pi-coding-agent`, TUI primitives from `@earendil-works/pi-tui`, Vitest, Node built-ins, pnpm workspace scripts.

---

## Source spec

Master design doc: `docs/superpowers/specs/2026-05-14-pi-symphony-extension-design.md`

Slice 1 requirements:

- Resolve and validate the Symphony binary.
- Start headless Symphony or attach to an existing server.
- Render connection status, project link, polling status, worker counts, and basic process ownership.
- Cover slash commands and tools for init, doctor, start, attach, status, stop, and help.

## File structure

Create these files:

- `apps/symphony/pi-extension/package.json` — Pi package manifest and package-local scripts.
- `apps/symphony/pi-extension/tsconfig.json` — strict TypeScript config for source and tests.
- `apps/symphony/pi-extension/vitest.config.ts` — Vitest config for colocated tests.
- `apps/symphony/pi-extension/src/index.ts` — Pi extension entrypoint; wires commands, tools, and shutdown cleanup.
- `apps/symphony/pi-extension/src/runtime.ts` — shared runtime object for state, resolver, process manager, client, and status helpers.
- `apps/symphony/pi-extension/src/state.ts` — serializable extension state and restore/persist helpers.
- `apps/symphony/pi-extension/src/command-args.ts` — slash-command argument parsing.
- `apps/symphony/pi-extension/src/errors.ts` — typed extension errors and display helpers.
- `apps/symphony/pi-extension/src/binary-resolver.ts` — binary lookup, validation, prompting, and persistence.
- `apps/symphony/pi-extension/src/http-client.ts` — Symphony HTTP API client and error normalization.
- `apps/symphony/pi-extension/src/process-manager.ts` — owned child process startup, HTTP URL discovery, readiness polling, and stop behavior.
- `apps/symphony/pi-extension/src/dashboard.ts` — Slice 1 TUI health dashboard component and launcher.
- `apps/symphony/pi-extension/src/commands.ts` — slash commands.
- `apps/symphony/pi-extension/src/tools.ts` — LLM-callable tools.
- `apps/symphony/pi-extension/README.md` — local install and smoke-test notes.

Tests:

- `apps/symphony/pi-extension/src/command-args.test.ts`
- `apps/symphony/pi-extension/src/binary-resolver.test.ts`
- `apps/symphony/pi-extension/src/http-client.test.ts`
- `apps/symphony/pi-extension/src/process-manager.test.ts`
- `apps/symphony/pi-extension/src/dashboard.test.ts`
- `apps/symphony/pi-extension/src/package-smoke.test.ts`

Modify these files:

- `turbo.json` — add package manifest/config files to task inputs so this package participates in cache invalidation.

Do not modify Symphony Rust source for Slice 1.

---

### Task 1: Package scaffold

**Files:**
- Create: `apps/symphony/pi-extension/package.json`
- Create: `apps/symphony/pi-extension/tsconfig.json`
- Create: `apps/symphony/pi-extension/vitest.config.ts`
- Create: `apps/symphony/pi-extension/README.md`
- Modify: `turbo.json`
- Test: `apps/symphony/pi-extension/src/package-smoke.test.ts`

- [ ] **Step 1: Create the package manifest**

Create `apps/symphony/pi-extension/package.json`:

```json
{
  "name": "@kata-sh/pi-symphony-extension",
  "version": "0.1.0",
  "description": "Pi extension for launching, attaching to, and monitoring Kata Symphony",
  "license": "MIT",
  "private": false,
  "type": "module",
  "keywords": ["pi-package", "symphony", "kata"],
  "repository": {
    "type": "git",
    "url": "git+https://github.com/gannonh/kata.git",
    "directory": "apps/symphony/pi-extension"
  },
  "exports": {
    ".": "./src/index.ts",
    "./package.json": "./package.json"
  },
  "files": ["src", "README.md", "package.json"],
  "scripts": {
    "build": "tsc --noEmit",
    "typecheck": "tsc --noEmit",
    "test": "pnpm exec vitest run",
    "test:watch": "pnpm exec vitest",
    "lint": "eslint src/ --max-warnings=0"
  },
  "pi": {
    "extensions": ["./src/index.ts"]
  },
  "peerDependencies": {
    "@earendil-works/pi-ai": "*",
    "@earendil-works/pi-coding-agent": "*",
    "@earendil-works/pi-tui": "*",
    "typebox": "*"
  },
  "devDependencies": {
    "@types/node": "^25.5.0",
    "typescript": "^6.0.3",
    "vitest": "^4.1.5"
  },
  "engines": {
    "node": ">=20.6.0"
  }
}
```

- [ ] **Step 2: Create TypeScript config**

Create `apps/symphony/pi-extension/tsconfig.json`:

```json
{
  "compilerOptions": {
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "target": "ES2022",
    "lib": ["ES2022", "DOM"],
    "types": ["node", "vitest/globals"],
    "strict": true,
    "declaration": false,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "noEmit": true,
    "allowImportingTsExtensions": true
  },
  "include": ["src/**/*.ts"]
}
```

- [ ] **Step 3: Create Vitest config**

Create `apps/symphony/pi-extension/vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    exclude: ["dist/**", "node_modules/**"],
  },
});
```

- [ ] **Step 4: Create smoke test for manifest**

Create `apps/symphony/pi-extension/src/package-smoke.test.ts`:

```ts
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const packagePath = resolve(here, "../package.json");

describe("pi package manifest", () => {
  it("declares the Pi extension entrypoint and package identity", async () => {
    const manifest = JSON.parse(await readFile(packagePath, "utf8")) as {
      name?: string;
      keywords?: string[];
      pi?: { extensions?: string[] };
      peerDependencies?: Record<string, string>;
    };

    expect(manifest.name).toBe("@kata-sh/pi-symphony-extension");
    expect(manifest.keywords).toContain("pi-package");
    expect(manifest.pi?.extensions).toEqual(["./src/index.ts"]);
    expect(manifest.peerDependencies?.["@earendil-works/pi-coding-agent"]).toBe("*");
    expect(manifest.peerDependencies?.["@earendil-works/pi-tui"]).toBe("*");
  });
});
```

- [ ] **Step 5: Create README**

Create `apps/symphony/pi-extension/README.md`:

```md
# @kata-sh/pi-symphony-extension

Pi extension for initializing, launching, attaching to, and monitoring Kata Symphony.

## Local development

```sh
pnpm --dir apps/symphony/pi-extension test
pnpm --dir apps/symphony/pi-extension typecheck
pi -e ./apps/symphony/pi-extension
```

## Commands in Slice 1

- `/symphony:help`
- `/symphony:init [--force]`
- `/symphony:doctor [workflow]`
- `/symphony:start [workflow]`
- `/symphony:attach <url>`
- `/symphony:dashboard`
- `/symphony:status`
- `/symphony:stop`
```

- [ ] **Step 6: Update Turbo task inputs**

Modify `turbo.json` so `typecheck`, `test`, and `build` include package metadata/config files. Replace each relevant `inputs` array with the same entries plus these strings:

```json
"package.json",
"vitest.config.ts",
"vitest.config.mts",
"vitest.config.js"
```

For `test`, also include:

```json
"src/**/*.test.ts"
```

- [ ] **Step 7: Run the package smoke test**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/package-smoke.test.ts
```

Expected: PASS for `pi package manifest`.

- [ ] **Step 8: Commit scaffold**

```bash
git add apps/symphony/pi-extension/package.json apps/symphony/pi-extension/tsconfig.json apps/symphony/pi-extension/vitest.config.ts apps/symphony/pi-extension/README.md apps/symphony/pi-extension/src/package-smoke.test.ts turbo.json
git commit -m "feat(pi-symphony): scaffold pi extension package"
```

---

### Task 2: State, errors, and command argument parsing

**Files:**
- Create: `apps/symphony/pi-extension/src/state.ts`
- Create: `apps/symphony/pi-extension/src/errors.ts`
- Create: `apps/symphony/pi-extension/src/command-args.ts`
- Test: `apps/symphony/pi-extension/src/command-args.test.ts`

- [ ] **Step 1: Write argument parsing tests**

Create `apps/symphony/pi-extension/src/command-args.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { parseAttachArgs, parseDoctorArgs, parseInitArgs, parseStartArgs } from "./command-args.ts";

describe("command argument parsing", () => {
  it("parses init force flag", () => {
    expect(parseInitArgs("--force")).toEqual({ force: true });
    expect(parseInitArgs("")).toEqual({ force: false });
  });

  it("rejects unknown init flags", () => {
    expect(() => parseInitArgs("--bad")).toThrow("Unknown /symphony:init option: --bad");
  });

  it("keeps workflow arguments as one path string", () => {
    expect(parseDoctorArgs(".symphony/WORKFLOW.md")).toEqual({ workflow: ".symphony/WORKFLOW.md" });
    expect(parseStartArgs("/tmp/My Workflow.md")).toEqual({ workflow: "/tmp/My Workflow.md" });
    expect(parseStartArgs("   ")).toEqual({ workflow: undefined });
  });

  it("requires an attach URL", () => {
    expect(parseAttachArgs("http://127.0.0.1:8080")).toEqual({ url: "http://127.0.0.1:8080" });
    expect(() => parseAttachArgs("")).toThrow("Usage: /symphony:attach <url>");
  });
});
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/command-args.test.ts
```

Expected: FAIL because `command-args.ts` does not exist.

- [ ] **Step 3: Implement state**

Create `apps/symphony/pi-extension/src/state.ts`:

```ts
export const STATE_ENTRY_TYPE = "symphony-extension-state";

export interface OwnedProcessMetadata {
  pid: number;
  command: string;
  cwd: string;
  baseUrl?: string;
  startedAt: string;
}

export interface LastKnownSymphonyState {
  baseUrl: string;
  trackerProjectUrl?: string;
  runningCount: number;
  retryCount: number;
  blockedCount: number;
  completedCount: number;
  pollingChecking: boolean;
  nextPollInMs: number;
  updatedAt: string;
}

export interface ExtensionState {
  binaryPath?: string;
  attachedBaseUrl?: string;
  ownedProcess?: OwnedProcessMetadata;
  dashboard: {
    showDetails: boolean;
  };
  stopOwnedOnShutdown: boolean;
  lastKnownState?: LastKnownSymphonyState;
}

export function createDefaultState(): ExtensionState {
  return {
    dashboard: { showDetails: false },
    stopOwnedOnShutdown: true,
  };
}

export function normalizeBaseUrl(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) throw new Error("URL must not be empty");
  const parsed = new URL(trimmed);
  parsed.pathname = parsed.pathname.replace(/\/+$/, "");
  parsed.search = "";
  parsed.hash = "";
  return parsed.toString().replace(/\/$/, "");
}

export function restoreStateFromEntries(entries: Array<{ type?: string; customType?: string; data?: unknown }>): ExtensionState {
  const state = createDefaultState();
  for (const entry of entries) {
    if (entry.type !== "custom" || entry.customType !== STATE_ENTRY_TYPE) continue;
    if (!entry.data || typeof entry.data !== "object") continue;
    const data = entry.data as Partial<ExtensionState>;
    if (typeof data.binaryPath === "string") state.binaryPath = data.binaryPath;
    if (typeof data.attachedBaseUrl === "string") state.attachedBaseUrl = data.attachedBaseUrl;
    if (typeof data.stopOwnedOnShutdown === "boolean") state.stopOwnedOnShutdown = data.stopOwnedOnShutdown;
    if (data.dashboard && typeof data.dashboard.showDetails === "boolean") {
      state.dashboard.showDetails = data.dashboard.showDetails;
    }
    if (data.ownedProcess) state.ownedProcess = data.ownedProcess;
    if (data.lastKnownState) state.lastKnownState = data.lastKnownState;
  }
  return state;
}

export function snapshotStateForPersistence(state: ExtensionState): ExtensionState {
  return {
    binaryPath: state.binaryPath,
    attachedBaseUrl: state.attachedBaseUrl,
    ownedProcess: state.ownedProcess,
    dashboard: { ...state.dashboard },
    stopOwnedOnShutdown: state.stopOwnedOnShutdown,
    lastKnownState: state.lastKnownState,
  };
}
```

- [ ] **Step 4: Implement typed errors**

Create `apps/symphony/pi-extension/src/errors.ts`:

```ts
export type SymphonyExtensionErrorKind =
  | "missing_binary"
  | "invalid_binary"
  | "command_failed"
  | "start_timeout"
  | "attach_unreachable"
  | "non_symphony_response"
  | "invalid_json"
  | "api_error"
  | "no_attachment"
  | "not_owned";

export class SymphonyExtensionError extends Error {
  readonly kind: SymphonyExtensionErrorKind;
  readonly details: Record<string, unknown>;

  constructor(kind: SymphonyExtensionErrorKind, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "SymphonyExtensionError";
    this.kind = kind;
    this.details = details;
  }
}

export function formatError(error: unknown): string {
  if (error instanceof SymphonyExtensionError) {
    const detailText = Object.keys(error.details).length > 0 ? `\n${JSON.stringify(error.details, null, 2)}` : "";
    return `${error.message}${detailText}`;
  }
  if (error instanceof Error) return error.message;
  return String(error);
}
```

- [ ] **Step 5: Implement command argument parsing**

Create `apps/symphony/pi-extension/src/command-args.ts`:

```ts
export interface InitArgs {
  force: boolean;
}

export interface WorkflowArgs {
  workflow?: string;
}

export interface AttachArgs {
  url: string;
}

export function parseInitArgs(args: string): InitArgs {
  const tokens = args.trim().split(/\s+/).filter(Boolean);
  let force = false;
  for (const token of tokens) {
    if (token === "--force") {
      force = true;
      continue;
    }
    throw new Error(`Unknown /symphony:init option: ${token}`);
  }
  return { force };
}

export function parseDoctorArgs(args: string): WorkflowArgs {
  return parseWorkflowArg(args);
}

export function parseStartArgs(args: string): WorkflowArgs {
  return parseWorkflowArg(args);
}

function parseWorkflowArg(args: string): WorkflowArgs {
  const workflow = args.trim();
  return workflow ? { workflow } : { workflow: undefined };
}

export function parseAttachArgs(args: string): AttachArgs {
  const url = args.trim();
  if (!url) throw new Error("Usage: /symphony:attach <url>");
  return { url };
}
```

- [ ] **Step 6: Run parsing tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/command-args.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit state and parsing**

```bash
git add apps/symphony/pi-extension/src/state.ts apps/symphony/pi-extension/src/errors.ts apps/symphony/pi-extension/src/command-args.ts apps/symphony/pi-extension/src/command-args.test.ts
git commit -m "feat(pi-symphony): add extension state and command parsing"
```

---

### Task 3: Symphony binary resolver

**Files:**
- Create: `apps/symphony/pi-extension/src/binary-resolver.ts`
- Test: `apps/symphony/pi-extension/src/binary-resolver.test.ts`

- [ ] **Step 1: Write resolver tests**

Create `apps/symphony/pi-extension/src/binary-resolver.test.ts`:

```ts
import { chmod, mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { resolveSymphonyBinary, validateBinaryPath } from "./binary-resolver.ts";
import { createDefaultState } from "./state.ts";

async function executable(path: string): Promise<string> {
  await mkdir(resolve(path, ".."), { recursive: true });
  await writeFile(path, "#!/bin/sh\necho symphony\n", "utf8");
  await chmod(path, 0o755);
  return path;
}

describe("Symphony binary resolver", () => {
  it("uses SYMPHONY_BIN first", async () => {
    const dir = await mkdtemp(join(tmpdir(), "pi-symphony-bin-"));
    const binary = await executable(join(dir, "custom-symphony"));
    const state = createDefaultState();

    const resolved = await resolveSymphonyBinary({
      cwd: dir,
      state,
      env: { SYMPHONY_BIN: binary, PATH: "" },
      promptForPath: async () => undefined,
    });

    expect(resolved).toBe(binary);
  });

  it("finds repo-local target release binary by walking upward", async () => {
    const root = await mkdtemp(join(tmpdir(), "pi-symphony-repo-"));
    const nested = join(root, "packages", "x");
    const binary = await executable(join(root, "apps", "symphony", "target", "release", "symphony"));
    await mkdir(nested, { recursive: true });

    const resolved = await resolveSymphonyBinary({
      cwd: nested,
      state: createDefaultState(),
      env: { PATH: "" },
      promptForPath: async () => undefined,
    });

    expect(resolved).toBe(binary);
  });

  it("uses persisted user path after built-in checks fail", async () => {
    const dir = await mkdtemp(join(tmpdir(), "pi-symphony-persisted-"));
    const binary = await executable(join(dir, "persisted"));
    const state = createDefaultState();
    state.binaryPath = binary;

    await expect(resolveSymphonyBinary({ cwd: dir, state, env: { PATH: "" }, promptForPath: async () => undefined })).resolves.toBe(binary);
  });

  it("prompts for an absolute path and persists it", async () => {
    const dir = await mkdtemp(join(tmpdir(), "pi-symphony-prompt-"));
    const binary = await executable(join(dir, "prompted"));
    const state = createDefaultState();

    const resolved = await resolveSymphonyBinary({
      cwd: dir,
      state,
      env: { PATH: "" },
      promptForPath: async () => binary,
    });

    expect(resolved).toBe(binary);
    expect(state.binaryPath).toBe(binary);
  });

  it("rejects relative binary paths", async () => {
    await expect(validateBinaryPath("relative/symphony")).rejects.toThrow("Symphony binary path must be absolute");
  });
});
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/binary-resolver.test.ts
```

Expected: FAIL because `binary-resolver.ts` does not exist.

- [ ] **Step 3: Implement resolver**

Create `apps/symphony/pi-extension/src/binary-resolver.ts`:

```ts
import { access } from "node:fs/promises";
import { constants } from "node:fs";
import { delimiter, dirname, isAbsolute, join } from "node:path";
import type { ExtensionState } from "./state.ts";
import { SymphonyExtensionError } from "./errors.ts";

export interface ResolveBinaryOptions {
  cwd: string;
  state: ExtensionState;
  env?: NodeJS.ProcessEnv;
  promptForPath?: () => Promise<string | undefined>;
}

export async function resolveSymphonyBinary(options: ResolveBinaryOptions): Promise<string> {
  const env = options.env ?? process.env;
  const candidates: string[] = [];

  if (env.SYMPHONY_BIN) candidates.push(env.SYMPHONY_BIN);

  const repoLocal = await findRepoLocalBinary(options.cwd);
  if (repoLocal) candidates.push(repoLocal);

  const pathBinary = await findOnPath("symphony", env.PATH ?? "");
  if (pathBinary) candidates.push(pathBinary);

  if (options.state.binaryPath) candidates.push(options.state.binaryPath);

  for (const candidate of candidates) {
    try {
      return await validateBinaryPath(candidate);
    } catch {
      continue;
    }
  }

  const prompted = await options.promptForPath?.();
  if (prompted) {
    const validated = await validateBinaryPath(prompted);
    options.state.binaryPath = validated;
    return validated;
  }

  throw new SymphonyExtensionError(
    "missing_binary",
    "Could not find Symphony binary. Set SYMPHONY_BIN, build apps/symphony/target/release/symphony, add symphony to PATH, or provide an absolute path.",
  );
}

export async function validateBinaryPath(path: string): Promise<string> {
  if (!isAbsolute(path)) {
    throw new SymphonyExtensionError("invalid_binary", "Symphony binary path must be absolute", { path });
  }
  try {
    await access(path, constants.X_OK);
  } catch (error) {
    throw new SymphonyExtensionError("invalid_binary", "Symphony binary path is not executable", {
      path,
      cause: error instanceof Error ? error.message : String(error),
    });
  }
  return path;
}

async function findRepoLocalBinary(cwd: string): Promise<string | undefined> {
  let current = cwd;
  while (true) {
    const candidate = join(current, "apps", "symphony", "target", "release", process.platform === "win32" ? "symphony.exe" : "symphony");
    try {
      await validateBinaryPath(candidate);
      return candidate;
    } catch {
      const parent = dirname(current);
      if (parent === current) return undefined;
      current = parent;
    }
  }
}

async function findOnPath(command: string, pathValue: string): Promise<string | undefined> {
  for (const dir of pathValue.split(delimiter).filter(Boolean)) {
    const candidate = join(dir, process.platform === "win32" ? `${command}.exe` : command);
    try {
      await validateBinaryPath(candidate);
      return candidate;
    } catch {
      continue;
    }
  }
  return undefined;
}
```

- [ ] **Step 4: Run resolver tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/binary-resolver.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit resolver**

```bash
git add apps/symphony/pi-extension/src/binary-resolver.ts apps/symphony/pi-extension/src/binary-resolver.test.ts
git commit -m "feat(pi-symphony): resolve symphony binary"
```

---

### Task 4: Symphony HTTP client for health

**Files:**
- Create: `apps/symphony/pi-extension/src/http-client.ts`
- Test: `apps/symphony/pi-extension/src/http-client.test.ts`

- [ ] **Step 1: Write HTTP client tests**

Create `apps/symphony/pi-extension/src/http-client.test.ts`:

```ts
import { createServer, type Server } from "node:http";
import { afterEach, describe, expect, it } from "vitest";
import { SymphonyHttpClient } from "./http-client.ts";
import { SymphonyExtensionError } from "./errors.ts";

let server: Server | undefined;

async function serve(handler: (req: { method?: string; url?: string }, body: string) => { status: number; body: unknown; contentType?: string }): Promise<string> {
  server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += String(chunk)));
    req.on("end", () => {
      const response = handler(req, body);
      res.statusCode = response.status;
      res.setHeader("content-type", response.contentType ?? "application/json");
      res.end(typeof response.body === "string" ? response.body : JSON.stringify(response.body));
    });
  });
  await new Promise<void>((resolve) => server!.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("expected TCP address");
  return `http://127.0.0.1:${address.port}`;
}

afterEach(async () => {
  if (!server) return;
  await new Promise<void>((resolve, reject) => server!.close((error) => (error ? reject(error) : resolve())));
  server = undefined;
});

describe("SymphonyHttpClient", () => {
  it("fetches state and summarizes health", async () => {
    const baseUrl = await serve((req) => {
      expect(req.url).toBe("/api/v1/state");
      return {
        status: 200,
        body: {
          tracker_project_url: "https://github.com/gannonh/kata/projects/1",
          running: { one: { issue_identifier: "KAT-1" } },
          retry_queue: [{ identifier: "KAT-2" }],
          blocked: [],
          completed: [{ identifier: "KAT-3" }],
          polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000, poll_count: 2 },
        },
      };
    });

    const client = new SymphonyHttpClient(baseUrl);
    const state = await client.getState();
    expect(state.tracker_project_url).toBe("https://github.com/gannonh/kata/projects/1");
    expect(client.toHealthSummary(state).runningCount).toBe(1);
    expect(client.toHealthSummary(state).retryCount).toBe(1);
    expect(client.toHealthSummary(state).completedCount).toBe(1);
  });

  it("normalizes Symphony API error envelopes", async () => {
    const baseUrl = await serve(() => ({
      status: 409,
      body: { error: { code: "no_active_session", message: "issue has no active RPC session", status: 409 } },
    }));

    const client = new SymphonyHttpClient(baseUrl);
    await expect(client.getState()).rejects.toMatchObject<SymphonyExtensionError>({
      kind: "api_error",
      message: "issue has no active RPC session",
    });
  });

  it("rejects invalid JSON", async () => {
    const baseUrl = await serve(() => ({ status: 200, body: "not-json", contentType: "application/json" }));
    const client = new SymphonyHttpClient(baseUrl);
    await expect(client.getState()).rejects.toMatchObject<SymphonyExtensionError>({ kind: "invalid_json" });
  });
});
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/http-client.test.ts
```

Expected: FAIL because `http-client.ts` does not exist.

- [ ] **Step 3: Implement HTTP client**

Create `apps/symphony/pi-extension/src/http-client.ts`:

```ts
import { SymphonyExtensionError } from "./errors.ts";
import { normalizeBaseUrl, type LastKnownSymphonyState } from "./state.ts";

export interface SymphonyStateResponse {
  tracker_project_url?: string;
  running?: Record<string, unknown>;
  retry_queue?: unknown[];
  blocked?: unknown[];
  completed?: unknown[];
  polling?: {
    checking?: boolean;
    next_poll_in_ms?: number;
    poll_interval_ms?: number;
    poll_count?: number;
    last_poll_at?: string;
  };
}

interface ApiErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    status?: number;
    details?: unknown;
  };
}

export class SymphonyHttpClient {
  readonly baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = normalizeBaseUrl(baseUrl);
  }

  async getState(signal?: AbortSignal): Promise<SymphonyStateResponse> {
    return this.requestJson<SymphonyStateResponse>("/api/v1/state", { method: "GET", signal });
  }

  async verify(signal?: AbortSignal): Promise<SymphonyStateResponse> {
    const state = await this.getState(signal);
    if (!state || typeof state !== "object") {
      throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony state", { baseUrl: this.baseUrl });
    }
    return state;
  }

  toHealthSummary(state: SymphonyStateResponse): LastKnownSymphonyState {
    return {
      baseUrl: this.baseUrl,
      trackerProjectUrl: state.tracker_project_url,
      runningCount: Object.keys(state.running ?? {}).length,
      retryCount: state.retry_queue?.length ?? 0,
      blockedCount: state.blocked?.length ?? 0,
      completedCount: state.completed?.length ?? 0,
      pollingChecking: Boolean(state.polling?.checking),
      nextPollInMs: state.polling?.next_poll_in_ms ?? 0,
      updatedAt: new Date().toISOString(),
    };
  }

  private async requestJson<T>(path: string, init: RequestInit): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    let response: Response;
    try {
      response = await fetch(url, { ...init, headers: { accept: "application/json", ...(init.headers ?? {}) } });
    } catch (error) {
      throw new SymphonyExtensionError("attach_unreachable", "Could not reach Symphony HTTP API", {
        url,
        cause: error instanceof Error ? error.message : String(error),
      });
    }

    const text = await response.text();
    let json: unknown;
    try {
      json = text ? JSON.parse(text) : {};
    } catch (error) {
      throw new SymphonyExtensionError("invalid_json", "Symphony HTTP API returned invalid JSON", {
        url,
        status: response.status,
        bodyPreview: text.slice(0, 200),
        cause: error instanceof Error ? error.message : String(error),
      });
    }

    if (!response.ok) {
      const envelope = json as ApiErrorEnvelope;
      if (envelope.error?.message) {
        throw new SymphonyExtensionError("api_error", envelope.error.message, {
          url,
          status: response.status,
          code: envelope.error.code,
          details: envelope.error.details,
        });
      }
      throw new SymphonyExtensionError("non_symphony_response", "Symphony HTTP API returned an unexpected error response", {
        url,
        status: response.status,
        body: json,
      });
    }

    return json as T;
  }
}
```

- [ ] **Step 4: Run HTTP client tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/http-client.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit client**

```bash
git add apps/symphony/pi-extension/src/http-client.ts apps/symphony/pi-extension/src/http-client.test.ts
git commit -m "feat(pi-symphony): add symphony http client"
```

---

### Task 5: Owned Symphony process manager

**Files:**
- Create: `apps/symphony/pi-extension/src/process-manager.ts`
- Test: `apps/symphony/pi-extension/src/process-manager.test.ts`

- [ ] **Step 1: Write process manager tests**

Create `apps/symphony/pi-extension/src/process-manager.test.ts`:

```ts
import { chmod, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer, type Server } from "node:http";
import { afterEach, describe, expect, it } from "vitest";
import { SymphonyProcessManager } from "./process-manager.ts";
import { createDefaultState } from "./state.ts";

let server: Server | undefined;

async function stateServer(): Promise<string> {
  server = createServer((_req, res) => {
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ running: {}, retry_queue: [], blocked: [], completed: [], polling: { checking: false, next_poll_in_ms: 0 } }));
  });
  await new Promise<void>((resolve) => server!.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("expected TCP address");
  return `http://127.0.0.1:${address.port}`;
}

afterEach(async () => {
  if (!server) return;
  await new Promise<void>((resolve, reject) => server!.close((error) => (error ? reject(error) : resolve())));
  server = undefined;
});

describe("SymphonyProcessManager", () => {
  it("starts Symphony with --no-tui and records owned process metadata", async () => {
    const baseUrl = await stateServer();
    const dir = await mkdtemp(join(tmpdir(), "pi-symphony-process-"));
    const script = join(dir, "fake-symphony.sh");
    await writeFile(script, `#!/bin/sh\necho "dashboard listening at ${baseUrl}"\nsleep 30\n`, "utf8");
    await chmod(script, 0o755);

    const state = createDefaultState();
    const manager = new SymphonyProcessManager(state);
    const started = await manager.start({ binary: script, cwd: dir, workflow: ".symphony/WORKFLOW.md", timeoutMs: 2000 });

    expect(started.baseUrl).toBe(baseUrl);
    expect(started.owned).toBe(true);
    expect(state.ownedProcess?.command).toContain("--no-tui");
    expect(state.ownedProcess?.command).toContain(".symphony/WORKFLOW.md");

    await manager.stopOwned();
  });

  it("does not stop when no owned child exists", async () => {
    const state = createDefaultState();
    const manager = new SymphonyProcessManager(state);
    await expect(manager.stopOwned()).rejects.toThrow("No Symphony process owned by this extension");
  });
});
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/process-manager.test.ts
```

Expected: FAIL because `process-manager.ts` does not exist.

- [ ] **Step 3: Implement process manager**

Create `apps/symphony/pi-extension/src/process-manager.ts`:

```ts
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { SymphonyExtensionError } from "./errors.ts";
import { SymphonyHttpClient } from "./http-client.ts";
import type { ExtensionState } from "./state.ts";

export interface StartOptions {
  binary: string;
  cwd: string;
  workflow?: string;
  timeoutMs?: number;
}

export interface StartResult {
  baseUrl: string;
  owned: true;
  pid: number;
}

export class SymphonyProcessManager {
  private child?: ChildProcessWithoutNullStreams;
  private output = "";

  constructor(private readonly state: ExtensionState) {}

  async start(options: StartOptions): Promise<StartResult> {
    if (this.child && !this.child.killed) {
      throw new SymphonyExtensionError("command_failed", "Symphony is already running as an owned child process", {
        pid: this.child.pid,
      });
    }

    const args = options.workflow ? [options.workflow, "--no-tui"] : ["--no-tui"];
    this.output = "";
    this.child = spawn(options.binary, args, { cwd: options.cwd, stdio: "pipe" });
    const pid = this.child.pid;
    if (!pid) throw new SymphonyExtensionError("command_failed", "Failed to spawn Symphony process");

    this.child.stdout.on("data", (chunk) => (this.output += String(chunk)));
    this.child.stderr.on("data", (chunk) => (this.output += String(chunk)));

    const baseUrl = await this.waitForReady(options.cwd, options.workflow, options.timeoutMs ?? 10_000);
    this.state.attachedBaseUrl = baseUrl;
    this.state.ownedProcess = {
      pid,
      command: [options.binary, ...args].join(" "),
      cwd: options.cwd,
      baseUrl,
      startedAt: new Date().toISOString(),
    };

    return { baseUrl, owned: true, pid };
  }

  async stopOwned(): Promise<void> {
    if (!this.child || this.child.killed) {
      this.state.ownedProcess = undefined;
      throw new SymphonyExtensionError("not_owned", "No Symphony process owned by this extension is running");
    }

    const child = this.child;
    child.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      const timeout = setTimeout(() => {
        if (!child.killed) child.kill("SIGKILL");
        resolve();
      }, 2000);
      child.once("exit", () => {
        clearTimeout(timeout);
        resolve();
      });
    });

    this.child = undefined;
    this.state.ownedProcess = undefined;
  }

  async shutdown(): Promise<void> {
    if (!this.state.stopOwnedOnShutdown) return;
    if (!this.child || this.child.killed) return;
    await this.stopOwned();
  }

  private async waitForReady(cwd: string, workflow: string | undefined, timeoutMs: number): Promise<string> {
    const started = Date.now();
    let lastError: unknown;

    while (Date.now() - started < timeoutMs) {
      const baseUrl = this.detectBaseUrl(cwd, workflow);
      try {
        const client = new SymphonyHttpClient(baseUrl);
        await client.verify();
        return baseUrl;
      } catch (error) {
        lastError = error;
      }

      if (this.child?.exitCode !== null) break;
      await new Promise((resolve) => setTimeout(resolve, 150));
    }

    throw new SymphonyExtensionError("start_timeout", "Timed out waiting for Symphony HTTP API", {
      expectedBaseUrl: this.detectBaseUrl(cwd, workflow),
      output: this.output.slice(-4000),
      childExitCode: this.child?.exitCode,
      cause: lastError instanceof Error ? lastError.message : String(lastError),
    });
  }

  private detectBaseUrl(cwd: string, workflow: string | undefined): string {
    const outputMatch = this.output.match(/https?:\/\/(?:127\.0\.0\.1|localhost):\d+/);
    if (outputMatch) return outputMatch[0];

    const workflowPath = workflow ? resolve(cwd, workflow) : join(cwd, ".symphony", "WORKFLOW.md");
    const configured = readWorkflowServerConfigSyncBestEffort(workflowPath);
    return `http://${configured.host}:${configured.port}`;
  }
}

function readWorkflowServerConfigSyncBestEffort(_workflowPath: string): { host: string; port: number } {
  return { host: "127.0.0.1", port: 8080 };
}
```

- [ ] **Step 4: Run process manager tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/process-manager.test.ts
```

Expected: PASS.

- [ ] **Step 5: Remove unused import if TypeScript reports it**

If `readFile` is unused, remove this line from `process-manager.ts`:

```ts
import { readFile } from "node:fs/promises";
```

Then rerun:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS.

- [ ] **Step 6: Commit process manager**

```bash
git add apps/symphony/pi-extension/src/process-manager.ts apps/symphony/pi-extension/src/process-manager.test.ts
git commit -m "feat(pi-symphony): start and stop owned symphony process"
```

---

### Task 6: Runtime orchestration

**Files:**
- Create: `apps/symphony/pi-extension/src/runtime.ts`

- [ ] **Step 1: Implement shared runtime**

Create `apps/symphony/pi-extension/src/runtime.ts`:

```ts
import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { resolveSymphonyBinary } from "./binary-resolver.ts";
import { formatError, SymphonyExtensionError } from "./errors.ts";
import { SymphonyHttpClient, type SymphonyStateResponse } from "./http-client.ts";
import { SymphonyProcessManager } from "./process-manager.ts";
import {
  createDefaultState,
  restoreStateFromEntries,
  snapshotStateForPersistence,
  STATE_ENTRY_TYPE,
  type ExtensionState,
} from "./state.ts";

export class SymphonyRuntime {
  state: ExtensionState = createDefaultState();
  readonly processManager = new SymphonyProcessManager(this.state);
  client?: SymphonyHttpClient;

  restore(ctx: ExtensionContext): void {
    this.state = restoreStateFromEntries(ctx.sessionManager.getEntries());
    this.client = this.state.attachedBaseUrl ? new SymphonyHttpClient(this.state.attachedBaseUrl) : undefined;
    Object.assign(this.processManager["state"], this.state);
  }

  persist(pi: { appendEntry: (customType: string, data?: unknown) => void }): void {
    pi.appendEntry(STATE_ENTRY_TYPE, snapshotStateForPersistence(this.state));
  }

  async resolveBinary(ctx: ExtensionContext): Promise<string> {
    return resolveSymphonyBinary({
      cwd: ctx.cwd,
      state: this.state,
      promptForPath: ctx.hasUI
        ? async () => ctx.ui.input("Symphony binary", "Absolute path to symphony executable")
        : async () => undefined,
    });
  }

  async attach(baseUrl: string): Promise<SymphonyStateResponse> {
    const client = new SymphonyHttpClient(baseUrl);
    const state = await client.verify();
    this.client = client;
    this.state.attachedBaseUrl = client.baseUrl;
    this.state.lastKnownState = client.toHealthSummary(state);
    return state;
  }

  async refreshState(): Promise<SymphonyStateResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const state = await this.client.getState();
    this.state.lastKnownState = this.client.toHealthSummary(state);
    return state;
  }

  statusText(): string {
    const attached = this.state.attachedBaseUrl ? `attached: ${this.state.attachedBaseUrl}` : "attached: no";
    const owned = this.state.ownedProcess ? `owned pid: ${this.state.ownedProcess.pid}` : "owned pid: none";
    const last = this.state.lastKnownState
      ? `running ${this.state.lastKnownState.runningCount}, retry ${this.state.lastKnownState.retryCount}, blocked ${this.state.lastKnownState.blockedCount}, completed ${this.state.lastKnownState.completedCount}`
      : "state: unknown";
    return `Symphony status\n${attached}\n${owned}\n${last}`;
  }

  errorText(error: unknown): string {
    return formatError(error);
  }
}
```

- [ ] **Step 2: Typecheck runtime**

Run:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS. If TypeScript rejects the private `state` access in `runtime.ts`, change `SymphonyProcessManager` constructor storage from `private readonly state` to `readonly state` and rerun.

- [ ] **Step 3: Commit runtime**

```bash
git add apps/symphony/pi-extension/src/runtime.ts apps/symphony/pi-extension/src/process-manager.ts
git commit -m "feat(pi-symphony): add shared extension runtime"
```

---

### Task 7: Health dashboard component

**Files:**
- Create: `apps/symphony/pi-extension/src/dashboard.ts`
- Test: `apps/symphony/pi-extension/src/dashboard.test.ts`

- [ ] **Step 1: Write dashboard tests**

Create `apps/symphony/pi-extension/src/dashboard.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { SymphonyDashboardComponent } from "./dashboard.ts";
import { createDefaultState } from "./state.ts";

describe("SymphonyDashboardComponent", () => {
  it("renders Slice 1 health fields", () => {
    const state = createDefaultState();
    state.attachedBaseUrl = "http://127.0.0.1:8080";
    state.ownedProcess = { pid: 123, command: "symphony --no-tui", cwd: "/repo", baseUrl: state.attachedBaseUrl, startedAt: "2026-05-14T00:00:00Z" };
    state.lastKnownState = {
      baseUrl: state.attachedBaseUrl,
      trackerProjectUrl: "https://github.com/gannonh/kata/projects/1",
      runningCount: 2,
      retryCount: 1,
      blockedCount: 0,
      completedCount: 4,
      pollingChecking: false,
      nextPollInMs: 5000,
      updatedAt: "2026-05-14T00:00:01Z",
    };

    const dashboard = new SymphonyDashboardComponent({
      state,
      refresh: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    const output = dashboard.render(120).join("\n");
    expect(output).toContain("Symphony Dashboard");
    expect(output).toContain("http://127.0.0.1:8080");
    expect(output).toContain("project: https://github.com/gannonh/kata/projects/1");
    expect(output).toContain("running: 2");
    expect(output).toContain("retry: 1");
    expect(output).toContain("owned process: pid 123");
  });

  it("closes on q", () => {
    const close = vi.fn();
    const dashboard = new SymphonyDashboardComponent({
      state: createDefaultState(),
      refresh: async () => undefined,
      close,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    dashboard.handleInput("q");
    expect(close).toHaveBeenCalledOnce();
  });
});
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/dashboard.test.ts
```

Expected: FAIL because `dashboard.ts` does not exist.

- [ ] **Step 3: Implement dashboard**

Create `apps/symphony/pi-extension/src/dashboard.ts`:

```ts
import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { matchesKey, truncateToWidth } from "@earendil-works/pi-tui";
import type { SymphonyRuntime } from "./runtime.ts";
import type { ExtensionState } from "./state.ts";

export interface DashboardOptions {
  state: ExtensionState;
  refresh: () => Promise<void>;
  close: () => void;
  requestRender: () => void;
  notify: (message: string, level: "info" | "warning" | "error") => void;
}

export class SymphonyDashboardComponent {
  private refreshing = false;

  constructor(private readonly options: DashboardOptions) {}

  handleInput(data: string): void {
    if (data === "q" || data === "Q" || matchesKey(data, "escape")) {
      this.options.close();
      return;
    }

    if (data === "r" || data === "R") {
      void this.refresh();
    }
  }

  render(width: number): string[] {
    const state = this.options.state;
    const health = state.lastKnownState;
    const lines = [
      "Symphony Dashboard",
      "",
      `connection: ${state.attachedBaseUrl ? "attached" : "detached"}`,
      `base url: ${state.attachedBaseUrl ?? "none"}`,
      `project: ${health?.trackerProjectUrl ?? "none"}`,
      `polling: ${health?.pollingChecking ? "checking" : "idle"} | next poll: ${health?.nextPollInMs ?? 0}ms`,
      `workers: running: ${health?.runningCount ?? 0} | retry: ${health?.retryCount ?? 0} | blocked: ${health?.blockedCount ?? 0} | completed: ${health?.completedCount ?? 0}`,
      `owned process: ${state.ownedProcess ? `pid ${state.ownedProcess.pid}` : "none"}`,
      `updated: ${health?.updatedAt ?? "never"}`,
      "",
      this.refreshing ? "refreshing..." : "keys: r refresh | q/esc close",
    ];

    return lines.map((line) => truncateToWidth(line, width));
  }

  invalidate(): void {}

  private async refresh(): Promise<void> {
    this.refreshing = true;
    this.options.requestRender();
    try {
      await this.options.refresh();
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.refreshing = false;
      this.options.requestRender();
    }
  }
}

export async function openDashboard(ctx: ExtensionContext, runtime: SymphonyRuntime): Promise<void> {
  if (!runtime.client) {
    ctx.ui.notify("No Symphony server is attached. Use /symphony:start or /symphony:attach first.", "warning");
    return;
  }

  await runtime.refreshState();

  await ctx.ui.custom<void>((tui, _theme, _keybindings, done) => {
    const component = new SymphonyDashboardComponent({
      state: runtime.state,
      refresh: async () => {
        await runtime.refreshState();
      },
      close: () => done(undefined),
      requestRender: () => tui.requestRender(),
      notify: (message, level) => ctx.ui.notify(message, level),
    });
    return component;
  });
}
```

- [ ] **Step 4: Run dashboard tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/dashboard.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit dashboard**

```bash
git add apps/symphony/pi-extension/src/dashboard.ts apps/symphony/pi-extension/src/dashboard.test.ts
git commit -m "feat(pi-symphony): add health dashboard"
```

---

### Task 8: Slash commands

**Files:**
- Create: `apps/symphony/pi-extension/src/commands.ts`
- Create: `apps/symphony/pi-extension/src/index.ts`

- [ ] **Step 1: Implement commands**

Create `apps/symphony/pi-extension/src/commands.ts`:

```ts
import type { ExtensionAPI, ExtensionCommandContext } from "@earendil-works/pi-coding-agent";
import { openDashboard } from "./dashboard.ts";
import { formatError, SymphonyExtensionError } from "./errors.ts";
import { parseAttachArgs, parseDoctorArgs, parseInitArgs, parseStartArgs } from "./command-args.ts";
import type { SymphonyRuntime } from "./runtime.ts";

export function registerSymphonyCommands(pi: ExtensionAPI, runtime: SymphonyRuntime): void {
  pi.registerCommand("symphony:help", {
    description: "Show Symphony extension commands and current status",
    handler: async (_args, ctx) => {
      ctx.ui.notify(helpText(runtime), "info");
    },
  });

  pi.registerCommand("symphony:init", {
    description: "Run symphony init in the current Pi working directory",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => {
      const parsed = parseInitArgs(args);
      const binary = await runtime.resolveBinary(ctx);
      const result = await pi.exec(binary, parsed.force ? ["init", "--force"] : ["init"], { cwd: ctx.cwd });
      if (result.code !== 0) throw new SymphonyExtensionError("command_failed", "symphony init failed", { cwd: ctx.cwd, code: result.code, stderr: result.stderr });
      runtime.persist(pi);
      ctx.ui.notify(result.stdout.trim() || "symphony init completed", "info");
    }),
  });

  pi.registerCommand("symphony:doctor", {
    description: "Run symphony doctor in the current Pi working directory",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => {
      const parsed = parseDoctorArgs(args);
      const binary = await runtime.resolveBinary(ctx);
      const commandArgs = parsed.workflow ? ["doctor", parsed.workflow] : ["doctor"];
      const result = await pi.exec(binary, commandArgs, { cwd: ctx.cwd });
      if (result.code !== 0) throw new SymphonyExtensionError("command_failed", "symphony doctor failed", { cwd: ctx.cwd, code: result.code, stderr: result.stderr });
      runtime.persist(pi);
      ctx.ui.notify(result.stdout.trim() || "symphony doctor completed", "info");
    }),
  });

  pi.registerCommand("symphony:start", {
    description: "Start Symphony headlessly, attach to the HTTP API, and open the dashboard",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => {
      const parsed = parseStartArgs(args);
      const binary = await runtime.resolveBinary(ctx);
      const started = await runtime.processManager.start({ binary, cwd: ctx.cwd, workflow: parsed.workflow });
      await runtime.attach(started.baseUrl);
      runtime.persist(pi);
      ctx.ui.notify(`Symphony started at ${started.baseUrl}`, "info");
      await openDashboard(ctx, runtime);
    }),
  });

  pi.registerCommand("symphony:attach", {
    description: "Attach to an existing Symphony HTTP server",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => {
      const parsed = parseAttachArgs(args);
      await runtime.attach(parsed.url);
      runtime.persist(pi);
      ctx.ui.notify(`Attached to Symphony at ${runtime.state.attachedBaseUrl}`, "info");
    }),
  });

  pi.registerCommand("symphony:dashboard", {
    description: "Open the Symphony health dashboard",
    handler: async (_args, ctx) => runCommandHandler(ctx, async () => {
      await openDashboard(ctx, runtime);
    }),
  });

  pi.registerCommand("symphony:status", {
    description: "Show Symphony attachment and process status",
    handler: async (_args, ctx) => runCommandHandler(ctx, async () => {
      if (runtime.client) await runtime.refreshState();
      runtime.persist(pi);
      ctx.ui.notify(runtime.statusText(), "info");
    }),
  });

  pi.registerCommand("symphony:stop", {
    description: "Stop a Symphony process started by this extension",
    handler: async (_args, ctx) => runCommandHandler(ctx, async () => {
      await runtime.processManager.stopOwned();
      runtime.persist(pi);
      ctx.ui.notify("Stopped owned Symphony process", "info");
    }),
  });
}

async function runCommandHandler(ctx: ExtensionCommandContext, fn: () => Promise<void>): Promise<void> {
  try {
    await fn();
  } catch (error) {
    ctx.ui.notify(formatError(error), "error");
  }
}

function helpText(runtime: SymphonyRuntime): string {
  return [
    "Symphony Pi extension",
    runtime.statusText(),
    "",
    "Commands:",
    "/symphony:init [--force]",
    "/symphony:doctor [workflow]",
    "/symphony:start [workflow]",
    "/symphony:attach <url>",
    "/symphony:dashboard",
    "/symphony:status",
    "/symphony:stop",
  ].join("\n");
}
```

- [ ] **Step 2: Implement extension entrypoint**

Create `apps/symphony/pi-extension/src/index.ts`:

```ts
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerSymphonyCommands } from "./commands.ts";
import { SymphonyRuntime } from "./runtime.ts";

export default function symphonyExtension(pi: ExtensionAPI): void {
  const runtime = new SymphonyRuntime();

  pi.on("session_start", async (_event, ctx) => {
    runtime.restore(ctx);
    ctx.ui.setStatus("symphony", runtime.state.attachedBaseUrl ? `symphony ${runtime.state.attachedBaseUrl}` : "symphony detached");
  });

  pi.on("session_shutdown", async () => {
    await runtime.processManager.shutdown();
  });

  registerSymphonyCommands(pi, runtime);
}
```

- [ ] **Step 3: Typecheck commands**

Run:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS.

- [ ] **Step 4: Commit commands**

```bash
git add apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/index.ts
git commit -m "feat(pi-symphony): register symphony slash commands"
```

---

### Task 9: LLM-callable tools

**Files:**
- Create: `apps/symphony/pi-extension/src/tools.ts`
- Modify: `apps/symphony/pi-extension/src/index.ts`

- [ ] **Step 1: Implement tool registration**

Create `apps/symphony/pi-extension/src/tools.ts`:

```ts
import { Type } from "@earendil-works/pi-ai";
import { defineTool, type ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { formatError, SymphonyExtensionError } from "./errors.ts";
import type { SymphonyRuntime } from "./runtime.ts";

export function registerSymphonyTools(pi: ExtensionAPI, runtime: SymphonyRuntime): void {
  pi.registerTool(defineTool({
    name: "symphony_help",
    label: "Symphony Help",
    description: "Show Symphony Pi extension commands and tool capabilities.",
    parameters: Type.Object({}),
    async execute() {
      return toolOk("Symphony tools: symphony_init, symphony_doctor, symphony_start, symphony_attach, symphony_status, symphony_stop, symphony_help", {
        attachedBaseUrl: runtime.state.attachedBaseUrl,
        ownedProcess: runtime.state.ownedProcess,
      });
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_init",
    label: "Symphony Init",
    description: "Run symphony init in Pi's current working directory.",
    parameters: Type.Object({ force: Type.Optional(Type.Boolean()) }),
    async execute(_id, params, signal, _update, ctx) {
      try {
        const binary = await runtime.resolveBinary(ctx);
        const result = await pi.exec(binary, params.force ? ["init", "--force"] : ["init"], { cwd: ctx.cwd, signal });
        if (result.code !== 0) throw new SymphonyExtensionError("command_failed", "symphony init failed", { cwd: ctx.cwd, code: result.code, stderr: result.stderr });
        runtime.persist(pi);
        return toolOk(result.stdout.trim() || "symphony init completed", { code: result.code, cwd: ctx.cwd });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_doctor",
    label: "Symphony Doctor",
    description: "Run symphony doctor with an optional workflow path.",
    parameters: Type.Object({ workflow: Type.Optional(Type.String()) }),
    async execute(_id, params, signal, _update, ctx) {
      try {
        const binary = await runtime.resolveBinary(ctx);
        const args = params.workflow ? ["doctor", params.workflow] : ["doctor"];
        const result = await pi.exec(binary, args, { cwd: ctx.cwd, signal });
        if (result.code !== 0) throw new SymphonyExtensionError("command_failed", "symphony doctor failed", { cwd: ctx.cwd, code: result.code, stderr: result.stderr });
        runtime.persist(pi);
        return toolOk(result.stdout.trim() || "symphony doctor completed", { code: result.code, cwd: ctx.cwd });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_start",
    label: "Symphony Start",
    description: "Start Symphony headlessly from Pi's current working directory and attach to its HTTP API.",
    parameters: Type.Object({ workflow: Type.Optional(Type.String()) }),
    async execute(_id, params, _signal, _update, ctx) {
      try {
        const binary = await runtime.resolveBinary(ctx);
        const started = await runtime.processManager.start({ binary, cwd: ctx.cwd, workflow: params.workflow });
        await runtime.attach(started.baseUrl);
        runtime.persist(pi);
        return toolOk(`Symphony started at ${started.baseUrl}`, { ...started, state: runtime.state.lastKnownState });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_attach",
    label: "Symphony Attach",
    description: "Attach to an existing Symphony HTTP server after verifying GET /api/v1/state.",
    parameters: Type.Object({ url: Type.String() }),
    async execute(_id, params) {
      try {
        await runtime.attach(params.url);
        runtime.persist(pi);
        return toolOk(`Attached to Symphony at ${runtime.state.attachedBaseUrl}`, { state: runtime.state.lastKnownState });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_status",
    label: "Symphony Status",
    description: "Return current Symphony attachment, process, and health summary.",
    parameters: Type.Object({}),
    async execute() {
      try {
        if (runtime.client) await runtime.refreshState();
        runtime.persist(pi);
        return toolOk(runtime.statusText(), { state: runtime.state });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_stop",
    label: "Symphony Stop",
    description: "Stop only a Symphony process started by this Pi extension.",
    parameters: Type.Object({}),
    async execute() {
      try {
        await runtime.processManager.stopOwned();
        runtime.persist(pi);
        return toolOk("Stopped owned Symphony process", { ownedProcess: runtime.state.ownedProcess });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));
}

function toolOk(text: string, details: Record<string, unknown>) {
  return {
    content: [{ type: "text" as const, text }],
    details,
  };
}
```

- [ ] **Step 2: Register tools in entrypoint**

Modify `apps/symphony/pi-extension/src/index.ts`:

```ts
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { registerSymphonyCommands } from "./commands.ts";
import { SymphonyRuntime } from "./runtime.ts";
import { registerSymphonyTools } from "./tools.ts";

export default function symphonyExtension(pi: ExtensionAPI): void {
  const runtime = new SymphonyRuntime();

  pi.on("session_start", async (_event, ctx) => {
    runtime.restore(ctx);
    ctx.ui.setStatus("symphony", runtime.state.attachedBaseUrl ? `symphony ${runtime.state.attachedBaseUrl}` : "symphony detached");
  });

  pi.on("session_shutdown", async () => {
    await runtime.processManager.shutdown();
  });

  registerSymphonyCommands(pi, runtime);
  registerSymphonyTools(pi, runtime);
}
```

- [ ] **Step 3: Typecheck tools**

Run:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS.

- [ ] **Step 4: Commit tools**

```bash
git add apps/symphony/pi-extension/src/tools.ts apps/symphony/pi-extension/src/index.ts
git commit -m "feat(pi-symphony): register symphony tools"
```

---

### Task 10: Final verification and Slice 1 acceptance

**Files:**
- Modify if needed: `apps/symphony/pi-extension/README.md`

- [ ] **Step 1: Run all package tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test
```

Expected: PASS for package smoke, parser, resolver, HTTP client, process manager, and dashboard tests.

- [ ] **Step 2: Run package typecheck**

Run:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS.

- [ ] **Step 3: Run affected validation**

Run:

```bash
pnpm run validate:affected
```

Expected: PASS for affected workspace packages. If Turborepo does not detect the new package on the first run, run the package-local test and typecheck commands from Steps 1 and 2 as the verification source for this PR.

- [ ] **Step 4: Manual smoke test with Pi**

Run from repo root:

```bash
pi -e ./apps/symphony/pi-extension
```

In Pi, run:

```text
/symphony:help
/symphony:status
```

Expected:

```text
/symphony:help shows command list and detached status.
/symphony:status shows attached: no, owned pid: none, state: unknown.
```

If a local Symphony binary exists, run:

```text
/symphony:doctor .symphony/WORKFLOW.md
```

Expected: command output from `symphony doctor` appears in a Pi notification. If local config is incomplete, the notification must show exit code, stderr, and cwd.

- [ ] **Step 5: Update README with verified command output**

Append this section to `apps/symphony/pi-extension/README.md` after manual verification:

```md
## Slice 1 verification

Verified commands:

```text
/symphony:help
/symphony:status
```

Package checks:

```sh
pnpm --dir apps/symphony/pi-extension test
pnpm --dir apps/symphony/pi-extension typecheck
```
```

- [ ] **Step 6: Commit verification notes**

```bash
git add apps/symphony/pi-extension/README.md
git commit -m "docs(pi-symphony): document slice 1 verification"
```

---

## Self-review

Spec coverage:

- Binary resolution and validation: Task 3.
- `symphony init`: Task 8 command, Task 9 tool.
- `symphony doctor`: Task 8 command, Task 9 tool.
- Headless start with `--no-tui`: Task 5 process manager, Task 8 command, Task 9 tool.
- Attach to existing server after `GET /api/v1/state`: Task 4 client, Task 8 command, Task 9 tool.
- Health dashboard with connection, project link, polling, worker counts, process ownership: Task 7.
- Stop only owned process: Task 5 process manager, Task 8 command, Task 9 tool.
- Help, status commands and tools: Task 8 and Task 9.
- Pi package distribution metadata: Task 1.

Placeholder scan:

- No `TBD`, `TODO`, `implement later`, or unspecified validation steps remain.
- Every task lists exact paths, commands, expected outcomes, and commit commands.

Type consistency:

- Shared state type is `ExtensionState` in `state.ts` and all dependent files.
- HTTP client type is `SymphonyHttpClient` in `http-client.ts`, `runtime.ts`, and process manager readiness checks.
- Dashboard component type is `SymphonyDashboardComponent` in implementation and tests.
- Runtime type is `SymphonyRuntime` in commands, tools, dashboard, and entrypoint.
