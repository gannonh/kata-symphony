# Symphony + Pi Extension Release

Coupled release of:

| Artifact | Source | Tag |
| --- | --- | --- |
| Symphony binary | `apps/symphony` (Rust) | `symphony-vX.Y.Z` |
| Pi Symphony extension | `apps/symphony/pi-extension` | `pi-symphony-vX.Y.Z` |

Both always share the same version string. CI workflow: `symphony-release.yml`.

Changelog (stable notes): `apps/symphony/CHANGELOG.md` (used when a `## X.Y.Z` section exists).

## Triggers

- **Scheduled nightly:** `cron: 0 */3 * * *` — only proceeds when `main` HEAD differs from the latest `symphony-v*-nightly.*` tag.
- **Manual:** Actions → Symphony Release → Run workflow
  - `channel`: `stable` | `nightly`
  - `version`: optional for stable (defaults to latest nightly core)
  - `dry_run`: build/test only; no tags, releases, or npm publish

There is **no** push-to-main path filter release anymore.

## Stable release steps

1. Prefer cutting at least one successful nightly first so stable can omit `version`.
2. Optional: update `apps/symphony/CHANGELOG.md` with a `## X.Y.Z` section on main (or accept auto-generated notes).
3. Dispatch:

   ```bash
   # Derive version from latest symphony nightly core
   gh workflow run symphony-release.yml -f channel=stable

   # Or pin the version explicitly
   gh workflow run symphony-release.yml -f channel=stable -f version=2.4.0

   # Dry run
   gh workflow run symphony-release.yml -f channel=stable -f version=2.4.0 -f dry_run=true
   ```

4. Watch and verify:

   ```bash
   gh run list --workflow=symphony-release.yml --limit 5
   gh release view symphony-vX.Y.Z
   gh release view pi-symphony-vX.Y.Z
   npm view @kata-sh/pi-symphony-extension version
   npm view @kata-sh/pi-symphony-extension dist-tags
   ```

## Nightly release steps

```bash
# Manual nightly
gh workflow run symphony-release.yml -f channel=nightly

# Dry run nightly
gh workflow run symphony-release.yml -f channel=nightly -f dry_run=true
```

Scheduled nightlies run automatically every 3 hours when main has moved.

## What CI does

1. **check_changes** (schedule only): skip if no commits since last `symphony-v*-nightly.*`.
2. **preflight**: resolve version; write the same version into `Cargo.toml` and `pi-extension/package.json` in the runner workspace; run cargo test/clippy and pi-extension lint/typecheck/test/pack.
3. **build**: multi-OS release binaries with aligned version.
4. **publish_extension**: `npm publish` `@kata-sh/pi-symphony-extension` with dist-tag `latest` / prerelease id / `nightly`.
5. **release**: create dual tags + dual GitHub Releases (binaries on the Symphony release).
6. **finalize** (stable non-prerelease only): commit version bump on `main` for Cargo.toml + pi-extension package.json.

## Install forms (Pi extension)

```bash
pi install npm:@kata-sh/pi-symphony-extension
pi install npm:@kata-sh/pi-symphony-extension@X.Y.Z
pi install git:github.com/gannonh/kata
pi install git:github.com/gannonh/kata@pi-symphony-vX.Y.Z
pi -e ./apps/symphony/pi-extension
```

Root `package.json` stays `0.0.0` and exposes the extension via its `pi` manifest for monorepo git installs.

## Acceptance criteria

- [ ] Tags `symphony-vX.Y.Z` and `pi-symphony-vX.Y.Z` exist on the same commit
- [ ] GitHub Release `Symphony vX.Y.Z` has linux/mac/windows binaries
- [ ] GitHub Release `Pi Symphony Extension vX.Y.Z` exists
- [ ] npm has `@kata-sh/pi-symphony-extension@X.Y.Z` on the expected dist-tag
- [ ] For stable: main has matching versions in `Cargo.toml` and pi-extension `package.json` after finalize
