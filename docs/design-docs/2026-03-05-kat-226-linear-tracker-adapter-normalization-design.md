# KAT-226 Linear Tracker Adapter and Normalization Contract Design

## Context

- Ticket: KAT-226
- Goal: Implement `SPEC.md` Section 11 integration behavior for the Linear-compatible tracker adapter.
- Project context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M2 Parallel Subsystems`
  - Linear docs: `Project Spec`, `Symphony v1 Execution Plan (Dependency DAG)`
- Parent/dependency context reviewed:
  - Blocked by `KAT-223` (`Done`) for typed config defaults/env resolution.
  - This ticket blocks `KAT-229`, `KAT-230`, `KAT-233`, `KAT-236`.
  - Parallel worktree ownership comment (2026-03-05) constrains this ticket to tracker adapter + normalization only.
- Existing code context reviewed:
  - `src/tracker/contracts.ts` (`TrackerClient` methods already fixed)
  - `src/domain/models.ts` (`Issue` and `blocked_by` shape)
  - `src/domain/normalization.ts` (existing state/workspace helpers)
  - `src/config/build-effective-config.ts` and config defaults for tracker endpoint/states
  - `src/bootstrap/service.ts` currently uses a no-op tracker stub

## References Reviewed

- `SPEC.md` Sections 11.1-11.4 (tracker operations, Linear query semantics, normalization rules, error mapping)
- `SPEC.md` Section 17.3 (tracker conformance matrix)
- `SPEC.md` Section 18.1 (required conformance checklist item for tracker client)
- Linear issue `KAT-226` description and acceptance criteria
- Linear issue comment `aede0f75-2cb9-489c-9a88-f35e7adc14e1` (parallel worktree contract)
- Linear project docs:
  - `Project Spec`
  - `Symphony v1 Execution Plan (Dependency DAG)`
- `ARCHITECTURE.md` integration-layer boundaries
- `WORKFLOW.md` tracker defaults (`project_slug`, `active_states`, `terminal_states`)

## Assumptions

1. Linear schema drift risk is handled by isolating query strings and validating payload shape explicitly.
2. Adapter errors must be typed and stable so orchestrator logic can branch on `code` rather than string matching.
3. `TrackerClient` method signatures in `src/tracker/contracts.ts` remain unchanged (cross-ticket freeze).
4. This ticket implements tracker read paths only (no tracker writes), consistent with SPEC Section 11.5 and issue boundaries.

## Options Considered

1. Single-file adapter (query + HTTP + normalization in one module)
   - Pros: fastest to ship initially.
   - Cons: high coupling, harder to unit-test pagination/error paths, fragile for schema drift.

2. Layered adapter with isolated query, transport, normalization, and error mapping modules (selected)
   - Pros: deterministic tests per concern, easier schema updates, explicit typed error boundaries.
   - Cons: slightly more boilerplate/files.

3. Generic GraphQL client abstraction shared across future trackers
   - Pros: reuse if more GraphQL trackers appear.
   - Cons: premature abstraction for current `tracker.kind == linear` scope; delays this ticket.

## Selected Approach

Implement a Linear-specific tracker adapter with strict internal boundaries:

- Query construction is isolated from transport and normalization.
- `TrackerClient` public surface remains exactly the three existing methods.
- Each method enforces deterministic behavior for pagination and empty inputs.
- All failure paths map to stable Section 11.4-aligned error codes.

## Proposed Module Layout

1. `src/tracker/errors.ts`
   - Defines typed tracker integration error class and constructors.
   - Codes include:
     - `linear_api_request`
     - `linear_api_status`
     - `linear_graphql_errors`
     - `linear_unknown_payload`
     - `linear_missing_end_cursor`

2. `src/tracker/linear/queries.ts`
   - Contains GraphQL query documents and variable typings for:
     - candidate fetch by active states + project slug
     - state refresh by issue IDs (`[ID!]`)
     - terminal-state fetch for cleanup

3. `src/tracker/linear/http.ts`
   - Centralized POST execution against configured endpoint.
   - Adds auth header.
   - Enforces timeout `30000ms` via abort signal.
   - Maps request/non-200/graphql errors to typed codes.

4. `src/tracker/linear/normalize.ts`
   - Converts Linear nodes to domain `Issue`.
   - Applies normalization rules from Section 11.3.

5. `src/tracker/linear/client.ts`
   - Implements `TrackerClient`:
     - `fetchCandidates()`
     - `fetchIssuesByIds(issueIds)`
     - `fetchTerminalIssues()`
   - Handles pagination loops and empty-input guard.

6. `src/tracker/index.ts`
   - Exports `createLinearTrackerClient` factory.

## API and Data-Flow Design

### Factory shape

`createLinearTrackerClient(configProvider, fetchImpl?) -> TrackerClient`

- Reads endpoint, api key, project slug, active states, terminal states from current config snapshot.
- Accepts optional fetch injection for deterministic tests.

### Operation behavior

1. `fetchCandidates()`
   - Query by `project.slugId == tracker.project_slug` and `state.name in tracker.active_states`.
   - Uses pagination default size `50`.
   - Aggregates pages in order until `hasNextPage == false`.

2. `fetchIssuesByIds(issueIds)`
   - If input list is empty: return `[]` immediately, no API call.
   - Query issues by GraphQL IDs using variable type `[ID!]`.
   - Returns normalized issues in API order (deterministic behavior documented by tests).

3. `fetchTerminalIssues()`
   - Query by project slug and `state.name in tracker.terminal_states`.
   - Uses same pagination behavior as candidates.

### Pagination contract

- Request `first: 50` by default.
- Cursor loop uses `pageInfo.hasNextPage` and `pageInfo.endCursor`.
- If `hasNextPage == true` but cursor missing/null, raise `linear_missing_end_cursor`.

## Normalization Contract

`normalizeLinearIssue(node) -> Issue`

Rules:

1. Stable issue fields
   - `id`, `identifier`, `title`, `description`, `state`, `branch_name`, `url` mapped directly with null-safe fallback.

2. Labels
   - Normalize to lowercase string values.

3. Blockers (`blocked_by`)
   - Derived from inverse relations where relation type is `blocks`.
   - Map each blocker to `{id, identifier, state}`.

4. Priority
   - Keep only integer values.
   - Non-integer or missing values => `null`.

5. Timestamps
   - Parse `createdAt` and `updatedAt` as ISO dates.
   - Invalid values => `null`.

## Error Mapping Contract

Failures map to typed error codes so orchestrator behavior can remain spec-compliant:

1. `linear_api_request`
   - Network/transport failures, timeout aborts, DNS/connectivity errors.

2. `linear_api_status`
   - HTTP response status not `2xx`.

3. `linear_graphql_errors`
   - Response JSON has non-empty top-level `errors`.

4. `linear_unknown_payload`
   - Response missing expected `data` structure or required query fields.

5. `linear_missing_end_cursor`
   - Pagination integrity violation when `hasNextPage` is true without cursor.

## Testing and Verification Strategy

Add focused tests under `tests/tracker/`.

1. Query semantics coverage
   - Candidate query applies active states + project slug.
   - Terminal query applies terminal states + project slug.
   - State refresh query variables typed/used as ID list semantics (`[ID!]`).

2. Pagination determinism
   - Multi-page candidate and terminal fetch preserve aggregate order.
   - Missing `endCursor` when `hasNextPage` true raises `linear_missing_end_cursor`.

3. Empty input behavior
   - `fetchIssuesByIds([])` returns `[]` without invoking HTTP transport.

4. Normalization coverage
   - Label lowercasing.
   - Inverse blocker mapping from `blocks` relation.
   - Priority integer/null behavior.
   - ISO timestamp parse behavior.

5. Error mapping coverage
   - Request failure -> `linear_api_request`.
   - Non-200 status -> `linear_api_status`.
   - GraphQL `errors` payload -> `linear_graphql_errors`.
   - Malformed payload -> `linear_unknown_payload`.

6. End-to-end adapter contract coverage
   - `TrackerClient` method return types and deterministic outputs against fixture payloads.

Verification commands (implementation phase):

- `pnpm test -- tests/tracker`
- `pnpm test`
- `make check`

## Scope Boundaries

### In scope

- Linear tracker adapter implementation in `src/tracker/**`
- Deterministic read-path query behavior for candidates, ID refresh, and terminal fetch
- Section 11.3 normalization contract
- Section 11.4 error category mapping
- Tracker integration tests

### Out of scope

- Orchestrator scheduling/claim/retry behavior (`KAT-225`, `KAT-230`)
- Workspace/hook runtime behavior (`KAT-227`)
- Codex app-server runner behavior (`KAT-228`)
- Tracker write APIs/state mutation surface
- Changes to `src/execution/contracts.ts` or broader orchestrator contract signatures

## Risks and Mitigations

1. Linear schema drift changes nested relation fields
   - Mitigation: isolate query strings and strict payload guards; fail as `linear_unknown_payload`.

2. Pagination loop bugs can duplicate or skip issues
   - Mitigation: dedicated multi-page fixture tests and explicit cursor invariant checks.

3. Inverse blocker interpretation mismatch
   - Mitigation: lock relation filtering logic to SPEC wording (`inverse relations of type blocks`) and test against representative fixtures.

4. Timeout behavior inconsistent across Node versions
   - Mitigation: centralize timeout logic in transport helper and mock fetch abort in tests.

## Handoff

This design defines the adapter/normalization contract required for KAT-226 and keeps interfaces compatible for parallel tickets KAT-227/KAT-228/KAT-229/KAT-230 integration.
