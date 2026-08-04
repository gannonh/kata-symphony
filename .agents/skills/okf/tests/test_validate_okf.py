from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "validate_okf.py"
spec = importlib.util.spec_from_file_location("validate_okf", SCRIPT)
validate_okf = importlib.util.module_from_spec(spec)
assert spec and spec.loader
sys.modules["validate_okf"] = validate_okf
spec.loader.exec_module(validate_okf)


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def create_minimal_bundle(tmp_path: Path) -> Path:
    docs = tmp_path / "docs"
    write(
        docs / "index.md",
        "---\nokf_version: \"0.1\"\n---\n\n# Docs\n\n* [Specs](specs/) - plans and roadmap\n* [ADRs](adrs/) - decisions\n",
    )
    write(docs / "log.md", "# Directory Update Log\n\n## 2026-06-14\n* **Initialization**: Created OKF bundle.\n")
    write(docs / "specs" / "index.md", "# Specs\n\n* [Plan](plan.md) - active plan\n")
    write(docs / "specs" / "log.md", "# Specs Update Log\n\n## 2026-06-14\n* **Creation**: Added specs section.\n")
    write(
        docs / "specs" / "plan.md",
        "---\ntype: Spec\ntitle: Plan\ndescription: Active implementation plan.\n---\n\n# Plan\n\nSee [ADR 1](/adrs/0001-record-decisions.md).\n",
    )
    write(docs / "adrs" / "index.md", "# ADRs\n\n* [Record decisions](0001-record-decisions.md) - use ADRs\n")
    write(docs / "adrs" / "log.md", "# ADR Update Log\n\n## 2026-06-14\n* **Creation**: Added ADR section.\n")
    write(
        docs / "adrs" / "0001-record-decisions.md",
        "---\ntype: ADR\ntitle: Record decisions\ndescription: Use ADRs for durable decisions.\n---\n\n# Decision\n\nUse ADRs.\n",
    )
    return docs


def test_valid_minimal_bundle_passes(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)

    result = validate_okf.validate_bundle(docs)

    assert result.errors == []


def test_missing_required_section_fails(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    (docs / "adrs" / "index.md").unlink()

    result = validate_okf.validate_bundle(docs)

    assert "Missing required file: adrs/index.md" in result.errors


def test_concept_missing_frontmatter_fails(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    write(docs / "guides" / "usage.md", "# Usage\n\nRun the CLI.\n")

    result = validate_okf.validate_bundle(docs)

    assert "guides/usage.md: missing YAML frontmatter" in result.errors


def test_concept_missing_type_fails(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    write(docs / "guides" / "usage.md", "---\ntitle: Usage\n---\n\n# Usage\n")

    result = validate_okf.validate_bundle(docs)

    assert "guides/usage.md: missing non-empty frontmatter field 'type'" in result.errors


def test_malformed_log_date_fails(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    write(docs / "log.md", "# Directory Update Log\n\n## June 14, 2026\n* Updated docs.\n")

    result = validate_okf.validate_bundle(docs)

    assert "log.md:3: log date headings must use YYYY-MM-DD" in result.errors


def test_root_index_allows_okf_version_frontmatter(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)

    result = validate_okf.validate_bundle(docs)

    assert not any("index.md" in error for error in result.errors)


def test_nested_index_frontmatter_fails(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    write(docs / "guides" / "index.md", "---\ntitle: Guides\n---\n\n# Guides\n")

    result = validate_okf.validate_bundle(docs)

    assert "guides/index.md: reserved index.md should not contain frontmatter except docs/index.md okf_version" in result.errors


def test_broken_local_links_warn_by_default(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    write(
        docs / "guides" / "usage.md",
        "---\ntype: Guide\ntitle: Usage\n---\n\nSee [missing](/missing.md).\n",
    )

    result = validate_okf.validate_bundle(docs)

    assert result.errors == []
    assert "guides/usage.md: broken local link: /missing.md" in result.warnings


def test_cli_returns_nonzero_on_errors(tmp_path: Path) -> None:
    docs = create_minimal_bundle(tmp_path)
    (docs / "specs" / "log.md").unlink()

    completed = subprocess.run(
        [sys.executable, str(SCRIPT), str(tmp_path)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )

    assert completed.returncode == 1
    assert "Missing required file: specs/log.md" in completed.stderr
