# Specs

Specs for this project are GitHub Issues. This directory holds no spec documents.

## Read the roadmap

```bash
gh issue list --label kind:spec --state open            # all active specs
gh issue list --label status:approved --state open      # approved, ready to build
gh issue list --label status:implemented --state open   # built, awaiting verification
gh issue view <N>                                       # read a spec
gh sub-issue list <N>                                   # read an epic's phases
```

## Status model

| Label                | Meaning                                     |
| -------------------- | ------------------------------------------- |
| `status:draft`       | Being written or revised. Do not build.     |
| `status:approved`    | Approved by the maintainer. Ready to build. |
| `status:implemented` | Built and reported. Ready to verify.        |
| `status:verified`    | Acceptance evidence accepted.               |
| `status:blocked`     | Cannot proceed. See the issue body.         |

## Writing and executing specs

Use the `plan-build-verify-github` skill. It publishes specs as issues, runs Build
against approved issues, and posts acceptance evidence back to the issue.

## Archive

Pre-migration spec files are preserved under [`archive/`](./archive/) with links to
their issues. Completed specs were archived without an issue. Both are history and
are not maintained.
