# Verify Workflow (GitHub)

Use this workflow to validate completed implementation against the spec issue's acceptance criteria, and to post the acceptance evidence back to the issue.

Verify is the third phase in Plan → Build → Verify. It produces acceptance evidence, checked acceptance criteria, and a signoff recommendation. Read `references/github-conventions.md` before starting.

## Step 1: Gather inputs

```bash
gh issue view <N> --json number,title,body,labels,state,url,comments
gh sub-issue list <N>          # if the issue is an epic
gh pr list --search "<N>" --state all --json number,title,state,url
```

Collect:

- The spec issue, ideally labeled `status:implemented`.
- The acceptance criteria checkbox list from the issue body.
- The Build completion report comment.
- The branch or PR to verify.

Run the phase-entry hygiene check from `references/github-conventions.md` and report findings. If acceptance criteria are missing or ambiguous, stop, add `needs:acceptance-criteria`, and return to Plan.

Mark the phase:

```bash
gh issue edit <N> --add-label "phase:verify"
```

If the user asks for UAT, signoff, merge readiness, or proof that work is complete, start here.

## Step 2: Verify against acceptance criteria

- Read `references/user-acceptance/workflow.md` completely and follow it to verify the **Acceptance criteria** from the issue body.
- Follow that workflow for evidence capture, UAT reporting, screenshots, recordings, command output, and human test guides.
- Use scripts under `scripts/user-acceptance/` when the workflow calls for them.
- Use an explicit **acceptance-criteria matrix** in the final report.
- Each criterion shows the verification method, result (`Pass`, `Fail`, `Blocked`, or `Not tested`), and evidence path or note. This keeps Verify tied to the approved scope instead of producing only a general UAT summary.

Evidence artifacts land under `uat-evidence/<target>-<timestamp>/` as described in that workflow. They stay on disk; `gh` cannot upload binaries to an issue. Reference their paths in the comment and tell the user which artifacts are worth attaching by hand.

## Step 3: Verify the report and fill in gaps

- Task a subagent to verify the `uat-evidence` against the issue's acceptance criteria.
- Give the subagent the issue number so it can read the criteria with `gh issue view <N>` rather than trusting your summary.
- Fill in any gaps before posting the report.

## Step 4: Post the acceptance evidence

```bash
MATRIX="$(mktemp -t pbvg-verify).md"
# write the report to "$MATRIX"
gh issue comment <N> --body-file "$MATRIX"
rm -f "$MATRIX"
```

Start the comment with `## Verify: acceptance criteria matrix` and include:

- Scope line naming the branch, PR, or commit range verified.
- The matrix: criterion, method, result, evidence path.
- Totals (`6 Pass / 0 Fail / 1 Blocked`).
- Failures and blocked items with what would unblock them.
- Unrelated validation failures, separated from in-scope failures.
- Manual run instructions a human can follow.
- Recommendation line: `Pending user sign-off` until the user accepts.

Post the same matrix as a PR comment when a PR exists.

## Step 5: Check off the acceptance criteria

For each criterion that passed, check its box in the issue body so the issue shows live acceptance progress:

```bash
BODY="$(mktemp -t pbvg-body).md"
gh issue view <N> --json body -q .body > "$BODY"
# flip "- [ ]" to "- [x]" for passing criteria only
gh issue edit <N> --body-file "$BODY"
rm -f "$BODY"
```

Rules:

- Only check criteria with evidence in the matrix. A criterion checked without evidence is a false acceptance record.
- Leave failing, blocked, and untested criteria unchecked.
- Never edit the wording of a criterion during Verify. If a criterion is wrong, say so and return to Plan.

## Step 6: Signoff

Present the recommendation to the user and wait for explicit acceptance. Do not self-accept.

Once the user accepts:

```bash
gh issue edit <N> \
  --remove-label "status:implemented,phase:verify" \
  --add-label "status:verified"
```

Update the body's `## Status` section to `Verified` with a `Verified: <ISO 8601 datetime>` line.

Close the issue, unless a merged PR already closed it:

```bash
gh issue close <N> --comment "Verified and accepted. See the acceptance criteria matrix above."
```

If the user does not accept, leave the issue at `status:implemented`, record what is missing in the comment, and return to Build for the failing criteria.

## Step 7: Epic rollup

When the verified issue is a sub-issue:

1. Comment on the parent linking the child's matrix.
2. Check the parent's own acceptance criteria only when a parent-level criterion is genuinely satisfied by the child's evidence.
3. Run `gh sub-issue list <PARENT>` and check whether every child is `status:verified`.
4. If all children are verified, verify the parent's top-level acceptance criteria directly, then transition the parent to `status:verified` and close it.
5. If children remain, report which one is next.

Do not mark a parent verified because its children are done. The parent's own criteria still need evidence.

## Step 8: Update the roadmap log

When the repo has an OKF bundle, append an entry to `docs/specs/log.md` under a `YYYY-MM-DD` heading, newest first:

```markdown
## 2026-08-04

- Verified #142 Export workflow for saved reports. Evidence: `uat-evidence/web-20260804-141200/`.
```

Keep it to roadmap-level events. The issue holds the detail.

## Stop and ask

Ask before proceeding when:

- The spec issue cannot be found or is not `status:implemented`.
- Acceptance criteria are missing or ambiguous.
- Required credentials, services, or devices are unavailable.
- Verification would run destructive commands.
- The branch has unrelated changes that make evidence unreliable.
- The issue was already closed without a verification record.
