# Symphony Security Posture

Last reviewed: 2026-03-05

## Scope

Security and operational safety posture for the orchestration harness.

## Core Principles

1. Explicitly document approval and sandbox defaults.
2. Keep workspace execution contained to configured root.
3. Treat hooks and generated prompts as untrusted inputs.
4. Keep secrets out of logs and persisted workspace artifacts.

## Baseline Controls

- Configurable Codex approval/sandbox settings via `WORKFLOW.md`
- Path containment checks before launching agent subprocesses
- Hook timeouts and failure handling per spec
- Structured logging with sensitive value redaction policy

## Deferred Hardening Candidates

- External sandboxing (container/VM) for workspace execution
- Egress policy controls for agent subprocesses
- Additional policy guards for dynamic tool calls

