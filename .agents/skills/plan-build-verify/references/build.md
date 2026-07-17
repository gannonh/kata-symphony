# Build Workflow

Use this workflow to execute an approved spec or implementation plan through small implementation tasks, review gates, and verified completion.

Build is the second phase in Plan → Build → Verify. It should start from approved spec and/or plan file path(s) with a Build handoff or implementation plan.

## Required inputs

- Spec and/or plan file path(s), usually `docs/specs/YYYY-MM-DD-<topic>.md` or an implementation plan referenced by the user.
- `## Acceptance criteria` section with observable pass/fail outcomes.
- Build handoff or plan section with scope, non-goals, ordered tasks, verification commands, and blocking open questions.
- Explicit user approval if the spec or plan status is not `Approved`.

## Required bundled workflow

Implementation tasks must use the bundled TDD workflow before writing production code.

1. Read `references/tdd/workflow.md` completely.
2. Follow linked references under `references/tdd/` as needed.
3. Do not substitute ad hoc TDD guidance when the bundled workflow applies.

## Build workflow

### 1. Run Build preflight

Before editing files:

1. Read the approved spec and/or plan file path(s) completely.
2. Confirm frontmatter `status: Approved` and the document body's `## Status` section is `Approved`, or confirm the user explicitly overrode the approval gate. If they disagree, stop and return to Plan to reconcile.
3. Confirm the spec or plan contains `## Acceptance criteria` with concrete criteria. If missing or ambiguous, stop and return to Plan to fix the source document.
4. Confirm `Blocking open questions` is `None`, or confirm the user explicitly approved proceeding with listed questions.
5. Inspect repo instructions such as `AGENTS.md`, `CLAUDE.md`, and README command sections.
6. Check worktree state with `git status --short --branch`.
7. Identify the current branch. Do not start implementation on `main` or `master` without explicit user consent.
8. Capture a base SHA with `git rev-parse HEAD`.
9. Identify verification commands from the spec and/or plan and repo scripts.
10. Confirm required tools are available: todo tracking and subagent dispatch if using the subagent path.

Stop and ask if the spec or plan is unapproved, the worktree has unrelated changes, the branch is unsafe, required tools are missing, or the plan has blocking questions.

### 2. Extract tasks and create todos

Extract implementation tasks from the approved spec and/or plan. Preserve the full task text, context, files, acceptance criteria, and verification commands.

Create todo items for all tasks when a todo tool is available. Keep exactly one implementation task in progress at a time.

### 3. Choose execution mode

Prefer the subagent path when subagent dispatch is available and the current agent is acting as orchestrator.

Use the single-agent path only when subagents are unavailable or the user explicitly asks you to work without them. Preserve the same gates: bundled TDD workflow, self-review, spec compliance check, code quality check, tests, and completion report.

## Subagent path

### 4. Dispatch the implementer

For each task, dispatch a fresh implementer subagent using `references/implementer-prompt.md`.

Give the subagent:

- Spec and/or plan file path(s).
- Task ID and full task text.
- Acceptance criteria for the task.
- Relevant code paths and repo context.
- Approved scope and non-goals.
- Base SHA for the task.
- Required verification commands.
- Instruction to read and follow `references/tdd/workflow.md` before writing implementation code.

Do not make the implementer read the plan file to discover its own task. Provide the needed context directly.

### 5. Handle implementer status

Implementers report one of four statuses:

- `DONE`: proceed to spec compliance review.
- `DONE_WITH_CONCERNS`: read concerns before review. Resolve correctness or scope concerns first.
- `NEEDS_CONTEXT`: provide missing context and re-dispatch.
- `BLOCKED`: assess whether to provide context, use a stronger model, split the task, or ask the user because the plan is wrong.

Never ignore an escalation or force the same retry without changing context, model, or task shape.

### 6. Run spec compliance review

After implementation, dispatch a spec compliance reviewer using `references/spec-reviewer-prompt.md`.

The reviewer must inspect actual code and compare it to:

- Approved spec and/or plan.
- Task text.
- Acceptance criteria.
- Non-goals.
- Approved deviations.

If the reviewer finds issues, send the task back to the implementer. Re-run spec compliance review after fixes. Do not proceed to code quality review until spec compliance passes.

### 7. Run code quality review

After spec compliance passes, dispatch a code quality reviewer using `references/code-quality-reviewer-prompt.md`. Prefer a compatible installed code-review skill when available; otherwise use the compact rubric in `references/code-reviewer.md`.

Provide:

- Task summary.
- Spec and/or plan file path(s) and task ID.
- Base SHA before task.
- Head SHA after implementation.
- Test evidence.
- Approved deviations.

If the reviewer finds Critical or Important issues, send them back to the implementer and re-run code quality review after fixes. Do not mark the task complete while review issues remain open.

### 8. Complete the task

A task is complete only when:

- Bundled TDD workflow from `references/tdd/workflow.md` was followed.
- Required tests and verification commands pass.
- Spec compliance review passes.
- Code quality review passes.
- Concerns and approved deviations are recorded.

Commit after each coherent task when project instructions require commits or the user requested commits. Stage only files changed for that task.

Mark the todo item complete only after the task meets all completion criteria.

Repeat steps 4-8 for each task.

## Single-agent path

Use this path only when subagents are unavailable or disallowed.

For each task:

1. Read the task text and acceptance criteria.
2. Read and follow `references/tdd/workflow.md` before writing implementation code.
3. Implement the smallest slice that satisfies the task.
4. Run required verification commands.
5. Perform a written spec compliance check against the task and non-goals.
6. Perform a written code quality check using a compatible installed code-review skill or the compact rubric in `references/code-reviewer.md`.
7. Fix issues and re-run checks until clean.
8. Record evidence and deviations.
9. Commit if project instructions or the user require commits.
10. Mark the todo complete.

Disclose in the final Build report that independent subagent review was unavailable.

## Deviation policy

If repo facts invalidate the plan, pause before changing scope.

Examples:

- A named file or package does not exist.
- The planned API conflicts with installed library docs.
- The planned data model conflicts with existing migrations.
- A task requires credentials, destructive migration, or unrelated refactor not approved in the spec.

When this happens:

1. State the conflict clearly.
2. Propose the smallest plan adjustment.
3. Ask the user to approve the deviation.
4. Update the spec if the decision changes scope, acceptance criteria, task order, or verification.

Do not silently implement a different plan.

## Final review

After all tasks pass their per-task gates:

1. Capture final head SHA.
2. Run the full verification command set from the spec and/or plan.
3. Dispatch or perform a final whole-branch review against the approved spec and/or plan.
4. Fix final-review issues.
5. Re-run final review until no blocking issues remain.
6. Update the spec or plan status from `Approved` to `Implemented` in both frontmatter and the document body's `## Status` section, or note why status could not be updated.

## Build completion report

Create a concise Build completion report. Prefer adding it to the spec or plan under `## Build completion report`; if the source document should remain unchanged, write an adjacent file such as:

```text
docs/specs/YYYY-MM-DD-<topic>-build-report.md
```

Include:

- Spec and/or plan file path(s).
- Base SHA and final head SHA.
- Tasks completed.
- Files changed.
- Tests and verification commands run, with results.
- Review gates completed.
- Approved deviations.
- Known follow-up issues.
- Whether independent subagent review was used.

## Transition to Verify

After Build is complete, ask the user if they want to move to Verify.

If yes, transition to `references/verify.md` and follow it for acceptance review and validation.

## Red flags

Stop and ask when:

- The spec or plan is not approved and the user has not explicitly overridden the gate.
- Blocking open questions remain.
- The worktree contains unrelated changes.
- The branch is `main` or `master` and the user has not approved direct implementation there.
- No dedicated TDD workflow is bundled or readable.
- Acceptance criteria are missing, vague, or not testable.
- Required verification commands are unknown.
- Reviewers find unresolved issues.
- The plan is wrong or incomplete.

Never:

- Skip the bundled TDD workflow before implementation.
- Skip spec compliance review.
- Skip code quality review.
- Start code quality review before spec compliance passes.
- Mark a task complete while tests or review issues are failing.
- Dispatch multiple implementers in parallel against the same worktree.
- Let implementer self-review replace actual review.
