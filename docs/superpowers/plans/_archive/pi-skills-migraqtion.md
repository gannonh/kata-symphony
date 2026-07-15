---
type: Plan
title: Pi Skills Migraqtion
description: Archived implementation plan: Pi Skills Migraqtion.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---


We are migrating Kata CLI away from its current implementation as a custom CLI built on top of the Pi coding agent.

Current state:

- Kata CLI is tightly coupled to the Pi coding agent.
- It includes custom frontend loading behavior plus Pi-specific extensions.
- The desktop app currently relies on this custom CLI layer via RPC mode.

Target state:

- Kata’s planning and execution capabilities become a set of Agent Skills, coupled with a standalone npm CLI that can run from any compatible harness.
- Target harnesses include Pi coding agent, Claude Code, OpenAI Codex, GitHub Copilot, and other similar agent environments that support the agent Skills standard.
- The harness-agnostic parts of the system should be expressed as reusable agent skills.
- Backend operations such as Linear and GitHub interactions should be handled through the Node-based CLI layer.
- The desktop app should continue using the Pi coding agent in RPC mode, but it should call Pi directly rather than going through the custom Kata CLI.
- Symphony will remain the primary autonomous project execution layer for Kata planned projects and will remain unchanged.

Kata Orchestrator:

- We currently have a product in the monorepo called Kata Orchestrator, an agent skills-based project planning and execution product : apps/orchestrator
- What is described here will be a replacement for that.
- We should leverage the structural and architectural components of orchestrator, but adapt it for GitHub and linear backends using the CLI layer instead of local markdown files.

What I want you to produce:

- A clear proposed architecture for the new Kata CLI and skill model.
- A separation of responsibilities between:
  - harness-agnostic agent skills
  - the Node-based backend CLI
  - harness-specific adapters or integration layers
  - the desktop app
- So long as the planned artifacts are unchanged, no migration, backwards compatability or fallbacks will be necessary.
- Recommended packaging and distribution strategy for the npm CLI and skills.
- Risks, tradeoffs, and compatibility concerns across different harnesses.

Important constraints:

- Optimize for portability across multiple agent harnesses (Agent Skills focused implementation).
- Preserve desktop support through direct Pi RPC integration.
- Ensure the resulting system supports both interactive use in external harnesses and autonomous execution through Symphony.
- Prefer industry-standard skill or tool patterns wherever practical.
