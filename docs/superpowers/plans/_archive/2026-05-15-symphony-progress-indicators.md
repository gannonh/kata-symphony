---
type: Plan
title: Symphony Progress Indicators
description: "Archived implementation plan: Symphony Progress Indicators."
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Symphony Progress Indicators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add visible progress feedback for Symphony Pi extension commands and tools that can take a few seconds to complete.

**Architecture:** Add one focused progress helper module for command UI behavior, then wire commands through lightweight inline progress or a blocking loader based on expected duration. Tools will emit partial `onUpdate` messages before executing slow work, while final results and error formatting stay unchanged.

**Tech Stack:** TypeScript, Pi extension APIs (`ctx.ui.setWorkingIndicator`, `ctx.ui.setWorkingMessage`, `ctx.ui.setStatus`, `BorderedLoader`), Vitest, pnpm.

---

## File structure

- Create `apps/symphony/pi-extension/src/progress.ts`
  - Owns reusable command progress helpers and dot-matrix spinner frames.
  - Exports `withSymphonyProgress`, `withSymphonyLoader`, and `SYMPHONY_PROGRESS_FRAMES`.
- Create `apps/symphony/pi-extension/src/progress.test.ts`
  - Unit tests for helper success, failure, cleanup, and loader abort signal behavior.
- Modify `apps/symphony/pi-extension/src/commands.ts`
  - Wrap command handlers with progress helpers.
  - Keep existing command semantics and notifications.
  - Pass abort signals into `pi.exec`, process start, attach, refresh, and stop where available.
- Modify `apps/symphony/pi-extension/src/commands.test.ts`
  - Extend fake command UI with working indicator/message/custom UI methods.
  - Assert progress is shown and restored for representative short and long commands.
- Modify `apps/symphony/pi-extension/src/tools.ts`
  - Use the existing tool `onUpdate` callback before slow operations.
- Modify `apps/symphony/pi-extension/src/tools.test.ts`
  - Capture tool update calls and assert partial progress messages.

---

### Task 1: Add command progress helpers

**Files:**
- Create: `apps/symphony/pi-extension/src/progress.ts`
- Test: `apps/symphony/pi-extension/src/progress.test.ts`

- [ ] **Step 1: Write the failing progress helper tests**

Create `apps/symphony/pi-extension/src/progress.test.ts`:

```ts
import type { ExtensionCommandContext } from "@earendil-works/pi-coding-agent";
import { describe, expect, it, vi } from "vitest";
import { SYMPHONY_PROGRESS_FRAMES, withSymphonyLoader, withSymphonyProgress } from "./progress.ts";

function progressContext() {
  const setWorkingIndicator = vi.fn();
  const setWorkingMessage = vi.fn();
  const setStatus = vi.fn();
  const ctx = {
    ui: { setWorkingIndicator, setWorkingMessage, setStatus },
    cwd: "/repo",
    hasUI: true,
  } as unknown as ExtensionCommandContext;

  return { ctx, setWorkingIndicator, setWorkingMessage, setStatus };
}

function loaderContext() {
  const setStatus = vi.fn();
  const requestRender = vi.fn();
  const custom = vi.fn(async (factory: (tui: unknown, theme: unknown, keybindings: unknown, done: (value: unknown) => void) => unknown) => {
    let resolved = false;
    const done = () => {
      resolved = true;
    };
    const component = factory({ requestRender }, { fg: (_name: string, value: string) => value }, undefined, done) as { signal: AbortSignal; dispose?: () => void };
    await Promise.resolve();
    if (!resolved) done(undefined);
    component.dispose?.();
    return undefined;
  });
  const ctx = {
    ui: { setStatus, custom },
    cwd: "/repo",
    hasUI: true,
  } as unknown as ExtensionCommandContext;

  return { ctx, setStatus, custom };
}

describe("Symphony progress helpers", () => {
  it("sets and restores the inline working indicator, message, and status after success", async () => {
    const { ctx, setWorkingIndicator, setWorkingMessage, setStatus } = progressContext();
    const restoreStatus = vi.fn();

    const result = await withSymphonyProgress(ctx, { message: "Refreshing Symphony...", restoreStatus }, async () => "ok");

    expect(result).toBe("ok");
    expect(setWorkingIndicator).toHaveBeenNthCalledWith(1, { frames: SYMPHONY_PROGRESS_FRAMES, intervalMs: 120 });
    expect(setWorkingMessage).toHaveBeenNthCalledWith(1, "Refreshing Symphony...");
    expect(setStatus).toHaveBeenCalledWith("symphony", "Refreshing Symphony...");
    expect(setWorkingIndicator).toHaveBeenLastCalledWith();
    expect(setWorkingMessage).toHaveBeenLastCalledWith();
    expect(restoreStatus).toHaveBeenCalledWith(ctx);
  });

  it("restores inline progress after failure", async () => {
    const { ctx, setWorkingIndicator, setWorkingMessage } = progressContext();
    const restoreStatus = vi.fn();

    await expect(withSymphonyProgress(ctx, { message: "Attaching to Symphony...", restoreStatus }, async () => {
      throw new Error("attach failed");
    })).rejects.toThrow("attach failed");

    expect(setWorkingIndicator).toHaveBeenLastCalledWith();
    expect(setWorkingMessage).toHaveBeenLastCalledWith();
    expect(restoreStatus).toHaveBeenCalledWith(ctx);
  });

  it("passes a loader abort signal to long-running operations and restores status", async () => {
    const { ctx, setStatus, custom } = loaderContext();
    const restoreStatus = vi.fn();
    const operation = vi.fn(async (_signal: AbortSignal) => "started");

    const result = await withSymphonyLoader(ctx, { message: "Starting Symphony...", restoreStatus }, operation);

    expect(result).toBe("started");
    expect(custom).toHaveBeenCalledOnce();
    expect(operation).toHaveBeenCalledWith(expect.any(AbortSignal));
    expect(setStatus).toHaveBeenCalledWith("symphony", "Starting Symphony...");
    expect(restoreStatus).toHaveBeenCalledWith(ctx);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/progress.test.ts
```

Expected: FAIL because `./progress.ts` does not exist.

- [ ] **Step 3: Implement the minimal progress helper module**

Create `apps/symphony/pi-extension/src/progress.ts`:

```ts
import { BorderedLoader, type ExtensionCommandContext } from "@earendil-works/pi-coding-agent";

export const SYMPHONY_PROGRESS_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

export interface SymphonyProgressOptions {
  message: string;
  restoreStatus: (ctx: ExtensionCommandContext) => void;
}

export async function withSymphonyProgress<T>(
  ctx: ExtensionCommandContext,
  options: SymphonyProgressOptions,
  fn: () => Promise<T>,
): Promise<T> {
  ctx.ui.setWorkingIndicator({ frames: SYMPHONY_PROGRESS_FRAMES, intervalMs: 120 });
  ctx.ui.setWorkingMessage(options.message);
  ctx.ui.setStatus("symphony", options.message);

  try {
    return await fn();
  } finally {
    ctx.ui.setWorkingIndicator();
    ctx.ui.setWorkingMessage();
    options.restoreStatus(ctx);
  }
}

export async function withSymphonyLoader<T>(
  ctx: ExtensionCommandContext,
  options: SymphonyProgressOptions,
  fn: (signal: AbortSignal) => Promise<T>,
): Promise<T> {
  ctx.ui.setStatus("symphony", options.message);
  try {
    return await ctx.ui.custom<T | undefined>(async (tui, theme, _keybindings, done) => {
      const loader = new BorderedLoader(tui, theme, options.message);
      loader.onAbort = () => done(undefined);

      void fn(loader.signal)
        .then((result) => done(result))
        .catch((error) => {
          throw error;
        });

      return loader;
    }).then((result) => result as T);
  } finally {
    options.restoreStatus(ctx);
  }
}
```

- [ ] **Step 4: Run the test and fix TypeScript/runtime issues**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/progress.test.ts
```

Expected: PASS.

If the `withSymphonyLoader` test hangs or unhandled rejections appear, replace the loader implementation with a Promise race that stores the async operation error and resolves through `done`, then rethrows after `ctx.ui.custom` returns. Keep the public signatures unchanged.

- [ ] **Step 5: Commit Task 1**

```bash
git add apps/symphony/pi-extension/src/progress.ts apps/symphony/pi-extension/src/progress.test.ts
git commit -m "feat(symphony): add command progress helpers"
```

---

### Task 2: Wire command progress into Symphony commands

**Files:**
- Modify: `apps/symphony/pi-extension/src/commands.ts`
- Modify: `apps/symphony/pi-extension/src/commands.test.ts`

- [ ] **Step 1: Write failing command progress tests**

Modify `commandContext()` in `apps/symphony/pi-extension/src/commands.test.ts` so the fake UI records progress methods:

```ts
function commandContext() {
  const notify = vi.fn();
  const setStatus = vi.fn();
  const setWorkingIndicator = vi.fn();
  const setWorkingMessage = vi.fn();
  const custom = vi.fn(async (factory: (tui: unknown, theme: unknown, keybindings: unknown, done: (value: unknown) => void) => unknown) => {
    let value: unknown;
    const done = (next: unknown) => {
      value = next;
    };
    const component = factory({ requestRender: vi.fn() }, { fg: (_name: string, text: string) => text }, undefined, done) as { dispose?: () => void };
    await Promise.resolve();
    component.dispose?.();
    return value;
  });
  const ctx = {
    ui: { notify, setStatus, setWorkingIndicator, setWorkingMessage, custom },
    cwd: "/repo",
    hasUI: false,
  } as unknown as ExtensionCommandContext;

  return { ctx, notify, setStatus, setWorkingIndicator, setWorkingMessage, custom };
}
```

Add these tests inside `describe("symphony commands", () => { ... })`:

```ts
  it("shows inline progress while attaching", async () => {
    const runtime = new SymphonyRuntime();
    runtime.attach = vi.fn(async (baseUrl: string) => {
      runtime.state.attachedBaseUrl = baseUrl;
      runtime.state.lastKnownState = lastKnownState(baseUrl);
      return {};
    }) as unknown as SymphonyRuntime["attach"];

    const { commands } = registerCommands(runtime);
    const { ctx, setStatus, setWorkingIndicator, setWorkingMessage } = commandContext();
    const attach = commands.get("symphony:attach");
    if (!attach) throw new Error("expected attach command");

    await attach.handler("http://localhost:8080", ctx);

    expect(setWorkingMessage).toHaveBeenNthCalledWith(1, "Attaching to Symphony...");
    expect(setStatus).toHaveBeenNthCalledWith(1, "symphony", "Attaching to Symphony...");
    expect(setWorkingIndicator).toHaveBeenLastCalledWith();
    expect(setWorkingMessage).toHaveBeenLastCalledWith();
    expect(setStatus).toHaveBeenLastCalledWith("symphony", "symphony http://localhost:8080");
  });

  it("shows inline progress while refreshing", async () => {
    const runtime = new SymphonyRuntime();
    runtime.state.attachedBaseUrl = "http://127.0.0.1:8080";
    runtime.requestRefresh = vi.fn(async () => {
      runtime.state.lastKnownState = lastKnownState("http://127.0.0.1:8080");
      return {} as Awaited<ReturnType<SymphonyRuntime["requestRefresh"]>>;
    }) as SymphonyRuntime["requestRefresh"];

    const { commands } = registerCommands(runtime);
    const { ctx, setStatus, setWorkingMessage } = commandContext();
    const refresh = commands.get("symphony:refresh");
    if (!refresh) throw new Error("expected refresh command");

    await refresh.handler("", ctx);

    expect(setWorkingMessage).toHaveBeenNthCalledWith(1, "Refreshing Symphony...");
    expect(setStatus).toHaveBeenNthCalledWith(1, "symphony", "Refreshing Symphony...");
    expect(setStatus).toHaveBeenLastCalledWith("symphony", "symphony http://127.0.0.1:8080");
  });

  it("uses a blocking loader for doctor", async () => {
    const runtime = new SymphonyRuntime();
    runtime.resolveBinary = vi.fn(async () => "symphony") as SymphonyRuntime["resolveBinary"];
    const exec = vi.fn(async (_binary: string, _args: string[], _options: unknown) => ({ code: 0, stdout: "doctor ok", stderr: "", killed: false }));
    const commands = new Map<string, CommandOptions>();
    const pi = {
      registerCommand: (name: string, options: CommandOptions) => commands.set(name, options),
      appendEntry: vi.fn(),
      exec,
    } as unknown as ExtensionAPI;
    registerSymphonyCommands(pi, runtime);

    const { ctx, custom, setStatus } = commandContext();
    const doctor = commands.get("symphony:doctor");
    if (!doctor) throw new Error("expected doctor command");

    await doctor.handler("", ctx);

    expect(custom).toHaveBeenCalledOnce();
    expect(setStatus).toHaveBeenNthCalledWith(1, "symphony", "Running Symphony doctor...");
    expect(setStatus).toHaveBeenLastCalledWith("symphony", "symphony detached");
  });
```

- [ ] **Step 2: Run command tests to verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/commands.test.ts
```

Expected: FAIL because command handlers do not call progress helpers yet.

- [ ] **Step 3: Import progress helpers and wrap commands**

Modify imports in `apps/symphony/pi-extension/src/commands.ts`:

```ts
import { withSymphonyLoader, withSymphonyProgress } from "./progress.ts";
```

Wrap these handlers:

```ts
  pi.registerCommand("symphony:init", {
    description: "Run symphony init in the current Pi working directory",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => withSymphonyLoader(ctx, { message: "Initializing Symphony...", restoreStatus: (ctx) => setSymphonyStatus(ctx, runtime) }, async (signal) => {
      const parsed = parseInitArgs(args);
      const binary = await runtime.resolveBinary(ctx);
      const result = await pi.exec(binary, parsed.force ? ["init", "--force"] : ["init"], { cwd: ctx.cwd, signal });
      if (result.code !== 0) throw new SymphonyExtensionError("command_failed", "symphony init failed", { cwd: ctx.cwd, code: result.code, stderr: result.stderr });
      runtime.persist(pi);
      ctx.ui.notify(result.stdout.trim() || "symphony init completed", "info");
    })),
  });

  pi.registerCommand("symphony:doctor", {
    description: "Run symphony doctor in the current Pi working directory",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => withSymphonyLoader(ctx, { message: "Running Symphony doctor...", restoreStatus: (ctx) => setSymphonyStatus(ctx, runtime) }, async (signal) => {
      const parsed = parseDoctorArgs(args);
      const binary = await runtime.resolveBinary(ctx);
      const commandArgs = parsed.workflow ? ["doctor", parsed.workflow] : ["doctor"];
      const result = await pi.exec(binary, commandArgs, { cwd: ctx.cwd, signal });
      if (result.code !== 0) throw new SymphonyExtensionError("command_failed", "symphony doctor failed", { cwd: ctx.cwd, code: result.code, stderr: result.stderr });
      runtime.persist(pi);
      ctx.ui.notify(result.stdout.trim() || "symphony doctor completed", "info");
    })),
  });

  pi.registerCommand("symphony:start", {
    description: "Start Symphony headlessly, attach to the HTTP API, and open the dashboard",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => withSymphonyLoader(ctx, { message: "Starting Symphony...", restoreStatus: (ctx) => setSymphonyStatus(ctx, runtime) }, async (signal) => {
      const parsed = parseStartArgs(args);
      const binary = await runtime.resolveBinary(ctx);
      const started = await runtime.processManager.start({ binary, cwd: ctx.cwd, workflow: parsed.workflow, signal });
      await runtime.attach(started.baseUrl, signal);
      runtime.persist(pi);
      setSymphonyStatus(ctx, runtime);
      ctx.ui.notify(`Symphony started at ${started.baseUrl}`, "info");
      await openDashboard(ctx, runtime);
    })),
  });
```

Wrap lightweight commands:

```ts
  pi.registerCommand("symphony:attach", {
    description: "Attach to an existing Symphony HTTP server",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => withSymphonyProgress(ctx, { message: "Attaching to Symphony...", restoreStatus: (ctx) => setSymphonyStatus(ctx, runtime) }, async () => {
      const parsed = parseAttachArgs(args);
      assertLoopbackAttachUrl(parsed.url);
      await runtime.attach(parsed.url);
      runtime.persist(pi);
      setSymphonyStatus(ctx, runtime);
      ctx.ui.notify(`Attached to Symphony at ${runtime.state.attachedBaseUrl}`, "info");
    })),
  });

  pi.registerCommand("symphony:refresh", {
    description: "Request an immediate Symphony poll refresh",
    handler: async (_args, ctx) => runCommandHandler(ctx, async () => withSymphonyProgress(ctx, { message: "Refreshing Symphony...", restoreStatus: (ctx) => setSymphonyStatus(ctx, runtime) }, async () => {
      await runtime.requestRefresh();
      runtime.persist(pi);
      ctx.ui.notify(`Symphony refresh requested; ${runtimeCountsText(runtime)}`, "info");
    })),
  });

  pi.registerCommand("symphony:stop", {
    description: "Stop a Symphony process started by this extension",
    handler: async (_args, ctx) => runCommandHandler(ctx, async () => withSymphonyProgress(ctx, { message: "Stopping Symphony...", restoreStatus: (ctx) => setSymphonyStatus(ctx, runtime) }, async () => {
      const ownedBaseUrl = runtime.state.ownedProcess?.baseUrl;
      await runtime.processManager.stopOwned();
      runtime.clearAttachmentIfBaseUrl(ownedBaseUrl);
      runtime.persist(pi);
      setSymphonyStatus(ctx, runtime);
      ctx.ui.notify("Stopped owned Symphony process", "info");
    })),
  });
```

Leave `/symphony:help`, `/symphony:dashboard`, `/symphony:status`, and `/symphony:steer` unchanged for command progress unless product feedback asks for steer command progress too. The approved spec only lists short-operation command progress for refresh, attach, and stop.

- [ ] **Step 4: Run command tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/commands.test.ts src/progress.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

```bash
git add apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/commands.test.ts apps/symphony/pi-extension/src/progress.ts apps/symphony/pi-extension/src/progress.test.ts
git commit -m "feat(symphony): show progress for extension commands"
```

---

### Task 3: Emit partial progress updates from Symphony tools

**Files:**
- Modify: `apps/symphony/pi-extension/src/tools.ts`
- Modify: `apps/symphony/pi-extension/src/tools.test.ts`

- [ ] **Step 1: Update the registered tool type in tests to accept update callbacks**

Modify `RegisteredTool` in `apps/symphony/pi-extension/src/tools.test.ts`:

```ts
type ToolUpdate = (update: { content: Array<{ type: "text"; text: string }>; details?: Record<string, unknown> }) => void;

type RegisteredTool = {
  name: string;
  executionMode?: string;
  execute: (id: string, params: Record<string, unknown>, signal: AbortSignal, update: ToolUpdate | undefined, ctx: ExtensionContext) => Promise<unknown>;
};
```

- [ ] **Step 2: Write failing tool progress tests**

Add these tests inside `describe("symphony tools", () => { ... })`:

```ts
  it("emits progress while refreshing from the tool", async () => {
    const runtime = new SymphonyRuntime();
    runtime.requestRefresh = vi.fn(async () => {
      runtime.state.lastKnownState = lastKnownState("http://127.0.0.1:8080");
      return {} as Awaited<ReturnType<SymphonyRuntime["requestRefresh"]>>;
    }) as SymphonyRuntime["requestRefresh"];
    const { tools } = registerTools(runtime);
    const refresh = tools.get("symphony_refresh");
    if (!refresh) throw new Error("expected refresh tool");
    const update = vi.fn();

    await refresh.execute("1", {}, new AbortController().signal, update, toolContext().ctx);

    expect(update).toHaveBeenCalledWith({ content: [{ type: "text", text: "Refreshing Symphony..." }], details: { status: "working" } });
  });

  it("emits progress while steering from the tool", async () => {
    const runtime = new SymphonyRuntime();
    runtime.steerWorker = vi.fn(async () => ({ ok: true, issueId: "issue-123", issueIdentifier: "SIM-123", delivered: true, instructionPreview: "Use auth" })) as SymphonyRuntime["steerWorker"];
    const { tools } = registerTools(runtime);
    const steer = tools.get("symphony_steer");
    if (!steer) throw new Error("expected steer tool");
    const update = vi.fn();

    await steer.execute("1", { issueIdentifier: "SIM-123", instruction: "Use auth" }, new AbortController().signal, update, toolContext().ctx);

    expect(update).toHaveBeenCalledWith({ content: [{ type: "text", text: "Sending steer instruction..." }], details: { status: "working" } });
  });
```

- [ ] **Step 3: Run tool tests to verify failure**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/tools.test.ts
```

Expected: FAIL because tools do not emit partial progress updates yet.

- [ ] **Step 4: Add a shared tool progress helper**

Modify `apps/symphony/pi-extension/src/tools.ts` near `SYMPHONY_TOOL_EXECUTION_MODE`:

```ts
type ToolUpdate = (update: { content: Array<{ type: "text"; text: string }>; details?: Record<string, unknown> }) => void;

function updateProgress(onUpdate: ToolUpdate | undefined, text: string): void {
  onUpdate?.({ content: [{ type: "text", text }], details: { status: "working" } });
}
```

- [ ] **Step 5: Call `updateProgress` at the start of slow tool executions**

In `apps/symphony/pi-extension/src/tools.ts`, rename each `_update` parameter to `onUpdate` for these tools and add the call before slow work:

```ts
// symphony_init execute
updateProgress(onUpdate, "Initializing Symphony...");

// symphony_doctor execute
updateProgress(onUpdate, "Running Symphony doctor...");

// symphony_start execute
updateProgress(onUpdate, "Starting Symphony...");

// symphony_attach execute
updateProgress(onUpdate, "Attaching to Symphony...");

// symphony_refresh execute
updateProgress(onUpdate, "Refreshing Symphony...");

// symphony_steer execute
updateProgress(onUpdate, "Sending steer instruction...");

// symphony_stop execute
updateProgress(onUpdate, "Stopping Symphony...");
```

For `symphony_stop`, also rename `_signal` to `signal` if you decide to pass it into a future stop API. Do not add unused variables that fail lint.

- [ ] **Step 6: Run focused tool tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/tools.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit Task 3**

```bash
git add apps/symphony/pi-extension/src/tools.ts apps/symphony/pi-extension/src/tools.test.ts
git commit -m "feat(symphony): stream tool progress updates"
```

---

### Task 4: Full validation and documentation update

**Files:**
- Modify: `apps/symphony/pi-extension/README.md`
- Modify if needed: `docs/superpowers/specs/2026-05-15-symphony-progress-indicators-design.md`

- [ ] **Step 1: Update README with progress behavior**

Modify `apps/symphony/pi-extension/README.md` by adding this section after `Dashboard keys through Slice 2`:

```md
## Progress feedback

Commands that can take a few seconds show Pi-native progress feedback while they run:

- `/symphony:refresh`, `/symphony:attach`, and `/symphony:stop` use inline working text plus the Symphony footer status.
- `/symphony:start`, `/symphony:init`, and `/symphony:doctor` use a blocking loader panel.
- Symphony tools emit partial progress updates before returning their final result.
```

- [ ] **Step 2: Run full package checks**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test
pnpm --dir apps/symphony/pi-extension typecheck
pnpm --dir apps/symphony/pi-extension lint
```

Expected:

```text
Test Files 14 passed
```

The exact test count may be higher than 87 after adding `progress.test.ts`. Typecheck and lint should exit 0.

- [ ] **Step 3: Inspect git diff for accidental changes**

Run:

```bash
git diff -- apps/symphony/pi-extension/src/progress.ts apps/symphony/pi-extension/src/progress.test.ts apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/commands.test.ts apps/symphony/pi-extension/src/tools.ts apps/symphony/pi-extension/src/tools.test.ts apps/symphony/pi-extension/README.md
```

Expected: diff only contains progress helper, command progress wiring, tool progress updates, tests, and README text.

- [ ] **Step 4: Commit Task 4**

```bash
git add apps/symphony/pi-extension/README.md
git commit -m "docs(symphony): document progress feedback"
```

If Step 2 required code/test fixes after Task 3, include those exact files in this commit and use:

```bash
git add apps/symphony/pi-extension/README.md apps/symphony/pi-extension/src/progress.ts apps/symphony/pi-extension/src/progress.test.ts apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/commands.test.ts apps/symphony/pi-extension/src/tools.ts apps/symphony/pi-extension/src/tools.test.ts
git commit -m "fix(symphony): complete progress indicator validation"
```

---

## Self-review

Spec coverage:

- Visible command progress: Task 1 and Task 2.
- Lightweight inline progress for refresh, attach, stop: Task 2.
- Blocking loader for start, init, doctor: Task 2.
- Tool partial progress updates: Task 3.
- Cleanup on success/failure/cancellation: Task 1 tests helper cleanup, Task 2 uses helper in commands.
- Manual acceptance documentation: Task 4 README plus existing spec manual acceptance.

Placeholder scan: no placeholder implementation steps remain.

Type consistency:

- `withSymphonyProgress(ctx, options, fn)` and `withSymphonyLoader(ctx, options, fn)` signatures are consistent across tests and command usage.
- `restoreStatus` always receives `ExtensionCommandContext` and delegates to `setSymphonyStatus(ctx, runtime)`.
- Tool update payloads consistently use `{ content: [{ type: "text", text }], details: { status: "working" } }`.
