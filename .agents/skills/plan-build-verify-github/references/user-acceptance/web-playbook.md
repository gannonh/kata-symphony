# Web Evidence Playbook

Use the actual web app and capture what a human would see. Tests can supplement this walkthrough after evidence is collected.

## Tool choice

- Prefer `agent-browser` for web apps and browser automation. Verify the installed CLI with `agent-browser --help` because command sets vary by version.
- Use `playwright` when the repo already uses Playwright, when traces are useful, or when `agent-browser` cannot complete the flow.
- Use `chrome-cdp` only when the user explicitly approved inspection of an already-open Chrome page.

If the needed skill or CLI is missing, install it with `npx agents install <skill-name>`, then follow that skill's installation instructions. Do not rely on `agent-browser skills get ...` unless the installed CLI confirms that command exists.

## Evidence priority

1. Video of the feature flow, when practical, especially for UI changes involving interaction, layout, transitions, state changes, or error handling.
2. Screenshots at start, key interaction, and final result.
3. Accessibility, DOM, or body-text snapshots that confirm the state.
4. Console, network, or app logs when they explain behavior.
5. Negative evidence for bugfixes: saved searches showing the old error, flag, or stale wording is absent from logs and active source/docs.

If video is skipped, record the reason in `evidence.md`. If a preferred screenshot tool fails, use the next fallback and save a note in `logs/`.

## Web app workflow

1. Start the app with the repo's documented dev command.
2. Open the app in a real browser.
3. Navigate to the feature through the normal user path.
4. Capture a starting screenshot.
5. Perform the user actions: click, fill, select, scroll, upload, download, or navigate.
6. Capture video when practical, then screenshots through the meaningful checkpoints.
7. Confirm the final visible state with a fresh snapshot or screenshot.
8. Save logs or generated files that prove the result.

Example shape:

```bash
npm run dev 2>&1 | tee uat-evidence/web-<timestamp>/logs/dev-server.log
agent-browser open http://localhost:3000
agent-browser snapshot -i > uat-evidence/web-<timestamp>/logs/start-snapshot.txt
agent-browser screenshot uat-evidence/web-<timestamp>/screenshots/01-start.png
# interact with the feature using current element refs
agent-browser screenshot uat-evidence/web-<timestamp>/screenshots/02-result.png
agent-browser snapshot -i > uat-evidence/web-<timestamp>/logs/result-snapshot.txt
```

## Adversarial evidence review

After collecting artifacts, ask an adversarial review subagent to compare the evidence against the spec, acceptance criteria, requirements, PR description, or ticket when subagents are available. The review must return `Pass` or `Fail` for each criterion and cite exact artifact paths or log lines. Treat missing video, missing screenshots, inaccessible files, or ambiguous proof as a failed criterion until corrected or explicitly documented as blocked.

If subagents are unavailable, perform the same check inline and label it `Inline adversarial review`.

## Manual instructions

Write normal user steps first:

```markdown
Manual Run Instructions:
1. Start the app with `<command>`.
   Expected: the app opens at `<url>`.
2. Go to `<screen>`.
   Expected: `<visible state>`.
3. Click `<control>` and enter `<sample input>`.
   Expected: `<result>`.
```

Keep automation commands, seeded data, and test harnesses out of the primary UI path. If unavoidable, place them under `Fallback (Engineering Only)` and explain why.
