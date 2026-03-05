# Harness Engineering Reference (Adapted)

Last reviewed: 2026-03-05

Reference: OpenAI, "Harness engineering"
- [https://openai.com/index/harness-engineering/](https://openai.com/index/harness-engineering/)

## Why this matters for Symphony

Symphony is itself an orchestration harness. If repository legibility and evaluation discipline are weak, agent execution quality degrades and operational risk increases.

## Adapted Principles for this repo

1. Keep a concise repository map for agents (`AGENTS.md`).
2. Keep plans and design docs as first-class, versioned artifacts.
3. Enforce lightweight quality gates in CI.
4. Track reliability/security/quality posture explicitly.
5. Treat generated artifacts and context as disposable, documented outputs.

## Two-layer harness model

### Layer A: Build-time harness (this repository)

- Documentation layout and freshness checks
- Repeatable scripts for validation
- CI enforcement of baseline repo contract
- Clear traceability from spec -> plan -> implementation -> validation

### Layer B: Product-time harness (Symphony runtime)

- Runtime workflow contract (`WORKFLOW.md`)
- Scheduler/retry/reconciliation correctness
- Workspace safety and hook policy
- Observability and operator-facing monitoring surfaces (TUI first, web second)

