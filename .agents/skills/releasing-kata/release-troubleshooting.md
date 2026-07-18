# Release Troubleshooting

## Target sanity check

| Target | Workflow | Tags |
| --- | --- | --- |
| Symphony + Pi extension | `symphony-release.yml` | `symphony-v*`, `pi-symphony-v*` |
| CLI | `cli-release.yml` | `cli-v*` |

Releases no longer trigger from push-to-main version bumps. If nothing runs after merge, dispatch the workflow.

## Common failures

### Stable version resolution failed

```
No version input and no nightly tag to derive the stable version from.
```

**Fix (Symphony):** Pass `-f version=X.Y.Z`, or cut a Symphony nightly first.  
**Fix (CLI):** Omit `version` to use `apps/cli/package.json`, or pass `-f version=X.Y.Z` to override. CLI has no nightly channel.

### Tag already exists

The publish job refuses to overwrite an existing tag. Bump the version input or wait for a new nightly run number.

### npm publish failed

1. Confirm `NPM_TOKEN` repository secret.
2. Confirm package is not `private: true`.
3. Confirm the version is new on the registry:

```bash
npm view @kata-sh/cli versions --json
npm view @kata-sh/pi-symphony-extension versions --json
```

### Symphony binary build failed

```bash
cd apps/symphony
cargo test
cargo clippy -- -D warnings
cargo build --release
```

### Pi extension validation failed

```bash
pnpm --dir apps/symphony/pi-extension run lint
pnpm --dir apps/symphony/pi-extension run typecheck
pnpm --dir apps/symphony/pi-extension run test
(cd apps/symphony/pi-extension && npm pack --dry-run)
```

### Scheduled nightly skipped

Expected when `main` HEAD equals the commit of the latest `symphony-v*-nightly.*` tag. Force with:

```bash
gh workflow run symphony-release.yml -f channel=nightly
```

### Finalize commit did not land

Branch protection may block `github-actions[bot]` pushes. Either allow the bot to push to `main`, or manually commit the version files from the release run workspace / re-run with an explicit follow-up PR.

## Visibility

```bash
gh run list --workflow=cli-release.yml --limit 5
gh run list --workflow=symphony-release.yml --limit 5
gh run view <run-id>
gh run watch
gh release list
```

## Local version helpers

```bash
node --experimental-strip-types scripts/release/resolve-nightly-release.ts \
  --date 20260718 --run-number 1 --sha "$(git rev-parse HEAD)" --product symphony

node --experimental-strip-types scripts/release/resolve-stable-version.ts \
  --product symphony --version 2.4.0

node --experimental-strip-types scripts/release/resolve-stable-version.ts \
  --product cli --version 0.18.0
```
