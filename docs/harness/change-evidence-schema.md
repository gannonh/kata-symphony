# Change Evidence Schema

Last reviewed: 2026-03-06

## Goal

Define the minimum artifact structure for evidence-backed repo changes. These artifacts
let the harness verify that agents loaded the right context, updated the right durable
docs, and recorded concrete verification evidence.

## Required Artifacts

Meaningful changes should produce both of the following artifacts:

1. `docs/generated/change-evidence/<date>-<topic>.md`
2. `docs/generated/change-evidence/<date>-<topic>.json`

The markdown file is for human review. The JSON file is for scripts and hooks.

## Markdown Artifact

### Required sections

1. `Summary`
   - What changed and why.
2. `Changed Files`
   - The code/docs/config paths touched by the change.
3. `Context Loaded`
   - The source-of-truth files the agent used.
4. `Decision Artifacts`
   - Linked design docs or implementation plans that justify the change.
5. `Canonical Docs Updated`
   - The durable docs updated as a result of the change.
6. `Waivers`
   - Explicit justification for any required doc that was not updated.
7. `Verification`
   - Commands run, relevant outcomes, and unresolved risks.
8. `Verification Artifacts`
   - Linked verification docs or summaries produced by the change.

### Required content rules

- The file must contain at least one context file path.
- The file must list at least one changed path.
- The file must list at least one decision artifact for architecture-sensitive changes.
- A waiver entry must name the skipped doc and explain why no update was needed.
- Verification must include concrete commands, not only a success claim.
- Listing a doc under `Context Loaded` does not authorize editing it. Canonical
  doc edits must appear under `Canonical Docs Updated` or under `Waivers`.

## JSON Artifact

### Required top-level fields

```json
{
  "topic": "build-time-harness",
  "summary": "One sentence summary",
  "changedFiles": ["scripts/harness/check_repo_contract.sh"],
  "contextLoaded": ["ARCHITECTURE.md", "docs/harness/context-map.yaml"],
  "decisionArtifacts": [
    "docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md",
    "docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md"
  ],
  "canonicalDocsUpdated": ["docs/harness/BUILDING-WITH-HARNESS.md"],
  "waivers": [
    {
      "doc": "SECURITY.md",
      "reason": "No security posture change in this task"
    }
  ],
  "verification": [
    {
      "command": "bash scripts/harness/check_repo_contract.sh",
      "result": "pass"
    }
  ],
  "verificationArtifacts": ["docs/generated/harness-verification.md"],
  "impactedAreas": ["harness", "documentation"]
}
```

### Field requirements

- `topic`: short identifier for the change.
- `summary`: one-sentence description.
- `changedFiles`: array of changed repo-relative paths for the current diff
  being validated. The harness compares this field to the actual diff and will
  reject mismatches.
- `contextLoaded`: array of source-of-truth files loaded for the work.
- `decisionArtifacts`: array of linked design or plan docs.
- `canonicalDocsUpdated`: array of durable docs updated by the change.
- `waivers`: array of `{doc, reason}` objects.
- `verification`: array of `{command, result}` objects.
- `verificationArtifacts`: array of linked verification docs.
- `impactedAreas`: array of high-level affected areas such as `architecture`,
  `security`, `reliability`, `workflow`, `tests`, `docs`, or `harness`.

## Context Map Contract

`docs/harness/context-map.yaml` defines the ownership map the harness uses to
connect changed paths to required context and durable docs.

Local branch validation resolves the diff from the branch merge-base with
`origin/main`, `main`, `origin/master`, `master`, or the configured upstream
before falling back to `HEAD~1`.

### Required fields per rule

- `pattern`: repo-relative glob for changed paths.
- `owned_by`: list of source-of-truth docs or durable doc directories.
- `impacted_areas`: list of high-level impacted areas recorded in the JSON artifact.
- `notes`: short explanation of why the mapping exists.

## Ownership Examples

- `src/config/**`
  - `SPEC.md`
  - `ARCHITECTURE.md`
  - `docs/design-docs/`
- `src/execution/**`
  - `SPEC.md`
  - `ARCHITECTURE.md`
  - `SECURITY.md`
  - `RELIABILITY.md`
- `src/orchestrator/**`
  - `SPEC.md`
  - `ARCHITECTURE.md`
  - `RELIABILITY.md`
- `WORKFLOW.md`
  - `SECURITY.md`
  - `docs/references/harness-engineering.md`
  - `docs/harness/BUILDING-WITH-HARNESS.md`
