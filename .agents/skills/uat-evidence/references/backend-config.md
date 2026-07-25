# Backend Config

## Shared

Run from the Kata workspace root when possible. The runners load `.env` from the workspace. Default evidence output goes to:

```text
<workspace>/uat-evidence/<runtime>-<backend>-<timestamp>-<pid>/
```

Useful options:

```bash
--workspace /path/to/repo
--output-dir /path/to/custom/output
--dry-run
```

## Kata CLI Runtime

Use `--runtime kata-cli`.

Additional options:

```bash
--cli-root /path/to/repo/apps/cli
```

The runner sets `KATA_CLI_ROOT` to `apps/cli` when it exists.

### GitHub

The runner reads `.kata/preferences.md` when present. It needs:

- `workflow.mode: github`
- `github.repoOwner`
- `github.repoName`
- `github.stateMode: projects_v2`
- `github.githubProjectNumber`
- `GH_TOKEN` or `GITHUB_TOKEN`

Override values:

```bash
node <skill-directory>/scripts/uat-evidence.mjs test --runtime kata-cli --backend github \
  --github-owner gannonh \
  --github-repo kata \
  --github-project-number 17
```

### Linear

The runner reads `.env` and accepts overrides. It needs:

- `LINEAR_API_KEY` or `LINEAR_TOKEN`
- Linear workspace key/name
- Linear team key
- Linear project ID or slug

Defaults for this repo are:

- workspace: `kata-sh`
- team: `KAT`
- project: `LINEAR_PROJECT_ID` from `.env`

Override values:

```bash
node <skill-directory>/scripts/uat-evidence.mjs test --runtime kata-cli --backend linear \
  --linear-workspace kata-sh \
  --linear-team KAT \
  --linear-project "$LINEAR_PROJECT_ID"
```

When Linear has multiple active milestones, the runner pins the created milestone in temporary preferences before calling active-milestone operations.

## Symphony Runtime

Use `--runtime symphony-runtime`.

Additional options:

```bash
--symphony-root /path/to/repo/apps/symphony
--binary /path/to/symphony
```

The runner uses `.symphony/WORKFLOW.md` where available and loads `.env` from the workspace and `apps/symphony/.env`.

### GitHub

The runner needs:

- `GH_TOKEN` or `GITHUB_TOKEN`
- repository owner/name
- GitHub Projects v2 config supported by Symphony `WORKFLOW.md`

Override values:

```bash
node <skill-directory>/scripts/uat-evidence.mjs test --runtime symphony-runtime --backend github \
  --github-owner gannonh \
  --github-repo kata \
  --github-project-number 17
```

Symphony evidence stores these effective coordinates in sanitized form. Later
cleanup normally needs only `--evidence`; explicit repository overrides are
required only for ambiguous legacy evidence and must agree with every created
issue URL.

### Linear

The runner needs:

- `LINEAR_API_KEY` or `LINEAR_TOKEN`
- `tracker.project_slug`
- `tracker.workspace_slug`

Override values:

```bash
node <skill-directory>/scripts/uat-evidence.mjs test --runtime symphony-runtime --backend linear \
  --linear-project-slug kata \
  --linear-workspace-slug kata-sh
```
