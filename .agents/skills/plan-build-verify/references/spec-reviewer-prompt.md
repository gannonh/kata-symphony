# Spec Compliance Reviewer Prompt Template

Use this template when dispatching a spec compliance reviewer subagent.

Purpose: verify the implementation built what was requested, nothing more and nothing less.

```text
Task tool (general-purpose):
  description: "Review spec compliance for [TASK_ID]"
  prompt: |
    You are reviewing whether an implementation matches its approved specification.

    ## Spec and task

    Spec path: [SPEC_PATH]
    Task ID: [TASK_ID]

    ## What was requested

    [FULL TEXT of task requirements]

    ## Acceptance criteria

    [Task-specific acceptance criteria]

    ## Approved scope and non-goals

    Scope:
    [Approved scope]

    Non-goals:
    [Non-goals]

    ## Approved deviations

    [Any approved deviations, or "None"]

    ## Implementer report

    [From implementer's report]

    ## Git range

    Base SHA: [BASE_SHA]
    Head SHA: [HEAD_SHA]

    ## Review rules

    Do not rely on the implementer report. Verify independently by reading the actual code and diff.

    Check:

    - Did the implementation satisfy every requested requirement?
    - Did it satisfy each acceptance criterion?
    - Did it preserve non-goals?
    - Did it avoid unapproved extra features or scope changes?
    - Did it misunderstand any requirement?
    - Did it claim work that is not present in the code?
    - Did it update tests or docs required by the task?

    Use file and line references where possible.

    ## Output format

    ### Verdict
    ✅ Spec compliant
    or
    ❌ Issues found

    ### Evidence
    [Concise evidence from code, tests, and diff]

    ### Issues
    [Specific missing, extra, or misunderstood requirements with file:line references]

    ### Required fixes
    [What must change before this can proceed to code quality review]
```
