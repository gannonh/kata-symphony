# KAT-222 WORKFLOW.md Discovery and Parser Contract Design

## Context

- Ticket: KAT-222
- Goal: Implement `WORKFLOW.md` discovery and parser behavior from `SPEC.md` Sections 5.1-5.2.
- Parent/dependency context reviewed:
  - `KAT-221` (`Done`) unblocked this ticket and established core `WorkflowDefinition` contract.
  - `KAT-225` depends on this output for startup/tick preflight validation.
  - `KAT-234` depends on this output for CLI path/default lifecycle behavior.
- Project context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M1 Foundations & Contract Loading`
  - Linear docs: `Project Spec`, `Symphony v1 Execution Plan (Dependency DAG)`

## References Reviewed

- `SPEC.md` Section 5.1 (workflow file path precedence and missing-file behavior)
- `SPEC.md` Section 5.2 (front matter + prompt body parsing contract)
- `SPEC.md` Section 5.5 (error classes)
- `SPEC.md` Section 17.1 and 18.1 (conformance checks and implementation checklist)
- `WORKFLOW.md` (current repository contract example)
- Existing baseline contracts:
  - `src/domain/models.ts` (`WorkflowDefinition`)
  - `src/config/contracts.ts` (typed config provider seam)
  - `src/bootstrap/service.ts` / `src/bootstrap/main-entry.ts` (startup wiring shape)

## Assumptions

1. This ticket delivers deterministic loader/parser contract and tests, not full dynamic file watching/reload.
2. Error typing should be explicit enough for KAT-225/KAT-234 to gate startup/dispatch behavior without string parsing.
3. Path selection must support future CLI wiring while staying testable in isolation.

## Options Considered

1. Explicit line-oriented parser with typed error mapping (selected)
   - Read file as UTF-8, detect front matter with delimiter-aware line scan, parse YAML into object map, trim body.
   - Pros: deterministic behavior, easiest to assert exact failure modes, minimal ambiguity.
   - Cons: slightly more code than a convenience parser.

2. Regex-split parser around `---` delimiters
   - Single-pass regex to split front matter/body and feed YAML parser.
   - Pros: compact implementation.
   - Cons: brittle with edge cases (missing closing delimiter, delimiter in body), harder to produce deterministic parse errors.

3. Full Markdown parser with front matter plugin
   - Parse markdown AST and front matter through third-party pipeline.
   - Pros: future-proof for broader markdown processing.
   - Cons: unnecessary complexity/dependencies for this contract-only ticket.

## Selected Approach

Use a **line-oriented loader/parser with explicit typed domain errors** and injectable filesystem/cwd seams for deterministic tests.

## Proposed Contract Surface

### Module layout

1. `src/workflow/contracts.ts`
   - Loader input options and typed error/result contracts.
2. `src/workflow/errors.ts`
   - Error constructors/classes for:
     - `missing_workflow_file`
     - `workflow_parse_error`
     - `workflow_front_matter_not_a_map`
3. `src/workflow/loader.ts`
   - Path resolution + file read + parse split.
4. `src/workflow/index.ts`
   - Re-export public loader contract.

### Primary API shape

`loadWorkflowDefinition(options?) -> WorkflowDefinition`

Input options (exact naming can be finalized during implementation):

- `workflowPath?: string` (explicit runtime/CLI path)
- `cwd?: string` (default `process.cwd()`; injectable for tests)
- `readFile?: (path: string) => Promise<string>` (injectable seam for tests)

## Resolution and Parsing Rules

### 1. Path precedence (SPEC 5.1)

1. If explicit `workflowPath` is provided and non-empty, use it.
2. Otherwise use `<cwd>/WORKFLOW.md`.
3. Relative explicit paths resolve against `cwd`; absolute paths are preserved.

### 2. File read behavior (SPEC 5.1, 5.5)

1. Loader attempts UTF-8 read of resolved path.
2. Any read failure maps to typed `missing_workflow_file` with resolved path context.

### 3. Front matter detection and split (SPEC 5.2)

1. If content does **not** start with `---` on the first line:
   - `config = {}`
   - `prompt_template = full_file.trim()`
2. If content starts with `---`:
   - Scan forward for next standalone `---` delimiter line.
   - Missing closing delimiter => `workflow_parse_error`.
   - Text between delimiters is YAML front matter raw payload.
   - Remaining text after closing delimiter is prompt body.

### 4. YAML decode rules (SPEC 5.2, 5.5)

1. YAML decode failure => `workflow_parse_error`.
2. Decoded value must be a plain object/map.
3. Non-map decoded root (array/scalar/null/etc.) => `workflow_front_matter_not_a_map`.
4. On success: `config` is the decoded root object directly (not nested).

### 5. Prompt body contract (SPEC 5.2)

1. Prompt body is always `trim()`ed before storing as `prompt_template`.
2. Empty body is valid and returns `prompt_template: ""` (fallback prompt decisions remain outside this ticket).

## Deterministic Error Model

All loader failures should be discriminated by a stable `code` field and include `workflowPath`.

1. `missing_workflow_file`
   - Trigger: file cannot be read at resolved path.
2. `workflow_parse_error`
   - Trigger: malformed front matter structure (including missing closing delimiter) or YAML decode failure.
3. `workflow_front_matter_not_a_map`
   - Trigger: YAML parses but root type is not object/map.

## Test Strategy

Create focused unit tests under `tests/workflow/` covering:

1. Path resolution and precedence:
   - explicit path beats cwd default
   - default path is `<cwd>/WORKFLOW.md`
   - relative explicit path resolves against cwd
2. Read failure mapping:
   - explicit and default path missing => `missing_workflow_file`
3. Parsing behaviors:
   - no front matter => empty config + trimmed full body
   - valid map front matter + body split
   - invalid YAML => `workflow_parse_error`
   - missing closing `---` => `workflow_parse_error`
   - front matter YAML root as list/scalar/null => `workflow_front_matter_not_a_map`
4. Output invariants:
   - body trimming is deterministic
   - front matter root is returned as `config` directly

## Integration Notes for Downstream Tickets

1. KAT-223 can consume loader output as the single source for raw config map input.
2. KAT-225 preflight validation can branch directly on loader error codes for startup/tick gating.
3. KAT-234 CLI lifecycle can pass optional positional path into this loader and inherit deterministic error behavior.

## Scope Boundaries

### In scope

- Path precedence and default path selection
- YAML front matter + body split
- Typed loader error contract
- Deterministic unit tests for acceptance criteria

### Out of scope

- Dynamic watch/reload semantics
- Full typed config coercion/defaulting (`KAT-223`)
- Prompt template strict rendering (`KAT-224`)
- Startup/tick dispatch gating integration (`KAT-225`)
- CLI command-line parsing and host lifecycle behavior (`KAT-234`)

## Open Questions (Defaulted)

1. Should empty front matter (`---` then `---`) be treated as `{}` or non-map?
   - Default in this design: treat non-object decode as `workflow_front_matter_not_a_map` for strictness.
2. Should non-ENOENT read failures (permission/I/O) map to parse or missing?
   - Default in this design: map all read failures to `missing_workflow_file` to keep the external error surface stable per spec language.

## Handoff

This design defines the exact discovery/parsing contract needed to implement KAT-222 with deterministic behavior and unlock KAT-225 and KAT-234 integration work.
