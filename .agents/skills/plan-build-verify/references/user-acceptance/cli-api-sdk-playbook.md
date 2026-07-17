# CLI, TUI, API, and SDK Evidence Playbook

Use the changed interface directly and save outputs a human can inspect or rerun.

## Evidence priority

CLI:

1. Terminal transcript with exact command and exit code.
2. Generated output files.
3. JSON or structured output.
4. Screenshot or recording when visual formatting matters.

TUI:

1. Video or GIF of the terminal flow.
2. Screenshots of key screens.
3. Transcript, config, and output files where available.

API:

1. Request payload.
2. Response body and status code.
3. Logs showing the service handled the request.
4. Follow-up read that confirms persisted state.

SDK:

1. Minimal runnable example.
2. Command output.
3. Generated files or returned objects.
4. Logs when they clarify behavior.

## CLI workflow

1. Build or install the CLI as the repo documents.
2. Run the command that exercises the changed behavior.
3. Save stdout, stderr, and exit code.
4. Save any generated files under `outputs/`.
5. Run one trust-check command that confirms the output.
6. For bugfixes, save a negative check that the old error or stale output is absent.

Prefer the bundled runner when practical because it records logs and exit codes in the manifest:

```bash
node scripts/user-acceptance/init-evidence.mjs --target cli --scope "tasker JSON export"
node scripts/user-acceptance/run-capture-command.mjs --evidence <dir> --name export-json -- \
  tasker export --format json --output <dir>/outputs/tasks.json
node scripts/user-acceptance/run-capture-command.mjs --evidence <dir> --name validate-json -- \
  jq . <dir>/outputs/tasks.json
```

Manual fallback shape:

```bash
mkdir -p uat-evidence/cli-<timestamp>/logs uat-evidence/cli-<timestamp>/outputs
set -o pipefail
mytool process --input fixtures/sample.csv --format json \
  2>&1 | tee uat-evidence/cli-<timestamp>/logs/process.log
status=${PIPESTATUS[0]}
echo "$status" > uat-evidence/cli-<timestamp>/logs/process.exitcode
jq . uat-evidence/cli-<timestamp>/outputs/result.json > uat-evidence/cli-<timestamp>/logs/result-json.txt
```

## TUI workflow

1. Start the TUI with a deterministic fixture or normal local state.
2. Exercise the changed keyboard or mouse flow.
3. Capture the final state and any written files.
4. Record exact keystrokes in `evidence.md` so the user can replay them.

Use `script`, VHS, or `/computer-use` skill (if available) depending on what the app supports. Prefer video/GIF when the visual layout is part of the acceptance criteria.

## API workflow

1. Start the service locally or identify the target environment.
2. Save request payloads under `responses/` or `payloads/`.
3. Execute the request with `curl`, repo scripts, or the documented client.
4. Save response body and status code.
5. Run a follow-up GET/list/read when persistence or side effects matter.
6. Save relevant logs.
7. Record blocked credentials or unavailable services as blocked slices rather than passing them from tests alone.

Example shape:

```bash
mkdir -p uat-evidence/api-<timestamp>/responses uat-evidence/api-<timestamp>/logs
cat > uat-evidence/api-<timestamp>/responses/create-request.json <<'JSON'
{"name":"UAT sample"}
JSON
curl -sS -X POST http://localhost:8080/api/items \
  -H 'Content-Type: application/json' \
  -d @uat-evidence/api-<timestamp>/responses/create-request.json \
  -w '\nHTTP_STATUS:%{http_code}\n' \
  | tee uat-evidence/api-<timestamp>/responses/create-response.txt
```

## SDK workflow

1. Create a minimal example file in a temporary or evidence directory.
2. Import the local SDK build or package path.
3. Call the changed API with representative inputs.
4. Print structured output.
5. Save command output and any generated files.
6. Keep the example small and runnable so the human can copy it into their environment.

Example shape:

```bash
mkdir -p uat-evidence/sdk-<timestamp>/examples uat-evidence/sdk-<timestamp>/logs
cat > uat-evidence/sdk-<timestamp>/examples/uat-example.mjs <<'JS'
import { client } from '../../dist/index.js';
const result = await client.doThing({ name: 'UAT sample' });
console.log(JSON.stringify(result, null, 2));
JS
node uat-evidence/sdk-<timestamp>/examples/uat-example.mjs \
  2>&1 | tee uat-evidence/sdk-<timestamp>/logs/sdk-example.log
```

## Manual instructions

For CLI, API, and SDK targets, include copy-pasteable reproduction steps:

```markdown
Manual Run Instructions:
1. Build the project: `<command>`.
   Expected: build exits 0.
2. Run: `<command>`.
   Expected: output contains `<specific result>`.
3. Inspect: `<file or follow-up command>`.
   Expected: `<specific data>`.
```

For TUI targets, include launch command, keystrokes, visible screen names, and expected final state.
