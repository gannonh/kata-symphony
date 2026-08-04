---
name: okf
description: Create, read, and maintain Open Knowledge Format documentation bundles in repositories. Use this skill when the user asks for /okf, OKF, Open Knowledge Format, docs-as-knowledge, reading project context from docs, reorganizing docs into an agent-readable bundle, updating docs/specs/ADRs after a session or PR, or adding AGENTS.md instructions for agents to consume and maintain the docs bundle. Supports /okf read, /okf init, and /okf update workflows.
---

# OKF

Use this skill to read, initialize, or update an Open Knowledge Format bundle for a repository.

Before editing, read `references/SPEC.md` from this skill directory. Treat that file as the source of truth for OKF v0.1 conformance.

When validating a bundle, use the bundled helper when available:

```bash
python <skill-dir>/scripts/validate_okf.py <repo-root>
```

Use `--strict-links` when broken local Markdown links should fail validation instead of warn.

## Core idea

An OKF bundle is a directory tree of Markdown files with YAML frontmatter for concept documents, plus `index.md` and `log.md` files for navigation and history. In repositories, the bundle root is `./docs` unless the user explicitly chooses another path.

The skill has three workflows:

- `/okf read`: read the OKF bundle to load project context before planning or changing code.
- `/okf init`: reorganize existing project documentation into an OKF-compliant `./docs` bundle and add AGENTS.md instructions for future agents.
- `/okf update`: update the OKF bundle at the end of a session, feature branch, or PR so the docs reflect completed work.

## Required OKF shape for this skill

Create and maintain these sections in every repository OKF bundle:

```text
./docs/
├── index.md          # Roadmap and top-level progressive-disclosure index
├── log.md            # Chronological bundle update history, newest first
├── specs/            # Product specs, technical plans, issue plans, roadmaps
│   ├── index.md      # Specs roadmap and current work map
│   └── log.md        # Specs update history
└── adrs/             # Architecture Decision Records
    ├── index.md      # ADR listing grouped by accepted/proposed/superseded if known
    └── log.md        # ADR update history
```

`./docs/specs/index.md` is the roadmap. It should make active, planned, blocked, and completed work easy to scan.

`./docs/adrs` is required even if the repo has no ADRs yet. Seed it with `index.md` and `log.md` so future decisions have a stable home.

## Flexible sections

Create additional sections when the repository has durable knowledge that belongs there. Common examples:

- `architecture/` for system maps, boundaries, modules, data flow, and integration notes.
- `guides/` for contributor, setup, release, deployment, and operational guides.
- `reference/` for APIs, schemas, config, CLI commands, generated references, and external-source mirrors.
- `runbooks/` for troubleshooting, incidents, maintenance, on-call, and recovery procedures.
- `domains/` for product concepts, business concepts, terminology, metrics, and domain models.
- `research/` for exploration notes that should remain discoverable but are not committed plans.

Use the repo's existing language. Do not force all optional sections into every project.

## Concept document rules

Every Markdown file under `./docs` is a concept document unless its filename is `index.md` or `log.md`.

For concept documents, include YAML frontmatter:

```yaml
---
type: <short descriptive type>
title: <human display title>
description: <single sentence summary>
tags: [<tag>, <tag>]
timestamp: <ISO 8601 datetime>
---
```

Only `type` is required by OKF, but include `title` and `description` whenever you can infer them confidently. Preserve unknown frontmatter keys when updating an existing concept.

Use type names that explain what the document is, for example:

- `Spec`
- `Plan`
- `ADR`
- `Guide`
- `Runbook`
- `Reference`
- `Architecture Note`
- `Domain Concept`
- `Research Note`

## Cross-linking

OKF uses standard Markdown links between concepts to express relationships. Create and maintain these cross-links deliberately so agents can traverse related knowledge.

Use cross-links for durable relationships such as:

- specs that implement or supersede other specs
- ADRs that decide, constrain, or supersede specs and architecture notes
- runbooks that operate systems described by architecture or reference concepts
- guides that explain concepts in `domains/`, `reference/`, or `architecture/`
- roadmap items that point to their current spec, ADR, or delivery notes

Prefer bundle-relative absolute links, such as `/specs/auth-redesign.md`, when linking within the OKF bundle. Use relative links only when they are clearer and stable.

Add links in both directions when both concepts benefit from traversal. For example, a spec should link to the ADR that constrains it, and the ADR should link back to the affected spec or architecture concept.

Do not invent relationships. If a link target is plausible but unsupported by the repo or user context, leave it out or ask for clarification.

`index.md` files normally contain no frontmatter. The bundle-root `./docs/index.md` may include frontmatter only when declaring the OKF version, for example:

```yaml
---
okf_version: "0.1"
---
```

## `/okf read` workflow

Use this read-only workflow when the user asks to load OKF context, understand the project before work, inspect the roadmap, review decisions, or prepare for a task in a repo that already has an OKF bundle.

1. Locate the OKF bundle.
   - Use `./docs` unless the user names another bundle root.
   - If `./docs/index.md` is missing, report that no OKF bundle was found and suggest `/okf init` if appropriate.
   - Do not edit files, update logs, or create new documents during `/okf read`.

2. Read the top-level map.
   - Read `./docs/index.md`.
   - Read `./docs/specs/index.md` when present because it is the roadmap.
   - Read `./docs/adrs/index.md` when present to identify relevant decisions.
   - Skim other section indexes that appear relevant to the user's task, such as `architecture/`, `guides/`, `reference/`, `runbooks/`, or `domains/`.

3. Follow relevant cross-links.
   - Follow Markdown links from the roadmap and relevant indexes into specs, ADRs, architecture notes, guides, runbooks, reference docs, and domain concepts.
   - Prefer documents directly related to the user's task, changed files, named features, architecture areas, APIs, migrations, or operational concerns.
   - Stop when the context is sufficient for the requested work. Do not exhaustively read the whole bundle unless the user asks for a full audit.

4. Report the loaded context.
   - Summarize active roadmap items relevant to the task.
   - List relevant specs, ADRs, runbooks, guides, references, and domain concepts read.
   - Call out constraints, accepted decisions, open questions, stale links, missing docs, or docs gaps.
   - Recommend the next source files or docs to inspect before implementation.

## `/okf init` workflow

Use this workflow when the repo does not already have a coherent OKF bundle or the user asks to reorganize existing documentation.

1. Confirm the repo state.
   - Run `git status --short`.
   - If there are unrelated dirty changes, ask before moving or rewriting docs.
   - Identify the default branch if you need to compare historical docs.

2. Inventory existing documentation.
   - Search for Markdown docs in `./docs`, root-level files, and common documentation locations such as `adr/`, `adrs/`, `spec/`, `specs/`, `plans/`, `design/`, `architecture/`, `notes/`, `.github/`, and package subdirectories.
   - Include root `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, and similar project-level files in the inventory, but do not move root files that tools or humans expect at the repo root unless the user asks.
   - Read enough content to classify each document by purpose and freshness.
   - If you are uncertain whether a file should be migrated into OKF format or moved into an OKF location, ask the user for clarification before moving or rewriting it.

3. Plan the bundle shape.
   - Use the required sections above.
   - Add optional sections only when existing docs or repo needs justify them.
   - Prefer `git mv` for file moves when the repo is under git.
   - Preserve useful history and content. Avoid deleting docs unless they are duplicates and the retained location is clear.

4. Normalize documents into OKF concepts.
   - Add missing frontmatter to non-reserved Markdown files under `./docs`.
   - Convert plans and specs into `./docs/specs/`.
   - Convert ADRs into `./docs/adrs/`.
   - Create cross-links between related specs, ADRs, guides, runbooks, reference docs, architecture notes, and roadmap entries.
   - Use bundle-relative links from the docs root when stable, such as `/specs/my-plan.md`.
   - Fix links affected by moves when practical.

5. Create indexes and logs.
   - Write `./docs/index.md` as the top-level roadmap and navigation page.
   - Write `./docs/specs/index.md` as the roadmap for planned and completed work.
   - Write `index.md` for every created optional section.
   - Create `log.md` files at the root, `specs/`, `adrs/`, and other sections that receive substantive changes.
   - Log initialization with an ISO `YYYY-MM-DD` heading and concise bullets.

6. Update AGENTS.md.
   - Add clear instructions that agents should read `./docs/index.md` before substantial work.
   - Tell agents to follow links into relevant specs, ADRs, runbooks, and domain docs.
   - Tell agents to run `/okf update` or manually update the OKF bundle after substantial work, PRs, architecture decisions, behavior changes, or docs-moving work.
   - Tell agents to keep `./docs/specs/index.md` as the roadmap and add ADRs for durable architecture decisions.
   - Keep project-specific AGENTS.md instructions intact.

7. Validate.
   - Run `python <skill-dir>/scripts/validate_okf.py <repo-root>` when this skill's helper script is available.
   - Check every non-reserved Markdown file under `./docs` has parseable YAML frontmatter with non-empty `type`.
   - Check reserved `index.md` and `log.md` files match OKF structure.
   - Check important moved links.
   - Run available docs lint or tests if the repo has them.

8. Report.
   - Summarize exactly what changed, including files moved, files created, files updated, and files intentionally left in place.
   - List AGENTS.md instruction changes.
   - Note validation performed and any remaining docs gaps or user follow-ups.

## `/okf update` workflow

Use this workflow near the end of a session, feature branch, or PR.

1. Determine what changed.
   - Review `git status --short` and relevant diffs.
   - Read recent commits or PR context if available.
   - Identify new behavior, public APIs, architecture decisions, migrations, operational changes, product decisions, and completed or changed plans.

2. Read the current OKF map.
   - Read `./docs/index.md`.
   - Read `./docs/specs/index.md`.
   - Read relevant ADRs, specs, runbooks, or domain docs before editing.

3. Update the right concepts.
   - Update existing documents when the concept already exists.
   - Create new concept documents only for durable knowledge worth preserving.
   - Add or update ADRs for architecture decisions with lasting consequences.
   - Update specs when scope, status, acceptance criteria, rollout, or implementation details changed.
   - Update runbooks, guides, or reference docs when operators or future agents need the information.

4. Maintain navigation and cross-links.
   - Update affected `index.md` files so readers can discover new or changed concepts.
   - Keep `./docs/specs/index.md` current as the roadmap.
   - Add or refresh cross-links between related specs, ADRs, guides, runbooks, reference docs, architecture notes, and roadmap entries.
   - Add reciprocal links when both documents benefit from traversal.

5. Maintain logs.
   - Add a newest-first entry to the nearest relevant `log.md` files.
   - Include the date, what changed, and links to updated concepts.
   - Keep log entries factual and brief.

6. Validate.
   - Run `python <skill-dir>/scripts/validate_okf.py <repo-root>` when this skill's helper script is available.
   - Re-check OKF conformance for changed docs.
   - Run docs lint/tests when available.

7. Report.
   - Summarize exactly what changed, including concepts created, concepts updated, indexes refreshed, and logs updated.
   - Note validation performed and any remaining docs gaps or user follow-ups.

## AGENTS.md snippet

Adapt this snippet during `/okf init`:

```markdown
## Open Knowledge Format docs

This repository maintains an [OKF](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md) bundle at `./docs`.

- Use `/okf read` when available, or read `./docs/index.md` directly before substantial work, to understand the current documentation map.
- Follow cross-links into relevant specs, ADRs, runbooks, guides, architecture notes, reference docs, and domain docs before changing related code.
- Keep `./docs/specs/index.md` current as the roadmap for active, planned, blocked, and completed work.
- Add or update ADRs in `./docs/adrs` for durable architecture decisions.
- After substantial work, PRs, behavior changes, architecture decisions, migrations, or documentation moves, update the OKF bundle and add concise entries to the relevant `log.md` files.
- Maintain Markdown cross-links between related OKF concepts so future agents can traverse decisions, specs, architecture, runbooks, guides, and references.
- Every non-reserved Markdown file under `./docs` should have OKF frontmatter with at least a non-empty `type` field. `index.md` and `log.md` are reserved navigation/history files.
```

## Helper scripts

- `scripts/validate_okf.py`: validates the required OKF structure, concept frontmatter, reserved `index.md` and `log.md` rules, log date headings, and local Markdown links.

## Validation checklist

Before finishing, verify:

- `./docs/index.md` exists and points to the main sections.
- `./docs/log.md` exists and has newest-first `YYYY-MM-DD` entries.
- `./docs/specs/index.md` exists and serves as the roadmap.
- `./docs/specs/log.md` exists.
- `./docs/adrs/index.md` exists.
- `./docs/adrs/log.md` exists.
- Every non-reserved `./docs/**/*.md` file has frontmatter with `type`.
- Moved or newly created docs have useful `title` and `description` fields when known.
- Important cross-links still resolve or are intentionally forward references.
- New or changed durable relationships are represented with Markdown links between relevant concepts.
- AGENTS.md tells future agents how to consume and maintain the OKF bundle.

## When to ask the user

Ask before:

- Moving root-level files that common tools expect, such as `README.md`, `CONTRIBUTING.md`, or `AGENTS.md`.
- Deleting documentation instead of preserving or superseding it.
- Inventing product direction, architecture rationale, or roadmap commitments that are not supported by the repo or the user's instructions.
- Choosing between two plausible documentation taxonomies when the choice affects long-term navigation.
