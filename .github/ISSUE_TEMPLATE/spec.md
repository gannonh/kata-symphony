---
name: Spec
about: Spec-driven work for plan-build-verify. GitHub Issues are the source of truth.
title: ""
labels: ["kind:spec", "status:draft", "phase:plan"]
---

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

## Delivery slices

1. <user-observable outcome>: end-to-end behavior, likely layers/files, acceptance tie-in, and demo
2. <next user-observable outcome>: end-to-end behavior, likely layers/files, acceptance tie-in, and demo

## Demonstration

- Consumer: <human, operator, or API/SDK client>
- Action or input: <what they do>
- Observable result: <what becomes visible or usable>
- Evidence: <how to exercise, inspect, or capture it>

<For an unavoidable technical-enablement exception, instead record the blocker, minimum scope, contract/integration evidence, and immediate user-facing slice unlocked.>

## Verification

<required public-boundary E2E command; additional unit/integration checks; required screenshot checkpoints for visual targets; preferred video recorder or expected environment limitation; manual UAT steps>

## Risks and mitigations

<specific risks with practical mitigations>

## Build handoff

- Approved scope: <...>
- Non-goals: <...>
- Ordered slices: <...>
- Required verification commands: <...>
- Fixtures or credentials needed: <...>
- Blocking open questions: None
