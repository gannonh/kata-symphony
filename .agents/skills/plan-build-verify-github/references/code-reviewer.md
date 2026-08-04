# Code Quality Review Rubric

Use this compact rubric only when no compatible installed code-review skill is available. The reviewer must inspect the actual diff and code, not just the implementer report.

## Inputs to provide

- Description of what changed.
- Spec path and task ID.
- Task text, requirements, and acceptance criteria.
- Base SHA and head SHA.
- Test and verification evidence.
- Approved deviations, or `None`.

## Required inspection

Run or inspect the equivalent diff commands:

```bash
git diff --stat <BASE_SHA>..<HEAD_SHA>
git diff <BASE_SHA>..<HEAD_SHA>
```

Check the changed files and related call sites when needed to understand behavior.

## Review priorities

1. **Plan alignment**: implementation matches the approved task, acceptance criteria, non-goals, and approved deviations.
2. **Correctness**: behavior handles expected paths, edge cases, errors, and state transitions.
3. **Test quality**: tests verify user-visible or public-interface behavior and would fail for the wrong implementation.
4. **Maintainability**: changed units have clear responsibilities, names match project language, and complexity stays proportional to the task.
5. **Safety**: no data loss, credential leakage, insecure defaults, destructive migrations, or hidden operational risk.
6. **Integration**: changes fit existing project patterns, scripts, APIs, schemas, docs, and configuration.

## Severity calibration

- **Critical**: broken required behavior, data loss, security issue, destructive migration risk, or a failing required check.
- **Important**: missing acceptance coverage, fragile design, unhandled important edge case, unclear ownership boundary, or unapproved scope change.
- **Minor**: naming, small documentation gaps, local simplifications, or non-blocking polish.

Fix Critical issues immediately. Fix Important issues before continuing unless the user explicitly accepts the risk. Track Minor issues when they do not block the plan.

## Output format

```markdown
### Strengths
- <Specific evidence-backed strengths, or None>

### Issues

#### Critical
- File:line: <issue>. Why it matters: <reason>. Suggested fix: <fix>.

#### Important
- File:line: <issue>. Why it matters: <reason>. Suggested fix: <fix>.

#### Minor
- File:line: <issue>. Why it matters: <reason>. Suggested fix: <fix>.

### Recommendations
- <Process, testing, or follow-up recommendations>

### Assessment
Ready to continue: Yes | No | With fixes
Reasoning: <1-2 sentence technical assessment>
```

If there are no issues in a severity group, write `None`. Include file and line references whenever possible. If the plan is flawed rather than the implementation, say so directly and explain the plan change needed.
