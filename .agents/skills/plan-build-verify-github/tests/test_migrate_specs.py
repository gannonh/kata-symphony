"""Regression tests for scripts/migrate_specs.sh.

Each test encodes a failure observed in a real migration run. The names say what
would break in production if the assertion stopped holding.
"""

from __future__ import annotations

import re

import pytest


APPROVED_SPEC = """---
type: Spec
title: Export workflow
status: Approved
---

# Export workflow

## Acceptance criteria

- [ ] Exports produce a CSV.
"""

PHASE_TABLE_SPEC = """# MCP OAuth support

## Status

- **Plan**: Approved
- **Build**: Implemented

## Acceptance criteria

- [ ] Tokens refresh.
"""

BUILD_REPORT = """# Build report: notifications

The notifications work shipped last quarter.
"""


# --------------------------------------------------------------- dry run --


def test_dry_run_survives_h1_and_status_section(repo):
    """A dry run used to die with SIGPIPE (exit 141) after the first file.

    Early `exit` inside a piped awk killed the upstream process, and pipefail
    turned that into a whole-script abort. Losing the dry run means the operator
    applies a migration they were never able to preview.
    """
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.write("docs/specs/2026-01-02-oauth.md", PHASE_TABLE_SPEC)
    repo.write("docs/specs/2026-01-03-report.md", BUILD_REPORT)
    repo.commit()

    proc = repo.migrate("--dry-run")

    assert proc.returncode == 0
    assert "2026-01-01-export.md" in proc.stdout
    assert "2026-01-03-report.md" in proc.stdout, "the run stopped before the last file"


def test_dry_run_writes_nothing(repo):
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()
    before = repo.read("docs/specs/index.md")

    repo.migrate("--dry-run")

    assert repo.read("docs/specs/index.md") == before
    assert (repo.root / "docs/specs/2026-01-01-export.md").exists()
    assert not any("issue create" in c for c in repo.gh_calls())
    assert not any(c.startswith("label create") for c in repo.gh_calls())


# ------------------------------------------------------ status detection --


@pytest.mark.parametrize(
    "body,expected_status,expected_action",
    [
        ("---\ntype: Spec\nstatus: Approved\n---\n\n# T\n", "Approved", "migrate"),
        ("# T\n\n## Status\n\nApproved\n", "Approved", "migrate"),
        ("# T\n\n## Status\n\n- **Plan**: Approved\n", "Plan: Approved", "migrate"),
        ("# T\n\n## Status\n\n- **Plan**: Approved\n- **Build**: Implemented\n",
         "Build: Implemented", "migrate"),
        ("# T\n\n## Status\n\n- **Build**: Implemented\n- **Verify**: Completed\n",
         "Verify: Completed", "archive"),
        ("# T\n\n**Status**: Implemented\n", "Implemented", "migrate"),
        ("# T\n\nNo status anywhere.\n", "<none>", "skip"),
    ],
)
def test_status_forms_are_recognized(repo, body, expected_status, expected_action):
    """Sixteen of twenty-six files came back `unknown` because only a bare
    `## Status` value parsed. Unrecognized status means the file is silently
    left behind or misclassified."""
    repo.write("docs/specs/2026-01-01-thing.md", body)
    repo.commit()

    out = repo.migrate("--assess").stdout
    line = next(l for l in out.splitlines() if "2026-01-01-thing.md" in l)

    assert expected_status in line, out
    assert line.strip().endswith(expected_action), out


def test_status_map_overrides_everything(repo):
    repo.write("docs/specs/2026-01-01-thing.md", "# T\n\n## Status\n\nImplemented\n")
    repo.write("overrides.txt", "# legacy classification\n2026-01-01-thing.md completed\n")
    repo.commit()

    out = repo.migrate("--assess", "--status-map", "overrides.txt").stdout
    line = next(l for l in out.splitlines() if "2026-01-01-thing.md" in l)

    assert "completed" in line
    assert "override" in line
    assert line.strip().endswith("archive")


def test_conflicting_status_evidence_blocks_writes(repo):
    """The OAuth documents claimed Implemented in one place and blocked in
    another. Writing on contradictory evidence produces a roadmap nobody
    trusts, so the run must stop before the first issue is created."""
    repo.write(
        "docs/specs/2026-01-01-oauth.md",
        "---\ntype: Spec\nstatus: Implemented\n---\n\n# OAuth\n\n## Status\n\nBlocked\n",
    )
    repo.commit()

    proc = repo.migrate(expect=1)

    assert "conflicting status evidence" in proc.stderr
    assert (repo.root / "docs/specs/2026-01-01-oauth.md").exists()
    assert not any("issue create" in c for c in repo.gh_calls())

    proc = repo.migrate("--allow-conflicts")
    assert "conflict" not in proc.stderr


def test_confidence_is_reported_per_file(repo):
    repo.write("docs/specs/a.md", "---\ntype: Spec\nstatus: Approved\n---\n\n# A\n")
    repo.write("docs/specs/b.md", "# B\n\n**Status**: Approved\n")
    repo.write("docs/specs/c.md", "# C\n")
    repo.commit()

    out = repo.migrate("--assess", "--default-status", "draft").stdout

    assert re.search(r"a\.md\s+Approved\s+frontmatter\s+high", out), out
    assert re.search(r"b\.md\s+Approved\s+inline\s+medium", out), out
    assert re.search(r"c\.md\s+draft\s+default\s+low", out), out


# ---------------------------------------------------- implemented policy --


@pytest.mark.parametrize(
    "action,expect_issue,expect_archived",
    [
        ("migrate", True, True),
        ("archive", False, True),
        ("blocked", True, True),
        ("skip", False, False),
    ],
)
def test_implemented_action_policy(repo, action, expect_issue, expect_archived):
    """`--default-status completed` only covered files with no status at all, so
    there was no supported way to archive Implemented work. The operator had to
    patch the script."""
    repo.write("docs/specs/2026-01-01-thing.md",
               "---\ntype: Spec\nstatus: Implemented\n---\n\n# Thing\n")
    repo.commit()

    repo.migrate("--implemented-action", action)

    created = [c for c in repo.gh_calls() if c.startswith("issue create")]
    assert bool(created) is expect_issue
    assert (repo.root / "docs/specs/archive/2026-01-01-thing.md").exists() is expect_archived
    if action == "blocked":
        assert "status:blocked" in repo.issue_labels(100)


# ------------------------------------------------------ archive metadata --


def test_archive_status_is_normalized_and_source_preserved(repo):
    """Archived frontmatter used to keep the pre-migration status while the
    pointer line claimed the file was completed. Two contradictory answers in
    one file is worse than either answer alone."""
    repo.write("docs/specs/done.md",
               "---\ntype: Spec\ntitle: Done thing\nstatus: Implemented\n---\n\n# Done thing\n")
    repo.write("docs/specs/live.md",
               "---\ntype: Spec\ntitle: Live thing\nstatus: Approved\n---\n\n# Live thing\n")
    repo.commit()

    repo.migrate("--implemented-action", "archive")

    done = repo.read("docs/specs/archive/done.md")
    assert "status: Completed" in done
    assert "source_status: Implemented" in done
    assert "migrated: false" in done
    assert "\nstatus: Implemented" not in done

    live = repo.read("docs/specs/archive/live.md")
    assert "status: Migrated" in live
    assert "source_status: Approved" in live
    assert "github_issue: 100" in live


def test_archive_frontmatter_always_has_type(repo):
    """OKF validation fails on archived files without a non-empty `type`."""
    repo.write("docs/specs/nofm.md", "# No frontmatter\n\n## Status\n\nApproved\n")
    repo.commit()

    repo.migrate()

    archived = repo.read("docs/specs/archive/nofm.md")
    assert "type: Spec" in archived
    assert "title: No frontmatter" in archived


# ---------------------------------------------------------------- links --


def test_links_to_moved_specs_are_rewritten(repo):
    """Moving files one directory deeper broke every relative link pointing at
    them. The migration moved files and left the repo's documentation lying."""
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.write("docs/index.md",
               "# Docs\n\nSee [export](specs/2026-01-01-export.md) and "
               "[adr](adrs/0001-x.md).\n")
    repo.write("docs/adrs/0001-x.md",
               "---\ntype: ADR\n---\n\n# X\n\nConstrained by "
               "[the export spec](/specs/2026-01-01-export.md).\n")
    repo.write("README.md",
               "# Repo\n\nRoadmap: [export](docs/specs/2026-01-01-export.md)\n\n"
               "```bash\ncat docs/specs/2026-01-01-export.md\n```\n")
    repo.commit()

    repo.migrate()

    assert "(specs/archive/2026-01-01-export.md)" in repo.read("docs/index.md")
    assert "adrs/0001-x.md" in repo.read("docs/index.md"), "untouched links must survive"
    assert "/specs/archive/2026-01-01-export.md" in repo.read("docs/adrs/0001-x.md")

    readme = repo.read("README.md")
    assert "docs/specs/archive/2026-01-01-export.md)" in readme
    assert "cat docs/specs/2026-01-01-export.md" in readme, "code fences must not be rewritten"


def test_link_rewriting_can_be_declined(repo):
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.write("docs/index.md", "# Docs\n\n[export](specs/2026-01-01-export.md)\n")
    repo.commit()

    repo.migrate("--no-rewrite-links")

    assert "specs/2026-01-01-export.md" in repo.read("docs/index.md")
    assert "archive" not in repo.read("docs/index.md")


# ---------------------------------------------------------------- index --


def test_index_preserves_roadmap_context_by_default(repo):
    """The generated index replaced the roadmap wholesale, dropping the active
    initiative and its issue links."""
    repo.write("docs/specs/index.md",
               "# Specs\n\n## Active\n\n- Integrated browser: #28, #29, #30, #31\n\n"
               "## Deferred\n\n- Offline mode\n")
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    repo.migrate()

    index = repo.read("docs/specs/index.md")
    assert "gh issue list --label kind:spec" in index
    assert "#28, #29, #30, #31" in index
    assert "Offline mode" in index


def test_replace_index_discards_previous_content(repo):
    repo.write("docs/specs/index.md", "# Specs\n\n## Active\n\n- Integrated browser: #28\n")
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    repo.migrate("--replace-index")

    index = repo.read("docs/specs/index.md")
    assert "gh issue list --label kind:spec" in index
    assert "#28" not in index


# ------------------------------------------------------------------ log --


def test_log_entry_merges_into_an_existing_same_day_heading(repo):
    """A second `## <today>` heading in the log produced a duplicate section the
    operator had to merge by hand."""
    import datetime

    today = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
    repo.write("docs/specs/log.md",
               f"# Specs Log\n\n## {today}\n\nAdded the export spec.\n\n## 2020-01-01\n\nCreated.\n")
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    repo.migrate()

    log = repo.read("docs/specs/log.md")
    assert log.count(f"## {today}") == 1
    assert "Added the export spec." in log
    assert "Migrated file-based specs" in log
    assert log.index(f"## {today}") < log.index("## 2020-01-01"), "newest-first ordering broke"


def test_log_records_failures(repo):
    repo.write("docs/specs/ok.md", APPROVED_SPEC)
    repo.write("docs/specs/mystery.md", "# Mystery\n\n## Status\n\nSomething odd\n")
    repo.commit()

    repo.migrate(expect=0)

    log = repo.read("docs/specs/log.md")
    assert "mystery.md" in log
    assert "status unclear" in log


# --------------------------------------------------------------- labels --


def test_labels_are_not_written_when_no_issue_is_created(repo):
    """A migration that created zero issues still wrote fourteen labels, and the
    dry run never said it would."""
    repo.write("docs/specs/done.md",
               "---\ntype: Spec\nstatus: Completed\n---\n\n# Done\n")
    repo.commit()

    proc = repo.migrate()

    assert not any(c.startswith("label create") for c in repo.gh_calls())
    assert "skipping label setup" in proc.stdout


def test_ensure_labels_flag_forces_label_setup(repo):
    repo.write("docs/specs/done.md",
               "---\ntype: Spec\nstatus: Completed\n---\n\n# Done\n")
    repo.commit()

    repo.migrate("--ensure-labels")

    assert any(c.startswith("label create kind:spec") for c in repo.gh_calls())


def test_dry_run_shows_the_label_plan(repo):
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    out = repo.migrate("--dry-run").stdout

    assert "Label plan" in out
    assert "would create" in out
    assert "kind:spec" in out


# ---------------------------------------------------------- idempotence --


def test_unparseable_create_output_recovers_the_issue(repo):
    """If `gh issue create` succeeded but its URL could not be parsed, the issue
    existed while the file stayed unmigrated. The next run then created a
    duplicate."""
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    proc = repo.migrate(GH_STUB_UNPARSEABLE_CREATE="1")

    assert "recovered #100" in proc.stderr
    assert (repo.root / "docs/specs/archive/2026-01-01-export.md").exists()
    assert "github_issue: 100" in repo.read("docs/specs/archive/2026-01-01-export.md")


def test_rerun_after_failure_reuses_the_existing_issue(repo):
    """A migration that died after creating issues but before archiving must not
    open duplicates on the next run."""
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    # Stand in for a previous run that created the issue and then died.
    issues = repo.state / "issues"
    issues.mkdir(parents=True, exist_ok=True)
    (issues / "77.body").write_text(
        "## Status\n\nApproved\n\n_Source key: `pbvg-source:docs/specs/2026-01-01-export.md`_\n",
        encoding="utf-8",
    )

    proc = repo.migrate()

    assert "reuse" in proc.stdout
    assert not any(c.startswith("issue create") for c in repo.gh_calls())
    assert "github_issue: 77" in repo.read("docs/specs/archive/2026-01-01-export.md")


def test_issue_body_carries_a_source_key(repo):
    repo.write("docs/specs/2026-01-01-export.md", APPROVED_SPEC)
    repo.commit()

    repo.migrate()

    assert "pbvg-source:docs/specs/2026-01-01-export.md" in repo.issue_body(100)


def test_already_migrated_files_are_skipped(repo):
    repo.write("docs/specs/2026-01-01-export.md",
               "---\ntype: Spec\nstatus: Approved\ngithub_issue: 42\n---\n\n# Export\n")
    repo.commit()

    proc = repo.migrate()

    assert "already migrated to #42" in proc.stdout
    assert not any(c.startswith("issue create") for c in repo.gh_calls())
