# Build Workflow (GitHub)

Use this workflow to execute an approved spec issue through small implementation tasks, review gates, and verified completion.

Build is the second phase in Plan → Build → Verify. It starts from a GitHub Issue labeled `status:approved`. Read `references/github-conventions.md` before starting.

## Required inputs

- Spec issue number or URL. For an epic, the specific sub-issue to build.
- `status:approved` label on that issue, or explicit user override.
- `## Acceptance criteria` section with observable pass/fail outcomes.
- `## Build handoff` section with scope, non-goals, ordered phases, verification commands, and blocking open questions.

## Required bundled workflow

Implementation tasks must use the bundled TDD workflow before writing production code.

1. Read `references/tdd/workflow.md` completely.
2. Follow linked references under `references/tdd/` as needed.
3. Do not substitute ad hoc TDD guidance when the bundled workflow applies.

## Build workflow

### 1. Run Build preflight

Before editing files:

1. Run the `gh` preflight from `references/github-conventions.md`.
2. Read the issue completely:

```bash
gh issue view <N> --json number,title,body,labels,state,url,comments
gh sub-issue list <N>    # if the issue is an epic
```

3. Confirm the issue carries `status:approved` and the body's `## Status` section says `Approved`, or confirm the user explicitly overrode the approval gate. If the label and the body disagree, stop and reconcile with the user before writing code.
4. **If the issue is an epic** (`kind:epic`), do not build the parent. Pick the first `status:approved` child whose dependencies are already `status:verified` or `status:implemented`. If several are ready, ask which to build or confirm the order. State the choice before proceeding.
5. Confirm the issue contains `## Acceptance criteria` with concrete checkbox criteria. If missing or ambiguous, add `needs:acceptance-criteria`, stop, and return to Plan to fix the issue.
6. Run the phase-entry hygiene check from `references/github-conventions.md` and report findings.
7. Inspect repo instructions such as `AGENTS.md`, `CLAUDE.md`, and README command sections.
8. Check worktree state with `git status --short --branch`.
9. Confirm `Blocking open questions` is `None`, or confirm the user explicitly approved proceeding with listed questions.
10. Create or check out the working branch:

```bash
gh issue develop <N> --name "<N>-<kebab-title>" --base <default-branch> --checkout
```

Use plain `git switch -c <N>-<kebab-title>` if `gh issue develop` is unavailable or the repo blocks it. Do not start implementation on `main` or `master` without explicit user consent.

11. Capture a base SHA with `git rev-parse HEAD`.
12. Identify verification commands from the issue's `## Build handoff` and `## Verification` sections plus repo scripts.
13. Confirm required tools are available: todo tracking and subagent dispatch if using the subagent path.
14. Move the issue into the Build phase:

```bash
gh issue edit <N> --add-label "phase:build"
gh issue comment <N> --body "Build started on branch \`<branch>\` at base SHA \`<sha>\`."
```

Stop and ask if the issue is unapproved, the worktree has unrelated changes, the branch is unsafe, required tools are missing, or the issue has blocking questions.

### 2. Extract tasks and create todos

Extract implementation tasks from the issue's `## Implementation phases` and `## Build handoff` sections. Preserve the full task text, context, files, acceptance criteria, and verification commands.

Create todo items for all tasks when a todo tool is available. Keep exactly one implementation task in progress at a time.

### 3. Choose execution mode

Prefer the subagent path when subagent dispatch is available and the current agent is acting as orchestrator.

Use the single-agent path only when subagents are unavailable or the user explicitly asks you to work without them. Preserve the same gates: bundled TDD workflow, self-review, spec compliance check, code quality check, tests, and completion report.

## Subagent path

### 4. Dispatch the implementer

For each task, dispatch a fresh implementer subagent using `references/implementer-prompt.md`.

Give the subagent:

- Issue number and URL.
- The relevant issue body sections, pasted into the prompt.
- Task ID and full task text.
- Acceptance criteria for the task.
- Relevant code paths and repo context.
- Approved scope and non-goals.
- Base SHA for the task.
- Required verification commands.
- Instruction to read and follow `references/tdd/workflow.md` before writing implementation code.

Do not make the implementer read the issue to discover its own task. Provide the needed context directly. Implementers should not run `gh` write commands; issue state is the orchestrator's responsibility.

### 5. Handle implementer status

Implementers report one of four statuses:

- `DONE`: proceed to spec compliance review.
- `DONE_WITH_CONCERNS`: read concerns before review. Resolve correctness or scope concerns first.
- `NEEDS_CONTEXT`: provide missing context and re-dispatch.
- `BLOCKED`: assess whether to provide context, use a stronger model, split the task, or ask the user because the spec is wrong.

Never ignore an escalation or force the same retry without changing context, model, or task shape.

If the work is genuinely blocked, record it on the issue:

```bash
gh issue edit <N> --remove-label "status:approved" --add-label "status:blocked"
gh issue comment <N> --body-file "$BLOCKED_NOTE"
rm -f "$BLOCKED_NOTE"
```

### 6. Run spec compliance review

After implementation, dispatch a spec compliance reviewer using `references/spec-reviewer-prompt.md`.

The reviewer must inspect actual code and compare it to:

- The issue body (give the reviewer the issue number so it can run `gh issue view <N>`).
- Task text.
- Acceptance criteria.
- Non-goals.
- Approved deviations recorded in issue comments.

If the reviewer finds issues, send the task back to the implementer. Re-run spec compliance review after fixes. Do not proceed to code quality review until spec compliance passes.

### 7. Run code quality review

After spec compliance passes, dispatch a code quality reviewer using `references/code-quality-reviewer-prompt.md`. Prefer a compatible installed code-review skill when available; otherwise use the compact rubric in `references/code-reviewer.md`.

Provide:

- Task summary.
- Issue number and task ID.
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
- Concerns and approved deviations are recorded as issue comments.

Commit after each coherent task when project instructions require commits or the user requested commits. Stage only files changed for that task. Reference the issue in the commit body (`Refs #<N>`), not in the subject line.

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

Disclose in the Build completion report comment that independent subagent review was unavailable.

## Deviation policy

If repo facts invalidate the spec, pause before changing scope.

Examples:

- A named file or package does not exist.
- The planned API conflicts with installed library docs.
- The planned data model conflicts with existing migrations.
- A task requires credentials, destructive migration, or unrelated refactor not approved in the spec.

When this happens:

1. State the conflict clearly.
2. Propose the smallest adjustment.
3. Ask the user to approve the deviation.
4. Record the approved deviation as an issue comment starting with `## Approved deviation`.
5. Edit the issue body when the decision changes scope, acceptance criteria, task order, or verification. Acceptance criteria changes require the user's explicit approval, since Verify tests against them.

Do not silently implement a different spec.

## Final review

After all tasks pass their per-task gates:

1. Capture final head SHA.
2. Run the full verification command set from the issue.
3. Dispatch or perform a final whole-branch review against the issue body.
4. Fix final-review issues.
5. Re-run final review until no blocking issues remain.

## Open the pull request

```bash
gh pr create \
  --base <default-branch> \
  --head <branch> \
  --title "<issue title>" \
  --body-file "$PR_BODY"
rm -f "$PR_BODY"
```

The PR body must contain `Closes #<N>` for the issue being implemented. For a sub-issue, close the sub-issue, not the parent. Summarize scope, tasks, verification results, and approved deviations in the PR body, and link to the Build completion report comment.

If the repo's convention is to merge without a PR, say so and skip this step.

## Build completion report

Post the report as a comment on the issue:

```bash
REPORT="$(mktemp -t pbvg-report).md"
# write the report to "$REPORT"
gh issue comment <N> --body-file "$REPORT"
rm -f "$REPORT"
```

Start the comment with `## Build completion report` and include:

- Issue number and, for sub-issues, the parent.
- Branch name, base SHA, and final head SHA.
- Tasks completed.
- Files changed.
- Tests and verification commands run, with results.
- Review gates completed.
- Approved deviations, with links to their comments.
- Known follow-up issues, with links if they were opened.
- Whether independent subagent review was used.
- PR link.

Then move the issue to implemented:

```bash
gh issue edit <N> \
  --remove-label "status:approved,phase:build" \
  --add-label "status:implemented"
```

Update the body's `## Status` section to `Implemented` in the same turn. Do not check off acceptance criteria here; Verify owns that.

Do not close the issue. Verify closes it, or the merged PR closes it and Verify records acceptance before or immediately after the merge.

## Epic rollup

When the built issue is a sub-issue:

1. Comment on the parent with a one-line status and a link to the child's Build completion report.
2. Leave the parent at `status:approved` until every child is `status:verified` and the parent's own acceptance criteria pass.
3. Report which sibling is next in dependency order.

## Follow-up work

When Build surfaces work that is out of scope, open a separate issue rather than expanding the current one:

```bash
gh issue create --title "<follow-up>" --body-file "$BODY" --label "needs:triage"
rm -f "$BODY"
```

Link it from the Build completion report. Triage will groom it into the roadmap.

## Transition to Verify

After Build is complete, ask the user if they want to move to Verify.

If yes, transition to `references/verify.md` and follow it for acceptance review and validation.

## Red flags

Stop and ask when:

- The issue is not `status:approved` and the user has not explicitly overridden the gate.
- The `status:*` label and the body's `## Status` section disagree.
- The issue is an epic and no child was selected.
- Blocking open questions remain.
- The worktree contains unrelated changes.
- The branch is `main` or `master` and the user has not approved direct implementation there.
- No dedicated TDD workflow is bundled or readable.
- Acceptance criteria are missing, vague, or not testable.
- Required verification commands are unknown.
- Reviewers find unresolved issues.
- The spec is wrong or incomplete.

Never:

- Skip the bundled TDD workflow before implementation.
- Skip spec compliance review.
- Skip code quality review.
- Start code quality review before spec compliance passes.
- Mark a task complete while tests or review issues are failing.
- Dispatch multiple implementers in parallel against the same worktree.
- Let implementer self-review replace actual review.
- Edit acceptance criteria to match what was built.
- Close the spec issue from Build.
