# Plan Workflow

Help turn ideas into fully formed design specs through natural collaborative dialogue.

Start by understanding the current project context, then ask questions one at a time to refine the idea. Once you understand what you're building, present the design and get user approval.

The result is a durable Markdown design spec in the local project at:

```text
docs/specs/YYYY-MM-DD-<topic>.md
```

Every written spec must include an explicit `## Acceptance criteria` section. Build and Verify depend on this section, so treat it as the spec's approval checklist, not optional supporting detail.

## Checklist

You MUST create a task for each of these items and complete them in order:

1. **Explore project context** — check files, docs, recent commits
2. **Ask clarifying questions** — one at a time, understand purpose/constraints/success criteria
3. **Propose 2-3 approaches** — with trade-offs and your recommendation
4. **Define acceptance criteria** — agree on observable pass/fail outcomes before writing the spec
5. **Present design** — in sections scaled to their complexity, get user approval after each section
6. **Write design spec** — save to `docs/specs/YYYY-MM-DD-<topic>-design.md`
7. **Adversarial spec review** — dispatch a separate sub-agent that did not write the spec to review it and provide feedback (see below)
8. **Validate reviewer feedback** — revise the spec when feedback is valid and actionable, or provide a reasoned rebuttal when it is not
9. **Finalize spec location** — move or save the finalized spec to the appropriate section/location and commit
10. **User reviews written spec** — ask user to review the spec file before proceeding
11. **Mark spec approved** — once the user explicitly approves, update frontmatter `status: Approved` and the top-of-document `## Status` section (see below)
12. **Transition to Build phase** — ask the user if they would like to advance to the Build phase (`./build.md`).

## Process Flow

```dot
digraph brainstorming {
    "Explore project context" [shape=box];
    "Ask clarifying questions" [shape=box];
    "Propose 2-3 approaches" [shape=box];
    "Define acceptance criteria" [shape=box];
    "Present design sections" [shape=box];
    "User approves design?" [shape=diamond];
    "Write design spec" [shape=box];
    "Adversarial spec review\n(separate sub-agent)" [shape=box];
    "Validate feedback\n(revise or rebut)" [shape=box];
    "Finalize spec location" [shape=box];
    "User reviews spec?" [shape=diamond];
    "Advance to Build phase" [shape=doublecircle];

    "Ask clarifying questions" -> "Propose 2-3 approaches";
    "Propose 2-3 approaches" -> "Define acceptance criteria";
    "Define acceptance criteria" -> "Present design sections";
    "Present design sections" -> "User approves design?";
    "User approves design?" -> "Present design sections" [label="no, revise"];
    "User approves design?" -> "Write design spec" [label="yes"];
    "Write design spec" -> "Adversarial spec review\n(separate sub-agent)";
    "Adversarial spec review\n(separate sub-agent)" -> "Validate feedback\n(revise or rebut)";
    "Validate feedback\n(revise or rebut)" -> "Finalize spec location";
    "Finalize spec location" -> "User reviews spec?";
    "User reviews spec?" -> "Write design spec" [label="changes requested"];
    "User reviews spec?" -> "Mark spec approved" [label="approved"];
    "Mark spec approved" [shape=box];
    "Mark spec approved" -> "Advance to Build phase";
}
```

**The terminal state is advancing to Build phase.** Do NOT invoke any other implementation skill. The ONLY workflow you invoke after Plan is Build.

## The Process

**Understanding the idea:**

- Check out the current project state first (files, docs, recent commits)
- Before asking detailed questions, assess scope: if the request describes multiple independent subsystems (e.g., "build a platform with chat, file storage, billing, and analytics"), flag this immediately. Don't spend questions refining details of a project that needs to be decomposed first.
- If the project is too large for a single spec, help the user decompose into a high level roadmap with sub-projects: what are the independent pieces, how do they relate, what order should they be built? Then brainstorm the first sub-project. Each sub-project gets its own spec.
- For appropriately-scoped projects, ask questions one at a time to refine the idea
- Prefer multiple choice questions when possible, but open-ended is fine too
- Only one question per message - if a topic needs more exploration, break it into multiple questions
- Focus on understanding: purpose, constraints, success criteria
- Convert success criteria into concrete acceptance criteria before drafting the spec. If the user gives vague outcomes such as "works well" or "feels fast", ask enough follow-up to make them observable.

**Exploring approaches:**

- Propose 2-3 different approaches with trade-offs
- Present options conversationally with your recommendation and reasoning
- Lead with your recommended option and explain why

**Presenting the design:**

- Once you believe you understand what you're building, present the design
- Scale each section to its complexity: a few sentences if straightforward, up to 200-300 words if nuanced
- Ask after each section whether it looks right so far
- Cover: architecture, components, acceptance criteria, data flow, error handling, testing
- Present acceptance criteria as their own design section and ask the user to approve or revise them before writing the spec.
- Be ready to go back and clarify if something doesn't make sense

**Design for isolation and clarity:**

- Break the system into smaller units that each have one clear purpose, communicate through well-defined interfaces, and can be understood and tested independently
- For each unit, you should be able to answer: what does it do, how do you use it, and what does it depend on?
- Can someone understand what a unit does without reading its internals? Can you change the internals without breaking consumers? If not, the boundaries need work.
- Smaller, well-bounded units are also easier for you to work with - you reason better about code you can hold in context at once, and your edits are more reliable when files are focused. When a file grows large, that's often a signal that it's doing too much.

**Working in existing codebases:**

- Explore the current structure before proposing changes. Follow existing patterns.
- Where existing code has problems that affect the work (e.g., a file that's grown too large, unclear boundaries, tangled responsibilities), include targeted improvements as part of the design - the way a good developer improves code they're working in.
- Don't propose unrelated refactoring. Stay focused on what serves the current goal.

## Spec status and frontmatter

Every written spec must track approval status in two places:

1. **YAML frontmatter** — `status` field at the top of the file
2. **Document body** — a `## Status` section immediately after the title and before other content

Use these status values during Plan:

- `Draft` — while writing, revising, or awaiting user review
- `Approved` — after explicit user approval of the written spec

When the user approves, update **both** places in the same edit. Do not advance to Build until both show `Approved`.

**Draft spec template:**

```markdown
---
type: Spec
title: <human-readable title>
description: <single-sentence summary>
status: Draft
---

# <Title>

## Status
Draft

## Goal
...
```

**After user approval**, update to:

```markdown
---
type: Spec
title: <human-readable title>
description: <single-sentence summary>
status: Approved
approved_at: <ISO 8601 datetime>
---

# <Title>

## Status
Approved

## Goal
...
```

Include OKF frontmatter fields (`type`, `title`, `description`) when the project uses OKF under `./docs`. Preserve any existing frontmatter keys when updating status.

If the user requests changes after approval, revert both frontmatter and `## Status` to `Draft`, remove `approved_at`, make the edits, re-run adversarial review if needed, and mark approved again only after the user re-approves.

## After the Design

**Documentation:**

- Write the validated design spec to `docs/specs/YYYY-MM-DD-<topic>-design.md`
  - (User preferences for spec location override this default)
- Make the spec implementation-ready. It should capture the agreed design, constraints, sequencing, implementation phases, risks, and verification guidance so the Build phase can proceed without re-planning.
- Use the section names, format, and level of detail below as guidance, not a rigid template. Omit sections that do not fit the work. Add sections when they make the spec easier to review or execute.
- Exception: `## Acceptance criteria` is mandatory. Do not omit, rename, or bury it under Testing/Verification. A spec without this exact section heading is invalid and must be fixed before commit, user review, or approval.
- Consider sections like:
  - `Goal` - the outcome and source of truth.
  - `Reference mocks`, `References`, or `Source of truth` - links to roadmap sections, screenshots, prior specs, designs, or examples.
  - `Current state` or `Verified current state` - what already exists, what is working, and what gaps remain.
  - `Constraints` and `Out of scope` - explicit boundaries, non-goals, and governing rules.
  - `Acceptance criteria` - numbered or checklist-style pass/fail outcomes that define when the work is complete.
  - `Architecture` - component relationships, boundaries, data flow, and a Mermaid diagram when relationships matter.
  - `Implementation phases` - ordered phases with the approach, likely files, and acceptance tie-ins for each phase.
  - `Sequencing` - dependency order, parallelizable work, and rollout notes.
  - `Verification` or `Testing and acceptance` - unit, integration, manual, UAT, and command-level checks.
  - `Risks and mitigations` - specific risks with practical mitigations.
  - `Key files` or `Suggested file map` - likely files or areas affected, without inventing unverified APIs.
  - `Explicitly deferred work` - related work intentionally left out.
  - `Build handoff` - approved scope, non-goals, ordered phases, required verification, fixtures, risks, and blocking questions.
- Keep implementation detail at the task-shaping level. Include phases, likely files, and acceptance checks; avoid large pasted code blocks or exact APIs that have not been verified in the repo.
- Set frontmatter `status: Draft` and add `## Status` / `Draft` at the top of the document body before committing for user review
- Commit the design spec to git

**Acceptance Criteria Section:**

The written spec must include this exact heading:

```markdown
## Acceptance criteria
```

Under it, include a numbered list or checklist. Each criterion must be:

- observable by a reviewer, test, command, screenshot, API response, or manual UAT step
- specific enough to produce a clear Pass/Fail/Blocked result during Verify
- tied to user-visible behavior, required system behavior, safety constraints, or approved non-goals
- free of vague language such as "fast", "easy", "robust", or "works" unless paired with an observable threshold or example

If acceptance criteria are genuinely unknown, stop and ask the user. Do not write a spec with missing, placeholder, or implied criteria.

**Adversarial Spec Review:**
After writing the spec document, dispatch a separate adversarial sub-agent that did not write the spec. The sub-agent reviews the spec and provides feedback. Do not replace this with a same-agent self-review. If no sub-agent mechanism is available, say so and ask the user how to proceed.

Ask the reviewer to challenge:

1. **Acceptance criteria gate:** Does the spec include an exact `## Acceptance criteria` heading with numbered or checklist-style criteria? Does each item have a clear verification path and pass/fail meaning?
2. **Placeholder scan:** Are there any "TBD", "TODO", incomplete sections, or vague requirements?
3. **Internal consistency:** Do any sections contradict each other? Does the architecture match the feature descriptions?
4. **Scope check:** Is this focused enough for a single implementation plan, or does it need decomposition?
5. **Ambiguity check:** Could any requirement be interpreted two different ways?
6. **Feasibility and verification:** Are there repo, dependency, testing, sequencing, or risk assumptions that need evidence?

The main agent validates the reviewer's feedback. Revise the spec when feedback is valid and actionable. When feedback is not valid or not actionable, leave the spec unchanged for that point and record a reasoned rebuttal in the planning notes or response. After accepted revisions are applied, move or save the finalized spec to the appropriate section/location and commit it.

**User Review Gate:**
After the adversarial review and main-agent validation pass, ask the user to review the written spec before proceeding:

> "Spec written and committed to `<path>`. Please review it and let me know if you want to make any changes."

Wait for the user's response. If they request changes, make them and re-run the adversarial spec review loop. Only proceed once the user approves.

**Mark spec approved:**

Once the user explicitly approves the written spec:

1. Update frontmatter `status` from `Draft` to `Approved`
2. Add `approved_at` with the current ISO 8601 datetime
3. Update the document body's `## Status` section from `Draft` to `Approved`
4. Commit the approval update to git

Confirm both locations show `Approved` before asking about Build.

**Build phase:**

- ask the user if they would like to advance to the Build phase (`references/build.md`).
- Do NOT invoke any other skill.

## Key Principles

- **One question at a time** - Don't overwhelm with multiple questions
- **Multiple choice preferred** - Easier to answer than open-ended when possible
- **YAGNI ruthlessly** - Remove unnecessary features from all designs
- **Explore alternatives** - Always propose 2-3 approaches before settling
- **Incremental validation** - Present design, get approval before moving on
- **Be flexible** - Go back and clarify when something doesn't make sense
