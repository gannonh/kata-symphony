# GitHub Conventions

Shared contract for every mode of this skill. Read this file completely before Plan, Build, Verify, Triage, or Migrate.

## Core rule

**The GitHub Issue is the spec.** Local Markdown is a temporary body file used to compose or edit an issue body, and it is deleted once the `gh` write succeeds. Nothing spec-shaped stays in the repo working tree.

If `gh` fails, stop and report the failure. Do not save the spec as a repo file as a fallback; that recreates the two-sources-of-truth problem this skill exists to remove.

## Preflight

Run once per session, before the first `gh` write.

```bash
gh auth status
gh repo view --json nameWithOwner,defaultBranchRef -q '.nameWithOwner + " (default: " + .defaultBranchRef.name + ")"'
gh extension list | grep -q 'gh sub-issue' && echo "sub-issue: present" || echo "sub-issue: missing"
```

Checks:

1. **Authentication.** If `gh auth status` fails, stop and ask the user to run `gh auth login`.
2. **Target repo.** `gh repo view` resolves from the current git remote. If the repo is a fork or the roadmap lives elsewhere, ask the user which repo owns the roadmap and pass `--repo <owner>/<name>` on every subsequent command.
3. **Issues enabled.** If `gh issue list` errors with issues disabled, stop and tell the user.
4. **Labels.** Run `scripts/ensure_labels.sh` (see below). It is idempotent and safe to re-run.
5. **Sub-issue extension.** Required only when decomposing a spec. If missing, offer `gh extension install yahsan2/gh-sub-issue` or use the fallback in "Sub-issues" below.

Record the resolved `<owner>/<repo>` for the session and reuse it.

## Label taxonomy

This skill owns three namespaces and creates them automatically:

| Label                       | Color    | Meaning                                                            |
| --------------------------- | -------- | ------------------------------------------------------------------ |
| `kind:spec`                 | `0E8A16` | A standalone spec issue. Every spec issue carries this.            |
| `kind:epic`                 | `5319E7` | A spec decomposed into sub-issues. Carries `kind:spec` as well.    |
| `kind:sub-spec`             | `C2E0C6` | A child issue produced by decomposing an epic.                     |
| `status:draft`              | `FBCA04` | Spec is being written or revised. Build is blocked.                |
| `status:approved`           | `0E8A16` | User approved the spec. Build may start.                           |
| `status:implemented`        | `1D76DB` | Build completed and reported. Verify may start.                    |
| `status:verified`           | `0052CC` | Acceptance evidence accepted by the user.                          |
| `status:blocked`            | `B60205` | Work cannot proceed. The issue body states why.                    |
| `phase:plan`                | `D4C5F9` | Currently in Plan.                                                 |
| `phase:build`               | `BFD4F2` | Currently in Build.                                                |
| `phase:verify`              | `C5DEF5` | Currently in Verify.                                               |
| `needs:acceptance-criteria` | `E99695` | Issue has no usable `## Acceptance criteria` section.              |
| `needs:decomposition`       | `E99695` | Scope is too large for one spec.                                   |
| `needs:triage`              | `E99695` | Issue has not been groomed into this skill's model.                |

Rules:

- `status:*` labels are **mutually exclusive**. Every transition removes the old one and adds the new one in a single `gh issue edit` call.
- `phase:*` labels are also mutually exclusive and reflect what is happening right now. Remove the `phase:*` label when a phase ends without immediately starting the next one.
- `needs:*` labels are triage flags. Clear them when the underlying gap is fixed.
- **Adopt existing repo labels** for area, component, priority, and type. Run `gh label list --limit 200` during preflight and reuse what already exists rather than creating parallel labels. Only the three namespaces above belong to this skill.

Create or repair the taxonomy:

```bash
bash <skill-dir>/scripts/ensure_labels.sh                 # current repo
bash <skill-dir>/scripts/ensure_labels.sh --repo owner/name
bash <skill-dir>/scripts/ensure_labels.sh --dry-run
```

The script uses `gh label create --force`, which creates missing labels and updates color and description on existing ones. It never deletes labels.

## Temporary body files

Compose issue bodies in a scratch file, then write through `gh`:

```bash
BODY="$(mktemp -t pbvg-spec).md"
# write the spec body to "$BODY"
gh issue create --title "<title>" --body-file "$BODY" --label "kind:spec,status:draft,phase:plan"
rm -f "$BODY"
```

Rules:

- Use the session scratchpad directory or `mktemp`. Never write drafts into the repo working tree, not even temporarily, and never into `docs/`.
- Delete the file after the `gh` command exits successfully.
- To revise an issue body, pull the current body down, edit it, push it back, then delete the file:

```bash
gh issue view <N> --json body -q .body > "$BODY"
# edit "$BODY"
gh issue edit <N> --body-file "$BODY"
rm -f "$BODY"
```

- Always round-trip through `--body-file`. Passing multi-line spec bodies through `--body` on the command line mangles backticks, quotes, and newlines.

## Spec issue body template

Use this shape for `kind:spec` and `kind:sub-spec` issues. Scale each section to the work; omit sections that do not apply. `## Status` and `## Acceptance criteria` are mandatory and must use these exact headings.

```markdown
## Status

Draft

## Goal

<the outcome, in one or two sentences>

## Context

<current state, verified facts about the repo, links to related issues, PRs, ADRs, or designs>

## Constraints and non-goals

<explicit boundaries, governing rules, and what this spec will not do>

## Acceptance criteria

- [ ] <observable pass/fail outcome>
- [ ] <observable pass/fail outcome>

## Architecture

<component relationships, boundaries, data flow, Mermaid diagram when relationships matter>

## Implementation phases

1. <phase>: approach, likely files, acceptance tie-in
2. <phase>: approach, likely files, acceptance tie-in

## Verification

<unit, integration, manual, UAT, and command-level checks>

## Risks and mitigations

<specific risks with practical mitigations>

## Build handoff

- Approved scope: <...>
- Non-goals: <...>
- Ordered phases: <...>
- Required verification commands: <...>
- Fixtures or credentials needed: <...>
- Blocking open questions: None
```

Notes:

- Write acceptance criteria as GitHub task-list checkboxes (`- [ ]`). Verify checks them off as each one passes, which makes the issue show live acceptance progress in the list view.
- The `## Status` section mirrors the `status:*` label. Both must agree. Update them in the same turn.
- Do not use YAML frontmatter in issue bodies. GitHub renders it as a table or as literal text. Status lives in the `## Status` section and the label.

## Status transitions

| Transition                             | When                                                   | Command                                                                                             |
| -------------------------------------- | ------------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| create → `status:draft`                | Plan publishes the spec issue                          | `gh issue create --label "kind:spec,status:draft,phase:plan"`                                        |
| `status:draft` → `status:approved`     | User explicitly approves the written spec              | `gh issue edit N --remove-label status:draft --add-label status:approved --remove-label phase:plan`  |
| `status:approved` → `status:implemented` | Build completes all gates and posts its report       | `gh issue edit N --remove-label status:approved,phase:build --add-label status:implemented`          |
| `status:implemented` → `status:verified` | User accepts the acceptance evidence                 | `gh issue edit N --remove-label status:implemented,phase:verify --add-label status:verified`         |
| any → `status:blocked`                 | Work cannot proceed; reason recorded in a comment      | `gh issue edit N --remove-label <current> --add-label status:blocked`                                |
| `status:approved` → `status:draft`     | User requests changes after approval                   | `gh issue edit N --remove-label status:approved --add-label status:draft,phase:plan`                 |

Update the `## Status` section in the body to match, in the same turn as the label change.

Close the issue only when it reaches `status:verified`, or when the user decides the work will not be done. A merged PR containing `Closes #N` closes the issue automatically; if Verify has not run, reopen it or record why verification was skipped.

## Sub-issues

Decompose when a spec has more than one independently buildable and verifiable phase, when phases have different acceptance criteria, or when the user asks.

**Structure: parent holds the outcome, children hold the phases.**

- The parent issue keeps goal, context, constraints, architecture, risks, and top-level acceptance criteria. Label it `kind:spec,kind:epic`.
- Each implementation phase becomes a sub-issue with its own scoped `## Acceptance criteria` and `## Build handoff`. Label each `kind:sub-spec` plus its own `status:*`.
- Children link back to the parent in their `## Context` section.
- Build runs per sub-issue. Verify runs per sub-issue, then rolls up.
- The parent reaches `status:verified` only when every child is `status:verified` and the parent's own top-level acceptance criteria pass.

Create children with the extension:

```bash
gh sub-issue create --parent <N> \
  --title "Phase 1: <name>" \
  --body-file "$BODY" \
  --label "kind:sub-spec,status:approved"

gh sub-issue list <N>            # inspect the hierarchy
gh sub-issue add <N> <CHILD>     # link an existing issue as a child
```

If `gh sub-issue` is unavailable and the user does not want to install it, fall back to a `## Sub-issues` task list in the parent body (`- [ ] #143`) and keep it current by hand. State clearly in the report that native sub-issue links were not used.

Do not nest more than one level. If a child needs decomposition, the parent scope was wrong; return to Plan.

## Branches and pull requests

- Branch name: `<issue-number>-<kebab-title>`, for example `142-export-workflow`. Derive it from the issue with `gh issue develop <N> --name <branch> --base <default>` when the user wants GitHub to link the branch to the issue.
- One branch per spec issue or per sub-issue. Never build two issues on one branch.
- The PR body must contain `Closes #<N>` for the issue it implements. For a sub-issue, close the sub-issue, not the parent.
- Link the PR back to the issue with a comment when the PR is opened from a branch GitHub did not auto-link.

## Comments

Post reports and evidence as issue comments, using a body file:

```bash
gh issue comment <N> --body-file "$REPORT"
rm -f "$REPORT"
```

Conventions:

- Build completion report: comment starting with `## Build completion report`.
- Verify evidence: comment starting with `## Verify: acceptance criteria matrix`.
- Deviation approvals: comment starting with `## Approved deviation`.
- Blocking reasons: comment starting with `## Blocked`.

The issue plus its comments is the complete record for the work. A reader should be able to reconstruct scope, decisions, implementation, and acceptance from the issue alone.

Image and video artifacts cannot be uploaded through `gh`. Reference their repo-relative or absolute paths in the comment, and tell the user which artifacts they may want to drag into the issue by hand.

## Querying the roadmap

```bash
gh issue list --label kind:spec --state open --json number,title,labels,updatedAt
gh issue list --label status:approved --state open          # ready to build
gh issue list --label status:implemented --state open       # ready to verify
gh issue list --label needs:acceptance-criteria --state open
gh issue view <N> --json number,title,body,labels,state,comments,url
gh search issues --repo <owner>/<repo> "<keywords>" --state open   # duplicate check
```

Before Plan creates a new spec issue, search for an existing issue covering the same work. Extend the existing issue instead of opening a duplicate.

## OKF integration

This skill coexists with the `okf` skill. When the repo has an OKF bundle at `./docs`:

- **`docs/specs/` stops holding spec documents.** `docs/specs/index.md` becomes a pointer that tells agents and humans the roadmap lives in GitHub Issues, with the queries needed to read it.
- **`docs/specs/log.md` stays** and records roadmap-level events: migrations, epics opened, specs verified. Keep entries newest-first under `YYYY-MM-DD` headings.
- **`docs/adrs/` is unaffected.** Durable architecture decisions remain ADR files in the repo. A spec issue that depends on a decision links to the ADR path; the ADR links back to the issue URL.
- **Other OKF sections are unaffected.** `architecture/`, `guides/`, `reference/`, `runbooks/`, and `domains/` keep working as before.
- OKF validation still passes because `docs/specs/index.md` and `docs/specs/log.md` are reserved files that need no frontmatter, and no non-reserved Markdown remains in `docs/specs/` outside `archive/`.
- Archived spec files under `docs/specs/archive/` keep their OKF frontmatter so `validate_okf.py` continues to pass.

### Archived spec frontmatter

Every archived file carries the same normalized shape, so the frontmatter and the pointer line in the body always agree:

| Key             | Value                                                                     |
| --------------- | ------------------------------------------------------------------------- |
| `type`          | The original `type`, or `Spec` when the source file had none.             |
| `title`         | The original `title`, first `# ` heading, or a title derived from the name. |
| `status`        | `Migrated` when the spec became an issue. `Completed` when archived as finished work. |
| `source_status` | The status the file declared before the migration.                        |
| `github_issue`  | The issue number. Present only when `status: Migrated`.                    |
| `migrated`      | `true` for issue-backed specs, `false` for completed ones.                 |
| `archived_at`   | UTC timestamp of the archive operation.                                   |

The pre-migration status is preserved in `source_status` rather than left in `status`. Two keys with two different answers to "is this done" is worse than either answer alone.

Use this content for `docs/specs/index.md`:

````markdown
# Specs

Specs for this project are GitHub Issues. This directory holds no spec documents.

## Read the roadmap

```bash
gh issue list --label kind:spec --state open            # all active specs
gh issue list --label status:approved --state open      # approved, ready to build
gh issue list --label status:implemented --state open   # built, awaiting verification
gh issue view <N>                                       # read a spec
gh sub-issue list <N>                                   # read an epic's phases
```

## Status model

| Label                | Meaning                                     |
| -------------------- | ------------------------------------------- |
| `status:draft`       | Being written or revised. Do not build.     |
| `status:approved`    | Approved by the maintainer. Ready to build. |
| `status:implemented` | Built and reported. Ready to verify.        |
| `status:verified`    | Acceptance evidence accepted.               |
| `status:blocked`     | Cannot proceed. See the issue body.         |

## Writing and executing specs

Use the `plan-build-verify-github` skill. It publishes specs as issues, runs Build against approved issues, and posts acceptance evidence back to the issue.

## Archive

Pre-migration spec files are preserved under [`archive/`](./archive/) with links to their issues. They are historical and are not maintained.
````

The migration script appends the previous index content below this pointer under a "Roadmap context carried over from the previous index" heading rather than discarding it. Reconcile that section against the GitHub roadmap and delete it once the active and deferred items exist as issues. Pass `--replace-index` only when the previous index holds nothing worth keeping.

Add this to `AGENTS.md` during migration:

```markdown
## Specs live in GitHub Issues

Specs for this repository are GitHub Issues, not files. `docs/specs/` holds only an index pointer and an archive of pre-migration specs.

- Read the roadmap with `gh issue list --label kind:spec --state open`.
- Read a spec with `gh issue view <N>`; read an epic's phases with `gh sub-issue list <N>`.
- Do not create spec files under `docs/specs/`. Use the `plan-build-verify-github` skill, which publishes specs as issues.
- Never build an issue that is not labeled `status:approved` without explicit maintainer approval.
- Post build reports and acceptance evidence as comments on the spec issue.
- ADRs remain files under `docs/adrs/`. Cross-link them with the issues they constrain.
```

## Phase-entry hygiene check

At the start of Plan, Build, and Verify, run a lightweight check on the issues in scope and **report** problems without fixing them:

```bash
gh issue view <N> --json number,title,labels,body,state
```

Report when:

- The issue has no `status:*` label, or more than one.
- The `## Status` section disagrees with the `status:*` label.
- `## Acceptance criteria` is missing, empty, or contains vague language.
- An epic has no sub-issues, or a sub-issue has no parent.
- The issue is closed but not `status:verified`.
- The issue duplicates another open spec issue.

State the findings in one short block, then continue with the requested phase unless a finding blocks it (missing acceptance criteria blocks Build and Verify; an unapproved status blocks Build). To fix hygiene problems across the backlog, run Triage.

## Safety rules

- Never close, delete, or bulk-relabel issues you did not open without user approval.
- Never edit an issue body you have not read in full in this session.
- Never force-push or amend commits on a branch linked to an issue another agent or person is working on.
- When a `gh` write fails, report the exact command and error. Do not retry silently and do not fall back to local files.
