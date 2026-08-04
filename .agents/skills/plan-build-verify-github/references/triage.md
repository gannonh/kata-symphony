# Triage Workflow

Use this workflow to groom the issue backlog so the roadmap stays readable and every spec issue is in a state Build or Verify can act on.

Triage is on-demand. It never runs automatically. Plan, Build, and Verify perform a lightweight read-only hygiene check at phase entry and report findings; fixing them is this workflow's job. Read `references/github-conventions.md` before starting.

## Scope

Ask which scope to groom if the user did not say:

- **Full backlog** — every open issue in the repo.
- **Spec backlog** — `kind:spec` issues only. Default when the user says "groom the roadmap".
- **One epic** — a parent and its sub-issues.
- **Untriaged** — issues labeled `needs:triage` or carrying no `kind:*` label.

## Step 1: Load the backlog

```bash
gh issue list --state open --limit 200 \
  --json number,title,labels,state,updatedAt,createdAt,assignees,milestone,url

gh issue list --label kind:spec --state open --limit 200 \
  --json number,title,labels,body,updatedAt,url

gh issue list --label kind:epic --state open \
  --json number,title,url            # then gh sub-issue list <N> for each

gh issue list --state closed --limit 50 \
  --json number,title,labels,closedAt,url
```

Read issue bodies for the specs in scope. Do not judge an issue from its title.

## Step 2: Detect defects

Check every issue in scope against these rules.

**Label integrity**

- No `status:*` label on a `kind:spec` issue.
- More than one `status:*` label.
- More than one `phase:*` label.
- A `phase:*` label on an issue nobody is working on. A stale `phase:build` label means an abandoned branch or a crashed session.
- No `kind:*` label on an issue that is clearly a spec.
- A `needs:*` flag whose underlying gap is already fixed.

**Body integrity**

- Missing `## Status` section, or a `## Status` value that disagrees with the `status:*` label.
- Missing `## Acceptance criteria` heading.
- Acceptance criteria present but not checkbox-formatted, empty, or written with vague language ("works", "fast", "robust", "easy") and no observable threshold.
- Missing `## Build handoff` on a `status:approved` issue.
- `Blocking open questions` that is not `None` on a `status:approved` issue.
- Placeholder text: `TBD`, `TODO`, `<...>`, unfilled template sections.

**State integrity**

- Closed but not `status:verified`, with no comment explaining why.
- `status:verified` but still open.
- `status:implemented` with no Build completion report comment.
- `status:verified` with no acceptance criteria matrix comment.
- `status:blocked` with no comment stating the blocker, or a blocker that has since been resolved.

**Hierarchy integrity**

- `kind:epic` with no sub-issues.
- `kind:sub-spec` with no parent.
- A sub-issue whose acceptance criteria do not roll up to any parent criterion.
- A parent whose acceptance criteria are not covered by any child.
- Sub-issues nested more than one level deep.
- A parent at `status:verified` with unverified children.

**Scope integrity**

- A spec whose acceptance criteria span more than one independently buildable phase. Flag `needs:decomposition`.
- Duplicate or overlapping specs. Check with `gh search issues --repo <owner>/<repo> "<keywords>"`.
- An issue that is a bug report or chore rather than a spec, carrying `kind:spec`.

**Staleness**

- `status:draft` untouched for more than 30 days.
- `status:approved` untouched for more than 60 days with no branch or PR.
- `status:implemented` untouched for more than 14 days. Verification is overdue.
- `status:blocked` untouched for more than 30 days.

Use `updatedAt` for these, and adjust the thresholds to the repo's actual cadence. State the thresholds you used.

## Step 3: Report before acting

Present findings grouped by severity. Do not mutate anything yet.

```text
Blocking (Build or Verify cannot run)
  #150  no acceptance criteria                        → add needs:acceptance-criteria
  #147  status:approved but Blocking open questions: 2 → return to Plan

Integrity (state is misleading)
  #142  label status:approved, body says Implemented   → reconcile to Implemented
  #139  closed, never verified                         → reopen or record why
  #155  kind:epic with no sub-issues                   → decompose or drop kind:epic

Hygiene (safe corrections)
  #161  no kind:* label                                → add kind:spec
  #158  phase:build, no branch since 2026-06-02        → remove phase:build

Stale (needs a decision)
  #133  status:draft, 74 days untouched                → close, or revive?
  #128  status:approved, 91 days, no branch            → still on the roadmap?

Scope
  #149 and #152 overlap on export handling             → merge, or split the boundary?
```

## Step 4: Apply corrections

Apply in tiers, and get approval before anything in tier 2 or 3.

**Tier 1: safe, apply directly.** Report what you did.

- Add a missing `kind:*` label that is unambiguous from the body.
- Add `needs:acceptance-criteria`, `needs:decomposition`, or `needs:triage` flags.
- Remove a `needs:*` flag whose gap is fixed.
- Remove a stale `phase:*` label when no branch or PR is active.
- Reconcile a `## Status` section to match its `status:*` label when the label is clearly right (a Build report comment exists, a PR is merged).
- Link an orphaned sub-issue to its obvious parent with `gh sub-issue add`.

**Tier 2: ask first.** These change what the roadmap says.

- Swap a `status:*` label to a different state.
- Reopen a closed issue, or close an open one.
- Mark an issue `status:blocked`.
- Merge duplicates, which means closing one and folding its content into the other.
- Convert a spec into an epic and create sub-issues.
- Change milestone or priority.

**Tier 3: never without explicit instruction.**

- Delete an issue.
- Bulk-relabel more than 10 issues in one action.
- Edit acceptance criteria on a `status:implemented` or `status:verified` issue.
- Close stale issues in bulk.

Batch the `gh` calls once approved, and report the exact commands run.

```bash
gh issue edit 142 --remove-label "status:approved" --add-label "status:implemented"
gh issue edit 161 --add-label "kind:spec"
gh issue edit 158 --remove-label "phase:build"
```

Fixing a body requires the round-trip from `references/github-conventions.md`: view to a temp file, edit, `gh issue edit --body-file`, delete the temp file.

## Step 5: Order the backlog

After corrections, produce the ready-to-work ordering.

```bash
gh issue list --label status:approved --state open --json number,title,labels,milestone,url
```

An issue is **ready** when:

- It is `status:approved`.
- It has usable acceptance criteria.
- It is not `status:blocked` and has no unresolved dependency.
- For a sub-issue, its stated dependencies are `status:verified` or `status:implemented`.
- No open PR already implements it.

Rank ready issues by:

1. Unblocking value: how many other issues depend on it.
2. Milestone or explicit user priority.
3. Age, oldest first, among equals.

Present the top few with the reason each is ranked where it is. Say plainly when two can run concurrently and when they would collide, based on the code paths each would touch, not just the issue text.

## Step 6: Report

Summarize:

- Scope groomed and how many issues were read.
- Corrections applied, by tier.
- Corrections proposed and awaiting approval.
- Blocking defects that stop Build or Verify.
- Recommended next issue, with the reason.
- Staleness decisions still owed by the user.

When the repo has an OKF bundle and the grooming pass changed the roadmap materially, append an entry to `docs/specs/log.md`.

## Answering "what should we work on next?"

This is the short path through Triage. Run steps 1, 5, and 6 only. Skip the full defect scan, but still report any blocking defect on the issue you recommend, since it would stop Build immediately.
