# Plan Workflow (GitHub)

Help turn ideas into fully formed design specs through natural collaborative dialogue, then publish the spec as a GitHub Issue.

Start by understanding the current project context, then ask questions one at a time to refine the idea. Once you understand what you're building, present the design and get user approval.

The result is a GitHub Issue labeled `kind:spec` whose body is the spec. Read `references/github-conventions.md` before starting.

Every spec issue must include an explicit `## Acceptance criteria` section written as task-list checkboxes. Build and Verify depend on this section, so treat it as the spec's approval checklist, not optional supporting detail.

## Checklist

You MUST create a task for each of these items and complete them in order:

1. **Run preflight** — `gh` auth, target repo, labels, existing-issue search
2. **Explore project context** — files, docs, recent commits, related issues and PRs
3. **Ask clarifying questions** — one at a time, understand purpose/constraints/success criteria
4. **Propose 2-3 approaches** — with trade-offs and your recommendation
5. **Define acceptance criteria** — agree on observable pass/fail outcomes before writing the spec
6. **Present design** — in sections scaled to their complexity, get user approval after each section
7. **Publish spec issue** — compose the body in a temp file, `gh issue create`, delete the temp file
8. **Decompose into sub-issues** — when the spec has more than one independently buildable phase
9. **Adversarial spec review** — dispatch a separate sub-agent that did not write the spec to review the published issue
10. **Validate reviewer feedback** — revise the issue when feedback is valid and actionable, or provide a reasoned rebuttal when it is not
11. **User reviews the issue** — ask the user to read the issue before proceeding
12. **Mark spec approved** — once the user explicitly approves, swap `status:draft` for `status:approved` and update the `## Status` section
13. **Transition to Build phase** — ask the user if they would like to advance to Build (`./build.md`)

## Process Flow

```dot
digraph plan {
    "Preflight (gh, labels, dupes)" [shape=box];
    "Explore project context" [shape=box];
    "Ask clarifying questions" [shape=box];
    "Propose 2-3 approaches" [shape=box];
    "Define acceptance criteria" [shape=box];
    "Present design sections" [shape=box];
    "User approves design?" [shape=diamond];
    "Publish spec issue (status:draft)" [shape=box];
    "Needs decomposition?" [shape=diamond];
    "Create sub-issues" [shape=box];
    "Adversarial spec review\n(separate sub-agent)" [shape=box];
    "Validate feedback\n(revise or rebut)" [shape=box];
    "User reviews issue?" [shape=diamond];
    "Mark spec approved" [shape=box];
    "Advance to Build phase" [shape=doublecircle];

    "Preflight (gh, labels, dupes)" -> "Explore project context";
    "Explore project context" -> "Ask clarifying questions";
    "Ask clarifying questions" -> "Propose 2-3 approaches";
    "Propose 2-3 approaches" -> "Define acceptance criteria";
    "Define acceptance criteria" -> "Present design sections";
    "Present design sections" -> "User approves design?";
    "User approves design?" -> "Present design sections" [label="no, revise"];
    "User approves design?" -> "Publish spec issue (status:draft)" [label="yes"];
    "Publish spec issue (status:draft)" -> "Needs decomposition?";
    "Needs decomposition?" -> "Create sub-issues" [label="yes"];
    "Needs decomposition?" -> "Adversarial spec review\n(separate sub-agent)" [label="no"];
    "Create sub-issues" -> "Adversarial spec review\n(separate sub-agent)";
    "Adversarial spec review\n(separate sub-agent)" -> "Validate feedback\n(revise or rebut)";
    "Validate feedback\n(revise or rebut)" -> "User reviews issue?";
    "User reviews issue?" -> "Validate feedback\n(revise or rebut)" [label="changes requested"];
    "User reviews issue?" -> "Mark spec approved" [label="approved"];
    "Mark spec approved" -> "Advance to Build phase";
}
```

**The terminal state is advancing to Build phase.** Do NOT invoke any other implementation skill. The ONLY workflow you invoke after Plan is Build.

## The Process

**Preflight:**

- Run the preflight from `references/github-conventions.md`: `gh auth status`, resolve the target repo, ensure labels.
- Search for an existing issue covering this work before designing anything new:

```bash
gh issue list --label kind:spec --state open --json number,title,labels
gh search issues --repo <owner>/<repo> "<keywords>" --state all
```

- If an open issue already covers the request, ask whether to extend that issue instead of opening a new one. Extending means editing the existing body and returning it to `status:draft`.
- If a closed issue covers it, read it before proposing anything. Prior scope decisions are context.

**Understanding the idea:**

- Check out the current project state first (files, docs, recent commits, open PRs, related issues)
- Before asking detailed questions, assess scope: if the request describes multiple independent subsystems (e.g., "build a platform with chat, file storage, billing, and analytics"), flag this immediately. Don't spend questions refining details of a project that needs to be decomposed first.
- If the project is too large for a single spec, help the user decompose it. In this skill decomposition produces an epic issue with sub-issues, not separate unrelated specs. See "Decomposition" below.
- For appropriately-scoped projects, ask questions one at a time to refine the idea
- Prefer multiple choice questions when possible, but open-ended is fine too
- Only one question per message. If a topic needs more exploration, break it into multiple questions
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
- Present acceptance criteria as their own design section and ask the user to approve or revise them before publishing the issue
- Be ready to go back and clarify if something doesn't make sense

**Design for isolation and clarity:**

- Break the system into smaller units that each have one clear purpose, communicate through well-defined interfaces, and can be understood and tested independently
- For each unit, you should be able to answer: what does it do, how do you use it, and what does it depend on?
- Can someone understand what a unit does without reading its internals? Can you change the internals without breaking consumers? If not, the boundaries need work.
- Smaller, well-bounded units are also easier for you to work with. You reason better about code you can hold in context at once, and your edits are more reliable when files are focused. When a file grows large, that's often a signal that it's doing too much.

**Working in existing codebases:**

- Explore the current structure before proposing changes. Follow existing patterns.
- Where existing code has problems that affect the work (e.g., a file that's grown too large, unclear boundaries, tangled responsibilities), include targeted improvements as part of the design, the way a good developer improves code they're working in.
- Don't propose unrelated refactoring. Stay focused on what serves the current goal.

## Publishing the spec issue

After the user approves the design conversationally:

1. Compose the body in a temporary file using the spec issue body template from `references/github-conventions.md`.
2. Set the `## Status` section to `Draft`.
3. Write acceptance criteria as `- [ ]` checkboxes.
4. Create the issue:

```bash
BODY="$(mktemp -t pbvg-spec).md"
# write the spec body to "$BODY"
gh issue create \
  --title "<concise outcome-oriented title>" \
  --body-file "$BODY" \
  --label "kind:spec,status:draft,phase:plan"
rm -f "$BODY"
```

5. Add any existing repo labels for area, component, or priority that apply. Do not invent new ones.
6. Assign a milestone only if the repo uses milestones and the user names one.
7. Report the issue number and URL to the user.

Title guidance: state the outcome, not the activity. `Export workflow for saved reports` rather than `Add export`. Keep it under 70 characters so it reads well in list views.

Never paste a multi-line spec body into `--body`. Always use `--body-file`.

## Decomposition

Decompose when the spec has more than one independently buildable and verifiable phase, when phases have different acceptance criteria, or when the user asks.

Structure: **parent holds the outcome, children hold the phases.**

1. Add `kind:epic` to the parent and keep goal, context, constraints, architecture, risks, and top-level acceptance criteria in its body.
2. Replace the parent's `## Implementation phases` section with a short list naming each phase and its sub-issue number once created.
3. For each phase, compose a child body with its own `## Status`, `## Goal`, `## Context` (linking the parent), `## Acceptance criteria`, and `## Build handoff`, then:

```bash
gh sub-issue create --parent <PARENT> \
  --title "Phase 1: <name>" \
  --body-file "$BODY" \
  --label "kind:sub-spec,status:draft"
rm -f "$BODY"
```

4. Order children by dependency. State the dependency explicitly in each child's `## Context` ("Depends on #143").
5. Verify the hierarchy with `gh sub-issue list <PARENT>`.

Approval flows to children: when the user approves the epic, move the parent and all children to `status:approved` unless the user approves only part of the plan.

Do not nest more than one level. If a child needs its own decomposition, the parent scope was wrong. Return to alignment.

If `gh sub-issue` is unavailable, use the task-list fallback from `references/github-conventions.md` and say so.

## Acceptance criteria section

The issue body must include this exact heading:

```markdown
## Acceptance criteria
```

Under it, a checkbox list. Each criterion must be:

- observable by a reviewer, test, command, screenshot, API response, or manual UAT step
- specific enough to produce a clear Pass/Fail/Blocked result during Verify
- tied to user-visible behavior, required system behavior, safety constraints, or approved non-goals
- free of vague language such as "fast", "easy", "robust", or "works" unless paired with an observable threshold or example

If acceptance criteria are genuinely unknown, stop and ask the user. Do not publish an issue with missing, placeholder, or implied criteria. If the user directs you to publish before criteria are settled, add the `needs:acceptance-criteria` label so Triage and the Build preflight catch it.

## Adversarial spec review

After publishing the issue, dispatch a separate adversarial sub-agent that did not write the spec. Give it the issue number and tell it to read the issue with `gh issue view <N>`. Do not replace this with a same-agent self-review. If no sub-agent mechanism is available, say so and ask the user how to proceed.

Use `references/spec-reviewer-prompt.md` for the reviewer brief. Ask the reviewer to challenge:

1. **Acceptance criteria gate:** Does the issue include an exact `## Acceptance criteria` heading with checkbox criteria? Does each item have a clear verification path and pass/fail meaning?
2. **Placeholder scan:** Are there any "TBD", "TODO", incomplete sections, or vague requirements?
3. **Internal consistency:** Do any sections contradict each other? Does the architecture match the feature descriptions?
4. **Scope check:** Is this focused enough for a single spec, or does it need decomposition into sub-issues?
5. **Ambiguity check:** Could any requirement be interpreted two different ways?
6. **Feasibility and verification:** Are there repo, dependency, testing, sequencing, or risk assumptions that need evidence?
7. **Sub-issue coherence:** If decomposed, do the children cover the parent's acceptance criteria completely, with no gaps and no overlap?

The main agent validates the reviewer's feedback. Revise the issue body when feedback is valid and actionable:

```bash
BODY="$(mktemp -t pbvg-spec).md"
gh issue view <N> --json body -q .body > "$BODY"
# apply revisions to "$BODY"
gh issue edit <N> --body-file "$BODY"
rm -f "$BODY"
```

When feedback is not valid or not actionable, leave the body unchanged for that point and record a reasoned rebuttal as an issue comment so the decision stays with the spec.

## User review gate

After the adversarial review and main-agent validation pass, ask the user to review the issue:

> "Spec published as #<N>: <url>. Please review it and let me know if you want to make any changes."

Wait for the user's response. If they request changes, make them and re-run the adversarial review loop. Only proceed once the user approves.

## Mark spec approved

Once the user explicitly approves:

1. Update the body's `## Status` section from `Draft` to `Approved` and add an `Approved: <ISO 8601 datetime>` line beneath it.
2. Swap the labels:

```bash
gh issue edit <N> \
  --remove-label "status:draft,phase:plan" \
  --add-label "status:approved"
```

3. For an epic, apply the same transition to every approved child.
4. Confirm with `gh issue view <N> --json labels,body` that the label and the `## Status` section agree.

## Build phase

- Ask the user if they would like to advance to the Build phase (`references/build.md`).
- For an epic, name the first sub-issue Build should start with.
- Do NOT invoke any other skill.

## Key Principles

- **One question at a time** — Don't overwhelm with multiple questions
- **Multiple choice preferred** — Easier to answer than open-ended when possible
- **Search before creating** — Duplicate spec issues fragment the roadmap
- **YAGNI ruthlessly** — Remove unnecessary features from all designs
- **Explore alternatives** — Always propose 2-3 approaches before settling
- **Incremental validation** — Present design, get approval before moving on
- **The issue is the spec** — No spec content stays in the working tree
- **Be flexible** — Go back and clarify when something doesn't make sense
