# Code Quality Reviewer Prompt Template

Use this template when dispatching a code quality reviewer subagent.

Purpose: verify the implementation is well-built, maintainable, tested, and safe to continue from.

Only dispatch after spec compliance review passes.

```text
Task tool (general-purpose):
  Prefer a compatible installed code-review skill when available. Otherwise use the compact local rubric at references/code-reviewer.md

  DESCRIPTION: [Task summary from implementer's report]
  SPEC_PATH: [Spec path]
  TASK_ID: [Task ID]
  PLAN_OR_REQUIREMENTS: [Task text and acceptance criteria]
  BASE_SHA: [Commit before task]
  HEAD_SHA: [Current commit]
  TEST_EVIDENCE: [Commands run and results]
  APPROVED_DEVIATIONS: [Approved deviations or "None"]
```

In addition to the selected reviewer instructions, ask the reviewer to check:

- Does each changed file have one clear responsibility with a well-defined interface?
- Are units decomposed so they can be understood and tested independently?
- Is the implementation following the file structure from the approved spec?
- Did this task create new files that are already large, or significantly grow existing files? Focus on what this change contributed.
- Did tests verify behavior through public interfaces where practical?
- Are any deviations from the plan approved and documented?

Code reviewer returns: Strengths, Issues grouped by Critical, Important, and Minor, Recommendations, and Assessment.
