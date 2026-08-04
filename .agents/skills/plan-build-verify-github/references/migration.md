# Migration Workflow

Use this workflow to move a project from file-based specs (`plan-build-verify`) to GitHub Issues (`plan-build-verify-github`).

Migration is a one-time, one-way change. After it, the issue is the spec and `docs/specs/` holds only an index pointer and an archive. Read `references/github-conventions.md` before starting.

The script never guesses. Anything it cannot classify with evidence is reported and left in place for a human decision.

## Resolve the script paths first

Run the scripts from the skill directory that the agent runtime actually loaded, not from a path you assume exists. Check both before the first command:

```bash
ls <skill-dir>/scripts/migrate_specs.sh
ls <okf-skill-dir>/scripts/validate_okf.py     # only if the repo uses OKF
```

If a configured skill path does not exist, stop and ask the user which installation to use. Do not copy skill files into the project working tree to make a path resolve. Materializing skill assets inside the repo leaves untracked files that pollute the migration diff.

## What migrates and what does not

| Source spec state                                                      | Result                                                       |
| ---------------------------------------------------------------------- | ------------------------------------------------------------ |
| `Draft`, `Proposed`, `Idea`, `Planned`                                  | Issue created, `status:draft`                                 |
| `Approved`, `Ready`, `In progress`, `WIP`                               | Issue created, `status:approved`                              |
| `Implemented`, `Built`, `Needs verification`                            | Governed by `--implemented-action`. Default: issue created.   |
| `Blocked`, `On hold`                                                    | Issue created, `status:blocked`                               |
| `Verified`, `Complete`, `Done`, `Shipped`, `Superseded`, `Cancelled`, … | **No issue.** Archived locally as history.                    |
| No status, or an unrecognized status                                    | **Skipped.** Left in place and reported, so you can decide.   |

**Completed specs do not become issues.** Finished work has no place on a roadmap; migrating it would fill the backlog with issues that open and close in the same breath. Completed specs move to `docs/specs/archive/` with a header noting they predate the migration, and the migration log records them.

`Implemented` defaults to migrating, because that work is not finished: it has been built but not verified, and Verify needs an issue to post evidence to. Projects that use `Implemented` to mean "done and shipped" should pass `--implemented-action archive`.

## Status detection

The script reads status from four shapes, highest precedence first:

| Source        | Shape                                                       | Confidence |
| ------------- | ----------------------------------------------------------- | ---------- |
| `override`    | An entry in the `--status-map` manifest                     | high       |
| `frontmatter` | `status: Approved` in YAML frontmatter                      | high       |
| `phase`       | `- **Build**: Implemented` under a `## Status` heading       | high       |
| `section`     | A bare value on the first line under `## Status`            | high       |
| `inline`      | `**Status**: Implemented` anywhere in the body               | medium     |
| `default`     | The `--default-status` flag                                 | low        |

Phase forms use the furthest phase present. `Plan: Approved` plus `Build: Implemented` resolves to `Implemented`, because Build ran after Plan. `Verify: Completed` beats both.

Prose is never read for status. A build report that says "this shipped last quarter" but declares no status is reported as unknown, not inferred as complete.

When two sources classify to different states, the file has **conflicting status evidence**. The script reports every conflict and refuses to write anything until you resolve them in the source files, decide them with `--status-map`, or accept the highest-precedence value with `--allow-conflicts`.

## Step 1: Assess

```bash
bash <skill-dir>/scripts/migrate_specs.sh --assess
```

This is read-only. It prints, per file: detected status, the source it came from, a confidence level, the derived title, and the planned action. It also reports conflicts, the current label state, and how many existing open issues carry `kind:spec`.

Report to the user:

- The status distribution and how many files are unclassified.
- Every file with `medium` or `low` confidence, and every conflict.
- Every title derived from a filename rather than frontmatter or an `# ` heading. Those titles are usually poor; fix the source file or plan to rename the issue.
- Which specs are large enough to become epics. The script does not decompose; that happens after migration.
- Whether the repo already has open issues that the new `kind:spec` query will not find.

Ask the user to confirm the plan before running anything.

## Step 2: Classify the unknowns

For every file the assessment could not classify, write a decision into a status map instead of guessing:

```
# migration-status.txt
# <path relative to the specs dir>  <status>
2026-03-11-mcp-oauth-verify-report.md   completed
2026-04-02-browser-shell.md             approved
legacy/2025-08-01-import.md             completed
```

Then re-assess with it:

```bash
bash <skill-dir>/scripts/migrate_specs.sh --assess --status-map migration-status.txt
```

Map entries win over every other source, so they also resolve conflicts. Show the user the resulting classification and get explicit agreement on anything you inferred rather than read.

`--default-status` remains available for the case where every unclassified file should get the same treatment. Prefer the map when files differ.

## Step 3: Preflight

1. Run the `gh` preflight from `references/github-conventions.md`.
2. Confirm the target repo is the one that should own the roadmap. For a fork, ask.
3. Commit or stash any pending changes under `docs/specs/`. The script refuses to run on a dirty specs directory.
4. Confirm the user wants the migration on the current branch, or create a branch for it.
5. Decide the label plan for pre-existing issues. The script never relabels an issue it did not create. If the roadmap already lives in issues under different labels, agree the mapping with the user and apply it separately.

## Step 4: Dry run

```bash
bash <skill-dir>/scripts/migrate_specs.sh --dry-run --status-map migration-status.txt
```

The dry run creates nothing. On top of the assessment it prints the exact per-file plan, the label changes it would make (which labels exist and which would be created), whether the index will preserve or replace existing content, and whether links will be rewritten.

Show the output to the user. Look for:

- `needs:acceptance-criteria` on anything approved or implemented. Those specs were built without testable criteria and will block Verify.
- Files still skipped for unclear status.
- Anything that should be an epic.
- Labels being created in a repo where no issues will be created. That happens only with `--ensure-labels`.

## Step 5: Migrate

```bash
bash <skill-dir>/scripts/migrate_specs.sh --status-map migration-status.txt
```

Options:

| Flag                        | Effect                                                                       |
| --------------------------- | ---------------------------------------------------------------------------- |
| `--repo owner/name`         | Target repo when the roadmap is not the current remote.                      |
| `--specs-dir <path>`        | Spec directory other than `docs/specs`.                                       |
| `--docs-root <path>`        | Root for `/`-absolute OKF links. Default `docs`.                              |
| `--status-map <file>`       | Per-file status decisions. Highest precedence.                                |
| `--default-status <s>`      | Status for files that declare none.                                           |
| `--implemented-action <a>`  | `migrate` (default), `archive`, `blocked`, or `skip`.                         |
| `--include-completed`       | Create issues for completed specs too.                                        |
| `--allow-conflicts`         | Proceed on conflicting evidence, taking the highest-precedence value.         |
| `--replace-index`           | Replace `docs/specs/index.md` wholesale instead of preserving its content.    |
| `--ensure-labels`           | Create the label taxonomy even when no issue will be created.                 |
| `--no-rewrite-links`        | Skip Markdown link rewriting. Cross-links will break.                         |
| `--no-verify-labels`        | Skip the read-only pre-migration issue and label report.                      |

What the script does per file:

1. Resolves status through the precedence chain above and applies the action policy.
2. Skips files that already carry a `github_issue:` key.
3. Looks for an existing issue carrying this file's source key, and reuses it instead of creating a duplicate.
4. Creates the issue with the spec body, a `## Status` section, a migration note, and a source key line, labeled `kind:spec` plus its status label.
5. Adds `needs:acceptance-criteria` when the body has no `## Acceptance criteria` heading.
6. Moves the file to `docs/specs/archive/`, preserving subdirectories.
7. Rewrites Markdown links across tracked documentation so references to the moved file resolve to its archive path.
8. Preserves the existing `docs/specs/index.md` content below the new GitHub pointer.
9. Adds a dated migration entry to `docs/specs/log.md`, merging into today's heading when one exists.

Label setup runs only when at least one issue will be created, or when `--ensure-labels` is passed.

Changes are staged, not committed. The script exits non-zero if any file failed.

### Archived file metadata

Archived frontmatter is normalized so the file and its pointer line always agree:

```yaml
---
type: Spec
title: Export workflow
status: Migrated          # or Completed, when archived without an issue
source_status: Approved   # what the file said before the migration
github_issue: 142         # present only for migrated specs
migrated: true            # false for completed specs
archived_at: 2026-08-04T09:36:10Z
---
```

`type` is filled in when the source file had none, so OKF validation keeps passing.

### Partial failure

If a run dies partway, re-run it. Every issue body carries `pbvg-source:<path>`. The script scans existing issues for that key before creating anything, so a file whose issue exists but was never archived is picked up rather than duplicated. Failures are listed in the summary and recorded in the migration log entry.

## Step 6: Update AGENTS.md

Add the AGENTS.md snippet from `references/github-conventions.md` so future agents look for specs in GitHub rather than in `docs/specs/`.

Remove or amend any existing instruction that tells agents to write specs to `docs/specs/YYYY-MM-DD-<topic>.md`. Leaving a stale instruction in place produces agents that write spec files no one reads.

Check the same for `CLAUDE.md`, `README.md`, `CONTRIBUTING.md`, and any `.github/` templates that reference the old spec path.

## Step 7: Validate

```bash
python <okf-skill-dir>/scripts/validate_okf.py .      # if the repo uses OKF
gh issue list --label kind:spec --state open --json number,title,labels
git status --short
git diff --cached --check
```

Confirm:

- Every migrated file has an issue, and every issue has a body that reads as a spec.
- `docs/specs/` contains only `index.md`, `log.md`, and `archive/`.
- Archived files have a non-empty `type`, a normalized `status`, and a `source_status`.
- Labels exist and are applied correctly.
- The link rewrite report lists no unresolved links inside the specs tree, or the ones it lists were already broken before the migration.
- `docs/specs/index.md` still carries the roadmap context you expected to keep.
- No spec file remains outside the archive except ones the user chose to leave.

Spot-check two or three issues by eye. The script does not reformat spec bodies, so anything that rendered badly as a file renders badly as an issue.

## Step 8: Decompose

The script does not create sub-issues. After migration, review the migrated issues for specs with more than one independently buildable phase:

1. Add `kind:epic` to the parent.
2. Create a sub-issue per phase with `gh sub-issue create --parent <N>`, following the decomposition rules in `references/github-conventions.md`.
3. Move each phase's acceptance criteria from the parent into the child that owns it, leaving outcome-level criteria on the parent.

Run Triage (`references/triage.md`) afterward to check hierarchy integrity and produce the ready-to-work ordering.

## Step 9: Commit and report

```bash
git commit -m "chore(specs): migrate specs to GitHub Issues"
```

Report:

- Issues created, with numbers and titles.
- Specs archived as complete, with no issue.
- Specs skipped, and what the user needs to decide about each.
- Any issues reused or recovered by source key.
- Files changed: index, log, archive, rewritten links, AGENTS.md.
- Issues flagged `needs:acceptance-criteria`.
- Preserved index content that still needs reconciling against the GitHub roadmap.
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

## Tests

`tests/test_migrate_specs.py` covers the status shapes, the action policies, archive metadata normalization, link rewriting, index preservation, same-day log merging, label gating, and issue recovery. Run it with any spec-parsing change:

```bash
python -m pytest <skill-dir>/tests -q
```
