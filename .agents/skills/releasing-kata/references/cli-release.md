# CLI Release

Package: `@kata-sh/cli`  
Version source at publish time: workflow-aligned `apps/cli/package.json`  
Changelog: `apps/cli/CHANGELOG.md`  
Tag format: `cli-vX.Y.Z` or `cli-vX.Y.Z-<prerelease>`  
CI workflow: `cli-release.yml` (manual `workflow_dispatch` only)

CLI is **not** coupled to Symphony and has a **single manual release channel** (no nightly).

## Version / dist-tags

- Omit `version` → start from `apps/cli/package.json`, then auto-bump patch until `cli-v*` is free
- Pass `version` → override (e.g. `0.18.0` or `0.18.0-alpha.0`); fails early if that tag already exists
- Plain semver (`0.18.0`) → npm `latest`
- Prerelease (`0.18.0-alpha.0`) → npm dist-tag from the prerelease id (`alpha`)

## Steps

1. Land the code on `main` (no version-bump PR required).
2. Optional: add `## X.Y.Z` to `apps/cli/CHANGELOG.md`.
3. Update docs when behavior changes (`apps/cli/README.md`, `AGENTS.md`, preferences docs as needed).
4. Dispatch:

   ```bash
   # Auto-bump to next free patch from package.json
   gh workflow run cli-release.yml

   # Override version
   gh workflow run cli-release.yml -f version=0.18.0

   # Prerelease
   gh workflow run cli-release.yml -f version=0.18.0-alpha.0

   # Dry run
   gh workflow run cli-release.yml -f version=0.18.0 -f dry_run=true
   ```

5. Verify:

   ```bash
   gh run list --workflow=cli-release.yml --limit 5
   gh release view cli-vX.Y.Z
   npm view @kata-sh/cli version
   npm view @kata-sh/cli dist-tags
   ```

## What CI does

1. **preflight**: resolve version (input, or auto-bump from `apps/cli/package.json`); align package.json; tsc, tests, golden-path, build.
2. **publish**: build, golden-path gate, `npm publish`, create `cli-vX.Y.Z` tag + GitHub Release.
3. **finalize** (non-prerelease only): commit `apps/cli/package.json` version on `main`.

## Acceptance criteria

- [ ] Published to npm under the expected dist-tag
- [ ] Git tag `cli-vX.Y.Z` created
- [ ] GitHub Release created
- [ ] For non-prerelease: main package.json matches after finalize
