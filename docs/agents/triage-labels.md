---
type: Guide
title: Triage labels
description: Maps triage intake and software-factory route roles to this repo's GitHub labels.
tags: [triage, labels, agents]
timestamp: 2026-07-15T20:00:00Z
---

# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Edit the right-hand column to match whatever vocabulary you actually use.

## Symphony A1 route labels

The [A1 GitHub Issue Triage design](/specs/archive/2026-07-16-a1-github-issue-triage-design.md) uses `needs-triage` as its intake label and maps each completed triage artifact to one route label:

| Canonical route | Default GitHub label | Meaning |
| --- | --- | --- |
| `implement` | `ready-for-agent` | Ready for Symphony's implementation flow |
| `spec` | `ready-to-spec` | Requires product or technical specification |
| `needs_information` | `needs-info` | Waiting for a specific human answer |
| `park` | `wait-to-implement` | Valid work intentionally deferred |
| `human_owned` | `ready-for-human` | Requires human implementation |

`wontfix` remains a maintainer-owned terminal triage decision. A1 does not emit it automatically.
