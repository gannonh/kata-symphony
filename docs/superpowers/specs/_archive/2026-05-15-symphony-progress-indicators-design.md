---
type: Spec
title: Symphony Progress Indicators Design
description: Archived spec document: Symphony Progress Indicators Design.
tags: [archive, symphony]
timestamp: 2026-07-15T20:00:00Z
---

# Symphony progress indicators design

Date: 2026-05-15
Scope: Add visible activity/progress feedback for Symphony Pi extension commands and tools that take a few seconds to complete.

## Goal

Symphony commands should show immediate visible activity while they run, then restore the normal Pi UI state when they finish or fail.

The user-facing shape follows Pi's existing working indicator style:

```text
[spinner] Working... [status]
```

## Non-goals

- No persistent job queue.
- No new command semantics.
- No changes to Symphony server APIs.
- No unrelated dashboard layout changes.

## Target commands

Use lightweight inline progress for short operations:

- `/symphony:refresh` shows `Refreshing Symphony...`
- `/symphony:attach` shows `Attaching to Symphony...`
- `/symphony:stop` shows `Stopping Symphony...`

Use a blocking loader panel for longer operations:

- `/symphony:start` shows `Starting Symphony...`
- `/symphony:init` shows `Initializing Symphony...`
- `/symphony:doctor` shows `Running Symphony doctor...`

The blocking loader should be cancellable where the underlying operation accepts an abort signal.

## Design

Add `apps/symphony/pi-extension/src/progress.ts` with two helpers:

1. `withSymphonyProgress(ctx, options, fn)`
   - Sets a Pi working indicator with dot/spinner frames.
   - Sets a working message.
   - Sets the `symphony` footer status to the active message.
   - Runs `fn`.
   - Restores the default working indicator/message in `finally`.
   - Restores the Symphony connection status in `finally` through a supplied callback.

2. `withSymphonyLoader(ctx, options, fn)`
   - Uses Pi's `BorderedLoader` for longer command handlers.
   - Passes the loader abort signal to `fn`.
   - Closes/cleans up the loader in all completion paths.
   - Restores Symphony footer status after completion.

Command handlers keep their existing success/error notifications. Progress helpers only add activity feedback and cleanup.

## Tool progress

Tools should use their existing `onUpdate` callback for partial progress:

- `symphony_start`: `Starting Symphony...`
- `symphony_attach`: `Attaching to Symphony...`
- `symphony_refresh`: `Refreshing Symphony...`
- `symphony_steer`: `Sending steer instruction...`
- `symphony_stop`: `Stopping Symphony...`
- `symphony_init`: `Initializing Symphony...`
- `symphony_doctor`: `Running Symphony doctor...`

Tool final results remain unchanged.

## Error handling

- Progress clears in `finally` on success, failure, and cancellation.
- Existing error formatting and notifications remain the source of user-visible failure text.
- Cancellation should abort only the current command operation.
- If a command cannot use a signal today, it still shows progress and clears it correctly.

## Testing

Add focused tests for:

- Progress helper restores working indicator/message/status after success.
- Progress helper restores UI after failure.
- Long-running command wrapper passes an abort signal to the operation.
- Command handlers set progress for refresh, attach, stop, start, init, and doctor.
- Tools emit partial `onUpdate` progress before final results for changed tools.

## Manual acceptance

1. Run `pi -e ./apps/symphony/pi-extension`.
2. Run `/symphony:doctor`.
   Expected: a blocking progress loader appears until the command completes.
3. Run `/symphony:attach http://127.0.0.1:<port>`.
   Expected: inline working text/status appears while attach verifies the server.
4. Run `/symphony:refresh`.
   Expected: inline working text/status appears while refresh completes.
5. Trigger a failing command.
   Expected: progress clears and the existing error notification appears.
