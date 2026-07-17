---
name: plan-build-verify
description: Use this skill for multi-step, spec-driven, or acceptance-gated implementation work that should move through Plan, Build, and Verify phases.
---

# Plan Build Verify

Use this skill to route implementation work through a sequential path:

1. **Plan**: align with the user on intent, constraints, approach, and acceptance criteria before writing a project-grounded spec and implementation plan.
2. **Build**: execute an approved spec or implementation plan.
3. **Verify**: validate completed work against the spec, plan, and acceptance criteria.

Most work should move through Plan → Build → Verify. Default to **Plan** unless the user explicitly directs you to execute an existing plan or verify completed work.

When the selected phase is **Plan**, do not jump straight to a written spec. Plan starts with context exploration and user alignment. The spec records the agreed direction after the user has responded to the alignment phase and approved or redirected the recommended approach.

## Phase contracts

| Phase  | Input                                                | Output                                                                                                                                                             | Status                                    |
| ------ | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| Plan   | Idea, vague request, or new build request            | Context exploration → alignment dialogue → approach approval → draft `docs/specs/YYYY-MM-DD-<topic>.md` with mandatory `## Acceptance criteria` and Build handoff → Approved after explicit user approval | `Idea` → `Aligned` → `Draft` → `Approved` |
| Build  | Approved spec, or explicit user override             | Implemented tasks, commits, review results, Build completion report                                                                                                | `Approved` → `Implemented`                |
| Verify | Implemented spec plus completed work or Build report | Acceptance evidence and signoff recommendation                                                                                                                     | `Implemented` → `Verified`                |

Build must not start from a draft spec unless the user explicitly overrides the approval gate. Verify must not claim signoff without evidence.

## Select the workflow

Determine the current phase from the user's request and project state. If the user gives a new build request, start with Plan even if they use the word “build”.

Use **Plan** when:

- The user asks to plan, scope, design, spec, shape, or prepare work.
- The user asks to build a new feature, product surface, integration, or architecture change without pointing to an approved plan.
- There is no approved spec or implementation plan for the requested work.
- The safest next step is interactive alignment and a Markdown spec before coding.

Examples that start with Plan:

- `/plan-build-verify "Let's build an analytics dashboard for foo bar."`
- `/plan-build-verify "Add API-backed project creation."`
- `/plan-build-verify "We need a Slack integration for agent updates."`

Use **Build** when:

- The user asks to execute, implement, build, or continue from an approved spec or plan.
- The request points to an existing plan/spec and asks for code changes.
- The current task is implementation rather than discovery or acceptance review.

Examples that start with Build:

- `/plan-build-verify "Execute this plan: docs/specs/foo-plan.md"`
- `/plan-build-verify "Implement docs/specs/2026-05-21-analytics-dashboard.md"`
- `/plan-build-verify "Continue the approved billing settings plan."`

Use **Verify** when:

- The user asks to verify, validate, review, test, sign off, run UAT, or prove work is complete.
- Implementation appears done and the next step is acceptance evidence.
- The user asks whether a branch, feature, or plan is ready to merge or hand off.

Examples that start with Verify:

- `/plan-build-verify "Run UAT on foobar."`
- `/plan-build-verify "Verify the analytics dashboard work is complete."`
- `/plan-build-verify "Check whether this branch is ready to merge."`

For tiny, clearly bounded edits such as a copy change or single config tweak, do not force the full Plan workflow. State the assumption and ask whether the user wants the full Plan → Build → Verify process.

If the phase remains ambiguous after applying these rules, ask one focused question that resolves the phase.

## Load the workflow instructions

After selecting the workflow, read the corresponding reference file completely and follow it. For Plan, the first reference step after context exploration is alignment. If you are about to draft a spec before the user has answered an alignment question or approved a recommended direction, stop and ask instead.

Reference files:

- Plan: `references/plan.md`
- Build: `references/build.md`
- Verify: `references/verify.md`

Only load the workflow you need. If the selected workflow's reference file is empty or incomplete, say so and ask the user whether to draft that workflow before proceeding.

## Bundled phase references

Build and Verify load bundled reference workflows from this skill directory. Read the entry point completely; follow linked files and scripts under the same subtree.

| Phase  | Workflow | Entry point |
| ------ | -------- | ----------- |
| Build  | TDD | `references/tdd/workflow.md` |
| Verify | User acceptance | `references/user-acceptance/workflow.md` |

Scripts for user-acceptance evidence live under `scripts/user-acceptance/`.

## Shared principles

- Inspect the repo before making claims about project structure or commands.
- Use a todo list when the work has multiple steps.
- In Plan, ask focused alignment questions one at a time before drafting the spec. If no factual clarification is needed, ask the user to confirm your framing, assumptions, acceptance criteria, success criteria, or recommended direction.
- In Plan, propose 2-3 approaches before settling when more than one viable direction exists. If one approach is clearly best, state the recommendation and wait for approval or redirection.
- Prefer small, verifiable phases over broad unverified changes.
- Keep scope tied to the selected spec, plan, or acceptance criteria.
- A Plan spec is incomplete unless it has an exact `## Acceptance criteria` section with observable pass/fail criteria that Build can implement and Verify can test.
- Plan specs track approval in frontmatter `status` and a top-of-document `## Status` section. Write both as `Draft` until user review; once the user approves, update both to `Approved` before Build.
- Surface uncertainty instead of filling gaps with guesses.
