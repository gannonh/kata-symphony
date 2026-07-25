# Evidence

Each test run writes under `uat-evidence/` by default:

- `evidence.json`: machine-readable proof.
- `evidence.md`: human-readable proof summary.
- `payloads/`: request payloads used for operations.
- Runtime-specific fixtures, such as `.kata/preferences.md` or `WORKFLOW.md`.

Evidence should include:

- runtime identity
- backend identity
- sanitized effective backend configuration (coordinates and token environment-variable name, never a token value)
- workspace
- runtime root or binary path
- git commit
- timestamp
- health result
- expected, observed, skipped, and missing operation coverage when available
- created backend state
- provider proof links
- provider read-back results
- retry, warning, and skip records
- cleanup status

For Symphony GitHub evidence, cleanup resolves the repository from explicit
cleanup overrides, then stored evidence configuration, then a unanimous
repository parsed from legacy created-issue URLs. Every created issue URL is
validated against that repository before the first provider read or mutation.
Ambiguous or conflicting evidence fails closed instead of inheriting the
current workspace's `WORKFLOW.md`.

## Kata CLI Pass Criteria

A Kata CLI backend test passes when:

- `health.check` succeeds.
- Every current CLI operation is observed.
- `project.getSnapshot` reports milestone readiness before completion.
- The created milestone is completed.
- Artifact proof checks pass.

## Symphony Runtime Pass Criteria

A Symphony runtime backend test passes when:

- `symphony doctor` succeeds for the generated workflow fixture.
- Every shared helper operation is observed.
- GitHub-only PR helper operations are observed when a PR is discoverable, or skipped with a concrete reason.
- Provider reads confirm issues, comments, documents, and proof links.
- Evidence files are written.

## Artifact and Provider Proof Checks

GitHub:

- Fetch created issues and comments through GitHub REST.
- Verify provider URLs are present.
- Record GitHub PR helper results or a skip reason when no PR is discoverable.

Linear:

- Fetch created issues, documents, and comments through Linear GraphQL.
- Verify provider URLs are present.
- Record created parent, child, follow-up, artifact, and document state as applicable.

## Failure Reporting

Report:

- runtime
- backend
- failed operation
- provider error message
- retry count or skip reason
- evidence path
- cleanup command

Do not summarize a failed run as success if cleanup completed.
