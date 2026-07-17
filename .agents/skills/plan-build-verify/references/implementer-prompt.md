# Implementer Subagent Prompt Template

Use this template when dispatching an implementer subagent from the Build workflow.

```text
Task tool (general-purpose):
  description: "Implement [TASK_ID]: [task name]"
  prompt: |
    You are implementing [TASK_ID]: [task name].

    ## Spec

    Spec path: [SPEC_PATH]
    Spec status: [Approved or explicit override]

    ## Task description

    [FULL TEXT of task from approved spec. Paste it here. Do not make the subagent read the whole spec to discover the task.]

    ## Acceptance criteria

    [Task-specific acceptance criteria]

    ## Approved scope and non-goals

    Scope:
    [What this task may change]

    Non-goals:
    [What this task must not change]

    ## Context

    [Scene-setting: where this fits, dependencies, architecture, relevant files, and project conventions]

    ## Git range

    Base SHA before this task: [BASE_SHA]

    ## Required verification

    [Commands and checks required for this task]

    ## Required development workflow

    Read and follow `references/tdd/workflow.md` (relative to the plan-build-verify skill directory) before writing production code.

    If that bundled workflow is unavailable, report BLOCKED and explain that the required TDD workflow is missing. Do not substitute another TDD workflow.

    ## Before you begin

    Ask questions now if any requirement, acceptance criterion, dependency, or implementation strategy is unclear.

    Raise concerns before starting work. It is better to pause than to guess.

    ## Your job

    Once clear on requirements:

    1. Read and follow `references/tdd/workflow.md`.
    2. Implement exactly what the task specifies.
    3. Write or update tests through the TDD workflow.
    4. Run required verification commands.
    5. Commit your work when project instructions require commits or the controller explicitly asks for a commit.
    6. Self-review.
    7. Report back.

    ## Code organization

    - Follow the file structure defined in the spec.
    - Keep files focused on one clear responsibility with a well-defined interface.
    - Follow established project patterns.
    - Improve code you touch when it directly serves the task.
    - Do not restructure unrelated code.
    - If a file grows beyond the plan's intent, report DONE_WITH_CONCERNS instead of inventing a larger refactor.

    ## When to escalate

    Report BLOCKED or NEEDS_CONTEXT when:

    - The task requires architectural decisions with multiple valid approaches.
    - The plan conflicts with the repo.
    - You need credentials or destructive actions not approved in the spec.
    - You are unsure whether your approach satisfies the acceptance criteria.
    - The task involves restructuring not anticipated by the spec.
    - The bundled TDD workflow at `references/tdd/workflow.md` is unavailable.

    ## Self-review before reporting

    Check:

    - Did I implement everything in the task and nothing outside it?
    - Did I preserve non-goals?
    - Did I follow `references/tdd/workflow.md`?
    - Did tests verify behavior, not implementation details?
    - Did verification commands pass?
    - Are names clear and consistent with project language?
    - Did I avoid speculative features?

    Fix issues before reporting when possible.

    ## Report format

    - Status: DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT
    - Task ID:
    - Spec path:
    - Base SHA:
    - Head SHA, if changed:
    - What changed:
    - Files changed:
    - Tests and verification run:
    - TDD workflow usage summary:
    - Self-review findings:
    - Approved deviations used:
    - Concerns or blockers:
```
