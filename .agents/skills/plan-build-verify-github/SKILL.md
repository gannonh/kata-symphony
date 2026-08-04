---
name: plan-build-verify-github
description: Use this skill for multi-step, spec-driven, or acceptance-gated implementation work in a GitHub repository where specs live as GitHub Issues. Routes work through Plan, Build, and Verify phases, manages the roadmap through issue labels and sub-issues, and triages and grooms the backlog. Also handles migration from file-based specs to GitHub Issues.
---

# Plan Build Verify (GitHub)

Use this skill to route implementation work through a sequential path where **GitHub Issues are the source of truth for specs**:

1. **Plan**: align with the user on intent, constraints, approach, and acceptance criteria, then publish the spec as a GitHub Issue.
2. **Build**: execute an approved spec issue.
3. **Verify**: validate completed work against the issue's acceptance criteria and post evidence to the issue.

Two supporting modes manage the roadmap:

4. **Triage**: groom the issue backlog for label hygiene, missing acceptance criteria, staleness, orphaned sub-issues, and readiness.
5. **Migrate**: convert an existing file-based `docs/specs/` bundle into GitHub Issues.

Most work should move through Plan → Build → Verify. Default to **Plan** unless the user explicitly directs you to execute an existing issue, verify completed work, groom the backlog, or migrate specs.

When the selected phase is **Plan**, do not jump straight to a written spec. Plan starts with context exploration and user alignment. The issue records the agreed direction after the user has responded to the alignment phase and approved or redirected the recommended approach.

## Read the conventions first

Before any mode, read `references/github-conventions.md` completely. It defines the repo preflight, label taxonomy, issue body template, status transitions, sub-issue mechanics, temporary body files, and the OKF integration contract. Every other reference file depends on it.

## Phase contracts

| Phase   | Input                                                     | Output                                                                                                                                                                     | Status label transition                       |
| ------- | --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| Plan    | Idea, vague request, or new build request                  | Context exploration → alignment dialogue → approach approval → spec issue with mandatory `## Acceptance criteria` and Build handoff → approval after explicit user approval | none → `status:draft` → `status:approved`     |
| Build   | Approved spec issue number, or explicit user override      | Implemented tasks, commits, PR, review results, Build completion report comment                                                                                            | `status:approved` → `status:implemented`      |
| Verify  | Implemented spec issue plus completed work or Build report | Acceptance evidence comment, checked acceptance criteria, signoff recommendation                                                                                           | `status:implemented` → `status:verified`      |
| Triage  | Repo issue backlog                                         | Grooming report and applied label, decomposition, and closure actions                                                                                                      | corrective, per issue                         |
| Migrate | Existing `docs/specs/*.md` bundle                          | Spec issues, archived local files, rewritten specs index                                                                                                                   | derived from each file's frontmatter `status` |

Build must not start from a `status:draft` issue unless the user explicitly overrides the approval gate. Verify must not claim signoff without evidence.

## Select the workflow

Determine the current phase from the user's request and repo state. If the user gives a new build request, start with Plan even if they use the word "build".

Use **Plan** when:

- The user asks to plan, scope, design, spec, shape, or prepare work.
- The user asks to build a new feature, product surface, integration, or architecture change without pointing to an approved issue.
- No `status:approved` issue exists for the requested work.
- The safest next step is interactive alignment and a published spec issue before coding.

Examples that start with Plan:

- `/plan-build-verify-github "Let's build an analytics dashboard for foo bar."`
- `/plan-build-verify-github "Add API-backed project creation."`
- `/plan-build-verify-github "We need a Slack integration for agent updates."`

Use **Build** when:

- The user asks to execute, implement, build, or continue from an approved spec issue.
- The request names an issue number, issue URL, or issue title and asks for code changes.
- The current task is implementation rather than discovery or acceptance review.

Examples that start with Build:

- `/plan-build-verify-github "Implement #142"`
- `/plan-build-verify-github "Execute the approved export workflow issue."`
- `/plan-build-verify-github "Continue the billing settings spec."`

Use **Verify** when:

- The user asks to verify, validate, review, test, sign off, run UAT, or prove work is complete.
- Implementation appears done and the next step is acceptance evidence.
- The user asks whether a branch, feature, or issue is ready to merge or hand off.

Examples that start with Verify:

- `/plan-build-verify-github "Run UAT on #142."`
- `/plan-build-verify-github "Verify the analytics dashboard work is complete."`
- `/plan-build-verify-github "Check whether this branch is ready to merge."`

Use **Triage** when:

- The user asks to triage, groom, clean up, prioritize, or review the backlog or roadmap.
- The user asks what to work on next.
- A phase-entry check surfaced issue hygiene problems and the user asked you to fix them.

Examples that start with Triage:

- `/plan-build-verify-github triage`
- `/plan-build-verify-github "What should we work on next?"`
- `/plan-build-verify-github "Groom the backlog."`

Use **Migrate** when:

- The user asks to migrate, move, or convert file-based specs to GitHub Issues.
- The repo has `docs/specs/*.md` spec files and the user wants this skill to take over the roadmap.

Examples that start with Migrate:

- `/plan-build-verify-github migrate`
- `/plan-build-verify-github "Move our docs/specs to GitHub Issues."`

For tiny, clearly bounded edits such as a copy change or single config tweak, do not force the full Plan workflow and do not open an issue. State the assumption and ask whether the user wants the full Plan → Build → Verify process.

If the mode remains ambiguous after applying these rules, ask one focused question that resolves it.

## Load the workflow instructions

After selecting the mode, read `references/github-conventions.md` and then the corresponding reference file completely, and follow both. For Plan, the first reference step after context exploration is alignment. If you are about to draft a spec issue before the user has answered an alignment question or approved a recommended direction, stop and ask instead.

Reference files:

- Shared conventions: `references/github-conventions.md` (always)
- Plan: `references/plan.md`
- Build: `references/build.md`
- Verify: `references/verify.md`
- Triage: `references/triage.md`
- Migrate: `references/migration.md`

Only load the workflow you need. If the selected workflow's reference file is empty or incomplete, say so and ask the user whether to draft that workflow before proceeding.

## Bundled phase references

Build and Verify load bundled reference workflows from this skill directory. Read the entry point completely; follow linked files and scripts under the same subtree.

| Phase  | Workflow        | Entry point                              |
| ------ | --------------- | ---------------------------------------- |
| Build  | TDD             | `references/tdd/workflow.md`             |
| Verify | User acceptance | `references/user-acceptance/workflow.md` |

Scripts for user-acceptance evidence live under `scripts/user-acceptance/`.

## Helper scripts

- `scripts/ensure_labels.sh`: idempotently creates this skill's label taxonomy in the target repo.
- `scripts/migrate_specs.sh`: bulk-converts `docs/specs/*.md` into spec issues, archives the source files, and rewrites the specs index. Supports `--dry-run`.

## Requirements

- `gh` CLI, authenticated with issue write access for the target repo.
- `gh sub-issue` extension (`yahsan2/gh-sub-issue`) for decomposed specs. If absent, install with `gh extension install yahsan2/gh-sub-issue` or fall back to the task-list mechanism described in `references/github-conventions.md`.
- A git remote pointing at the GitHub repository that owns the roadmap.

Run the preflight in `references/github-conventions.md` before the first `gh` write of a session. If `gh` is unavailable or unauthenticated, stop and tell the user. Do not fall back to writing spec files locally.

## Shared principles

- Inspect the repo and the issue backlog before making claims about project structure, commands, or existing work.
- Use a todo list when the work has multiple steps.
- In Plan, ask focused alignment questions one at a time before drafting the issue. If no factual clarification is needed, ask the user to confirm your framing, assumptions, acceptance criteria, success criteria, or recommended direction.
- In Plan, propose 2-3 approaches before settling when more than one viable direction exists. If one approach is clearly best, state the recommendation and wait for approval or redirection.
- Prefer small, verifiable phases over broad unverified changes.
- Keep scope tied to the selected issue and its acceptance criteria.
- A spec issue is incomplete unless it has an exact `## Acceptance criteria` section with observable pass/fail criteria that Build can implement and Verify can test.
- The issue is the spec. Local Markdown exists only as a temporary body file and is removed after the `gh` write succeeds.
- Every phase transition updates the issue's `status:*` label. An issue whose label does not match its real state is a triage defect.
- Surface uncertainty instead of filling gaps with guesses.
