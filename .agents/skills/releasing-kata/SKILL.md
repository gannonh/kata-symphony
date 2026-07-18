# Releasing Kata

Releases are **manually dispatched** (plus scheduled Symphony nightlies). Do not bump versions in a PR to trigger a release.

| Target | Package / artifact | Tag format | Workflow | Coupling |
| --- | --- | --- | --- | --- |
| **Symphony + Pi extension** | `symphony` binary + `@kata-sh/pi-symphony-extension` | `symphony-vX.Y.Z` and `pi-symphony-vX.Y.Z` (same `X.Y.Z`) | `symphony-release.yml` | Always released together |
| **CLI** | `@kata-sh/cli` | `cli-vX.Y.Z` | `cli-release.yml` | Independent |

Root `package.json` version is `0.0.0` — never touch it as a release version. Nightly and stable version strings are applied by the workflow at publish time; stable finalize commits the version back to `main`.

## Channels

| Channel | How | Version |
| --- | --- | --- |
| **stable** | `workflow_dispatch` with `channel=stable` (Symphony) or CLI dispatch | Explicit `version` input, or derive from latest matching `*-nightly.*` tag core |
| **nightly** | Schedule every 3h on main (Symphony only, skips if HEAD unchanged since last nightly) or manual `channel=nightly` | `{nextPatch}-nightly.YYYYMMDD.{run_number}` from current `Cargo.toml` patch+1 |

CLI has **no nightly**. CLI releases are manual only. Omit `version` to use `apps/cli/package.json` (or a future `cli-v*-nightly.*` core); pass `version` to override.

## Version semantics

| Type | When | Example |
| --- | --- | --- |
| `patch` | Bug fixes, small improvements | 2.3.0 → 2.3.1 |
| `minor` | New features, backward compatible | 2.3.0 → 2.4.0 |
| `major` | Breaking changes | 2.3.0 → 3.0.0 |
| prerelease | Validation / alpha / beta / rc | `0.18.0-alpha.0` → npm tag `alpha` |
| nightly | Continuous integration builds | `2.3.1-nightly.20260718.42` → npm tag `nightly` |

## Workflow

1. Identify the target (Symphony+Pi vs CLI).
2. Read the matching reference:
   - `references/symphony-release.md`
   - `references/cli-release.md`
3. Dispatch the workflow (optional `dry_run=true` first).
4. Verify tags, GitHub Releases, and npm as documented.

## Helpers

Release math lives under `scripts/release/`:

- `resolve-nightly-release.ts`
- `resolve-stable-version.ts`
- `resolve-previous-release-tag.ts`
- `update-symphony-versions.ts` (Cargo.toml + pi-extension package.json)
- `update-cli-version.ts`

## Troubleshooting

See `release-troubleshooting.md`.
