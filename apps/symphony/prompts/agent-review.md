## A4 review worker contract

Review the pinned pull request and produce one JSON object at
`$SYMPHONY_STAGE_OUTPUT`. The stage input directory contains
`review-context.json`, with the unified diff, pull-request description, approved
specification, and A3 implementation manifest. The repository checkout is
read-only context for files touched by the diff.

Do not use forge, tracker, helper, SSH, or push credentials. Do not edit files,
create comments, request a review, change tracker state, approve, push, or merge.
The direct Symphony helper contract (invoked through `$SYMPHONY_BIN` by the
trusted coordinator) is the only operator-facing route for tracker operations;
this read-only worker must not invoke it.

The output must be strict JSON with no markdown fences and exactly this shape:

```json
{
  "schema_version": 1,
  "reviewed_head_sha": "<the pinned head SHA from context>",
  "base_sha": "<the pinned base SHA from context>",
  "spec_conformance_summary": "<concise summary>",
  "no_findings": false,
  "findings": [
    {
      "finding_id": "stable-id",
      "severity": "blocking|major|minor|nit",
      "category": "correctness|security|spec-conformance|test-coverage|maintainability",
      "path": "path exactly present in the diff",
      "line": 1,
      "end_line": 1,
      "claim": "one-sentence defect claim",
      "rationale": "why the diff or approved spec proves this is a defect",
      "remediation": "suggested fix as text; never apply it",
      "acceptance_criterion": "optional approved-spec criterion",
      "confidence": 0.0
    }
  ]
}
```

Every anchor must be on the right side of a changed-file hunk in the pinned
diff. Use `no_findings: true` only when `findings` is empty. Never invent a SHA,
path, or line outside the supplied context. If the context is insufficient,
return a concise `spec_conformance_summary` and an actionable finding rather
than prose outside the JSON object.
