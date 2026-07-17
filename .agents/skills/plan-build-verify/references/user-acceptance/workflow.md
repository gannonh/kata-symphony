# User Acceptance Evidence

## Operating brief

When this workflow runs, gather real evidence that the feature branch, ticket, or PR works. Treat the request as: "prove the completed work by exercising it and producing artifacts a human can inspect."

Acceptance evidence should be experiential and reproducible:

- For UI changes, capture video when practical, then screenshots at meaningful checkpoints. Video captures timing, transitions, focus, and end-to-end flow.
- For CLI and TUI apps, capture terminal transcripts, screenshots, recordings, generated files, and exit codes.
- For APIs and SDKs, capture requests, responses, output files, logs, and small runnable examples.
- For native apps, including Electron-type apps, strongly prefer `/computer-use` when available to drive the real app window and validate acceptance criteria through normal user-visible behavior.

Automated tests can supplement UAT, but they do not replace an in-app walkthrough, command run, API call, or SDK example.

## Supported targets

Use this workflow for:

1. Web apps
2. CLI and TUI apps
3. APIs and SDKs
4. Native apps, including Electron-type apps

If the requested target is outside this list, ask whether to proceed with a best-effort evidence plan.

## Hard gates

1. **Scope lock first**
   - Start with `UAT Scope: <ticket/PR/branch scope only>`.
   - If the scope is unclear, inspect the branch diff, PR, ticket, or ask the user.

2. **Evidence before recommendation**
   - Do not recommend merge based only on summaries, static code review, or green tests.
   - Exercise the changed behavior and save artifacts.

3. **Video-first visible evidence for UI changes**
   - For web, TUI, and native app work, capture video when practical, especially when the change affects interaction, layout, transitions, state changes, or error handling.
   - Also capture screenshots at meaningful checkpoints so reviewers can inspect static states.
   - If video is not practical, state why it was skipped.

4. **Manual run instructions are mandatory**
   - Always include steps the human can run themselves.
   - For UI work, primary instructions must use normal product-user actions.
   - For CLI, TUI, API, or SDK work, include copy-pasteable commands or code snippets with expected outcomes.

5. **No GO verdict without explicit human acceptance**
   - Before user confirmation, use `Recommendation: Pending user sign-off`.
   - Use `GO` or `GO with follow-ups` only after the user explicitly accepts.

6. **Do not claim acceptance completion unilaterally**
   - Evidence collection supports acceptance.
   - The human grants acceptance.

7. **Adversarial evidence review**
   - When subagents are available, ask at least one adversarial reviewer to compare the gathered evidence against the spec, acceptance criteria, requirements, PR description, or ticket.
   - The reviewer must provide `Pass` or `Fail` for each criterion and cite the artifact path or log line that supports the judgment.
   - If subagents are unavailable, perform the same review inline and label it `Inline adversarial review`.

## Tool selection

Load and use the best available skill or CLI for the target. If a required skill or CLI is unavailable, install it with `npx agents install <skill-name>`, then follow that skill's installation instructions for its underlying CLI.

| Target | Preferred tools | Evidence to capture |
| --- | --- | --- |
| Web app | `agent-browser`; `playwright` when the repo already uses Playwright or traces are useful; `chrome-cdp` only for an already-open Chrome page with user approval | Video, screenshots, DOM/accessibility snapshots, console/network notes |
| CLI app | Shell commands, `script`, generated output files; `/computer-use` skill (if available) for terminal video/GIF when visual proof helps | Terminal transcript, exit codes, output files, JSON, screenshots/video for interactive flows |
| TUI app | `/computer-use` skill (if available) for terminal recording or screenshots; terminal transcript where possible | Video/GIF, screenshots, transcript, config/output files |
| API | `curl`, HTTP client, repo scripts, logs | Request/response JSON, status codes, logs, saved payloads |
| SDK | Minimal runnable example in the target language, repo examples/tests only as supplement | Source snippet, command output, generated files, logs |
| Native app, including Electron-type | `/computer-use` skill if available for app-window driving, interaction, accessibility inspection, screenshots, and recording; `agent_browser` Electron/CDP automation when the app exposes useful browser automation or CDP proof is needed | Window video, screenshots, accessibility snapshots, logs |

Installation checks:

```bash

# Then verify the underlying CLI required by the chosen skill
command -v agent-browser || true
```

Do not install tools that are unrelated to the target.

## Evidence workspace

Use the bundled helpers when practical. Resolve script paths relative to the `plan-build-verify` skill directory (`scripts/user-acceptance/`).

```bash
# Create the evidence directory and starter manifest.
node scripts/user-acceptance/init-evidence.mjs --target electron --scope "validated behavior"

# Run commands while saving logs, exit codes, and manifest entries.
node scripts/user-acceptance/run-capture-command.mjs --evidence <dir> --name focused-vitest -- pnpm test

# Capture a CDP-accessible web/Electron page when agent-browser screenshots fail.
node scripts/user-acceptance/cdp-capture-page.mjs --evidence <dir> --cdp http://127.0.0.1:9222 --title "Kata Desktop" --screenshot screenshots/start.png --text logs/start-text.txt

# Generate and check final artifacts before responding.
node scripts/user-acceptance/write-report.mjs --evidence <dir>
node scripts/user-acceptance/verify-evidence.mjs --evidence <dir>
```

The helpers do not replace judgment. They prevent common evidence mistakes: missing exit codes, empty reports, invalid `evidence.json`, absent UI screenshots/video, and command output that was never saved.

Write generated evidence under a repo-local folder:

```text
uat-evidence/<target>-<YYYYMMDD-HHMMSS>/
```

Create artifacts as appropriate:

```text
evidence.json          # machine-readable manifest
evidence.md            # concise human report
screenshots/           # PNG/JPG checkpoints
recordings/            # MP4/GIF terminal or UI recordings
responses/             # JSON/API/SDK outputs
logs/                  # command logs, server logs, console excerpts
outputs/               # generated files from the feature
```

Keep evidence paths stable and report them in the final answer. If `uat-evidence/` is not ignored by git, mention that it should be ignored before committing unless the user explicitly wants artifacts committed.

A minimal `evidence.json` should include:

```json
{
  "scope": "validated behavior",
  "target": "web|cli|tui|api|sdk|native|electron",
  "timestamp": "ISO-8601",
  "git_commit": "short sha",
  "artifacts": [
    {"type": "video|screenshot|json|log|output", "path": "relative/path", "description": "what it proves"}
  ],
  "commands": [
    {"command": "command run", "exit_code": 0, "output_path": "logs/example.log"}
  ],
  "slices": [
    {"name": "slice name", "result": "Pass|Fail", "evidence": ["relative/path"]}
  ]
}
```

## Workflow

### 1. Identify scope, target, and acceptance slices

- Inspect branch diff, PR description, ticket, README, package scripts, and app entry points as needed.
- Declare `UAT Scope: ...`.
- Declare `Target: web | cli | tui | api | sdk | native | electron | mixed`.
- Break the scope into 2 to 5 acceptance slices.
- Define visible pass/fail criteria for each slice.

### 2. Prepare the app or service

- Start the dev server, service, CLI, TUI, API, SDK fixture, or native/Electron-type app needed for the walkthrough.
- Prefer real local behavior over mocks.
- For web and native/Electron-type targets, check ports and existing processes before launch. Record any cleanup in `logs/`.
- Use the user's normal product command for the product smoke path. If you need CDP, traces, or deterministic screenshots, run a second instrumented path and label it as such.
- Pick free debug ports rather than assuming `9222`; if a port is occupied, save the listener evidence and choose another port.
- If credentials, services, hardware, or permissions block proof, record the blocker and provide the closest reproducible plan. Do not mark blocked slices as passed.

### 3. Execute the feature path

Use the matching playbook (paths relative to `references/user-acceptance/`):

- Web app UI flows: `web-playbook.md`
- Native/Electron-type UI flows: `native-electron-playbook.md`
- CLI, TUI, API, and SDK: `cli-api-sdk-playbook.md`

For native/Electron-type app-window validation, strongly prefer `/computer-use` if available so the evidence reflects the real desktop surface a user sees.

For mixed work, run the user-facing path first, then the technical proof tied to the same outcome.

### 4. Capture durable evidence

- Save artifacts under `uat-evidence/<target>-<timestamp>/`.
- Capture video for web, native/Electron-type, and TUI when practical.
- Capture screenshots at the start, key state changes, and final success state.
- Capture command output with `tee`, `script`, or `scripts/user-acceptance/run-capture-command.mjs`; always save exit codes.
- Save API/SDK JSON responses and generated output files.
- For bugfix UAT, capture negative evidence that the old failure is absent: search logs for the old error string, search active source/docs for removed flags or stale commands, and save zero-match outputs.
- Classify failed checks. If a failing file or behavior changed in the branch, treat the slice as failed. If it is outside the branch diff, report it as an unrelated validation failure with file/error evidence.
- Write `evidence.json` and `evidence.md`, then run `scripts/user-acceptance/verify-evidence.mjs` before responding.

### 5. Run adversarial evidence review

- Provide the spec, acceptance criteria, requirements, PR description, or ticket plus the saved artifacts to an adversarial review subagent when available.
- Ask the reviewer to decide `Pass` or `Fail` for each criterion and cite exact evidence paths.
- Treat missing, ambiguous, or inaccessible artifacts as failures for the affected criteria.
- If a reviewer flags a gap, collect more evidence or mark the slice failed before reporting.

### 6. Report results

Use this order:

1. `UAT Scope: ...`
2. `Target: ...`
3. `Slice-by-slice result`
4. `Evidence`
5. `Adversarial Review`
6. `Manual Run Instructions`
7. `Recommendation: Pending user sign-off`
8. `Please reply: accept / reject`

Keep the report concise. Link to artifact paths and explain what each artifact proves.

## Output contract

For every UAT response, include:

```markdown
UAT Scope: <scope>
Target: <web|cli|tui|api|sdk|native|electron|mixed>

Slice-by-slice result:
- Pass/Fail: <slice> - <one-line evidence summary>

Evidence:
- <artifact path> - <what it proves>

Adversarial Review:
- Pass/Fail: <criterion> - <artifact-backed reason>

Manual Run Instructions:
1. <human step or command>
   Expected: <visible result or output>

Recommendation: Pending user sign-off
Please reply: accept / reject
```

For UI targets, manual instructions must start with normal user actions in the running app. Put automation commands or test harnesses only in a clearly labeled `Fallback (Engineering Only)` section.

For CLI, TUI, API, and SDK targets, manual instructions may be commands or code snippets, but they must include expected output and any required environment variables.

## Ticket update guidance

If a ticket or PR is known, offer or perform a status update with:

- Scope validated
- Target and tools used
- Slice pass/fail summary
- Evidence paths or links
- Manual run instructions
- Current recommendation

Before explicit user acceptance, label the update `Pending user sign-off`. After explicit acceptance, record the final verdict if the project workflow requires it.

## Pre-response self-check

Before responding, verify:

- Scope is stated.
- Target is stated.
- Each slice has pass/fail status.
- Evidence artifacts exist or blockers are clearly labeled.
- `evidence.json` is valid and `evidence.md` is non-empty.
- UI evidence includes video plus screenshots when practical, or a stated reason video was skipped.
- UI screenshots/video files exist and are readable.
- Native app validation used `/computer-use` when available, or explains why another tool was used.
- Adversarial review checked every criterion against evidence and produced pass/fail judgments.
- Command logs include exit codes.
- Old-bug negative evidence is saved when validating a fix.
- Dev servers, native/Electron-type apps, or background processes launched for UAT are cleaned up or explicitly left running for the user.
- Manual run instructions are included.
- Recommendation remains `Pending user sign-off` unless the user has accepted.

## Common mistakes

- Running only tests and calling it UAT.
- Reporting a code summary without artifacts.
- Skipping video or screenshots for UI work without explanation.
- Providing Playwright or test commands as the primary manual UI path.
- Forgetting TUI visual evidence.
- Omitting API request and response files.
- Declaring `GO` before the user accepts.
- Installing every tool instead of selecting the best tool for the target.
