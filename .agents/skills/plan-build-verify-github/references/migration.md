# Migration Workflow

Use this workflow to move a project from file-based specs (`plan-build-verify`) to GitHub Issues (`plan-build-verify-github`).

Migration is a one-time, one-way change. After it, the issue is the spec and `docs/specs/` holds only an index pointer and an archive. Read `references/github-conventions.md` before starting.

## What migrates and what does not

| Source spec state                                                      | Result                                                       |
| ---------------------------------------------------------------------- | ------------------------------------------------------------ |
| `Draft`, `Proposed`, `Idea`                                             | Issue created, `status:draft`                                 |
| `Approved`, `Ready`, `In progress`, `WIP`                               | Issue created, `status:approved`                              |
| `Implemented`, `Built`, `Needs verification`                            | Issue created, `status:implemented`                           |
| `Blocked`, `On hold`                                                    | Issue created, `status:blocked`                               |
| `Verified`, `Complete`, `Done`, `Shipped`, `Superseded`, `Cancelled`, … | **No issue.** Archived locally as history.                    |
| No status, or an unrecognized status                                    | **Skipped.** Left in place and reported, so you can decide.   |

**Completed specs do not become issues.** Finished work has no place on a roadmap; migrating it would fill the backlog with issues that open and close in the same breath. Completed specs move to `docs/specs/archive/` with a header noting they predate the migration, and the migration log records them.

`Implemented` still migrates, because that work is not finished. It has been built but not verified, and Verify needs an issue to post evidence to. If your project uses `Implemented` to mean "done and shipped", say so and pass those files through with `--default-status completed` or fix the status values first.

## Step 1: Assess

```bash
find docs/specs -name '*.md' ! -name 'index.md' ! -name 'log.md' | sort
grep -rHn '^status:' docs/specs --include='*.md' | sort -t: -k3
```

Report to the user:

- How many spec files exist, and their status distribution.
- Which will become issues, which will be archived as complete, and which have no usable status.
- Whether any spec is large enough to become an epic with sub-issues. The script does not decompose; that happens after migration.

Ask the user to confirm the plan before running anything.

## Step 2: Preflight

1. Run the `gh` preflight from `references/github-conventions.md`.
2. Confirm the target repo is the one that should own the roadmap. For a fork, ask.
3. Commit or stash any pending changes under `docs/specs/`. The script refuses to run on a dirty specs directory.
4. Confirm the user wants the migration on the current branch, or create a branch for it.

## Step 3: Dry run

```bash
bash <skill-dir>/scripts/migrate_specs.sh --dry-run
```

The dry run creates nothing. It prints, per file, the issue title, the labels, and the archive destination, plus the files it will skip and why.

Show the output to the user. Look for:

- Titles derived from filenames rather than frontmatter. Those files have no `title:` and no `# ` heading; the derived title is usually poor. Fix the source file or plan to rename the issue afterward.
- `needs:acceptance-criteria` on anything approved or implemented. Those specs were built without testable criteria and will block Verify.
- Files skipped for unclear status. Decide each one: set a status, or leave it as an untracked document.
- Anything that should be an epic.

## Step 4: Migrate

```bash
bash <skill-dir>/scripts/migrate_specs.sh
```

Optional flags:

- `--repo owner/name` when the roadmap repo is not the current remote.
- `--specs-dir <path>` when specs do not live in `docs/specs`.
- `--default-status draft` to migrate files with no declared status instead of skipping them.
- `--include-completed` to create issues for completed specs. Use only when the user explicitly wants a historical record in GitHub.

What the script does per file:

1. Reads `status` from frontmatter, falling back to a `## Status` section, falling back to `--default-status`.
2. Skips files that already carry a `github_issue:` key, so re-runs are safe.
3. Creates the issue with the spec body, `## Status` section, and a migration note, labeled `kind:spec` plus its status label.
4. Adds `needs:acceptance-criteria` when the body has no `## Acceptance criteria` heading.
5. Moves the file to `docs/specs/archive/`, preserving subdirectories, and adds `github_issue`, `migrated`, and `archived_at` frontmatter keys plus a pointer line.
6. Rewrites `docs/specs/index.md` as a GitHub pointer.
7. Prepends a dated migration entry to `docs/specs/log.md` listing every file and its issue.

Changes are staged, not committed. The script exits non-zero if any file failed.

## Step 5: Update AGENTS.md

Add the AGENTS.md snippet from `references/github-conventions.md` so future agents look for specs in GitHub rather than in `docs/specs/`.

Remove or amend any existing instruction that tells agents to write specs to `docs/specs/YYYY-MM-DD-<topic>.md`. Leaving a stale instruction in place produces agents that write spec files no one reads.

Check the same for `CLAUDE.md`, `README.md`, `CONTRIBUTING.md`, and any `.github/` templates that reference the old spec path.

## Step 6: Validate

```bash
python <okf-skill-dir>/scripts/validate_okf.py .      # if the repo uses OKF
gh issue list --label kind:spec --state open --json number,title,labels
git status --short
```

Confirm:

- Every migrated file has an issue, and every issue has a body that reads as a spec.
- `docs/specs/` contains only `index.md`, `log.md`, and `archive/`.
- Archived files still have valid OKF frontmatter with a non-empty `type`.
- Labels exist and are applied correctly.
- No spec file remains outside the archive except ones the user chose to leave.

Spot-check two or three issues by eye. The script does not reformat spec bodies, so anything that rendered badly as a file renders badly as an issue.

## Step 7: Decompose

The script does not create sub-issues. After migration, review the migrated issues for specs with more than one independently buildable phase:

1. Add `kind:epic` to the parent.
2. Create a sub-issue per phase with `gh sub-issue create --parent <N>`, following the decomposition rules in `references/github-conventions.md`.
3. Move each phase's acceptance criteria from the parent into the child that owns it, leaving outcome-level criteria on the parent.

Run Triage (`references/triage.md`) afterward to check hierarchy integrity and produce the ready-to-work ordering.

## Step 8: Commit and report

```bash
git commit -m "chore(specs): migrate specs to GitHub Issues"
```

Report:

- Issues created, with numbers and titles.
- Specs archived as complete, with no issue.
- Specs skipped, and what the user needs to decide about each.
- Files changed: index, log, archive, AGENTS.md.
- Issues flagged `needs:acceptance-criteria`.
- Recommended next step, usually Triage or building the first approved issue.

## Rollback

Nothing is deleted, so rollback is a git revert plus closing the created issues:

```bash
git revert <migration-commit>
gh issue list --label kind:spec --json number -q '.[].number' | xargs -I{} gh issue close {} --reason "not planned"
```

Close only the issues the migration created. Check the numbers against the migration log entry in `docs/specs/log.md` before running anything in bulk.

## Partial migration

To migrate one directory at a time, use `--specs-dir` on a subdirectory. The index rewrite and log entry apply to that directory, so run the full flow per directory and reconcile the top-level `docs/specs/index.md` by hand at the end.

Do not run a partial migration and leave both systems live for long. Two sources of truth is the failure mode this skill exists to prevent.
