#!/usr/bin/env python3
"""Validate the repository OKF bundle under ./docs.

Usage:
    python okf/scripts/validate_okf.py [repo_root]
    python okf/scripts/validate_okf.py --docs path/to/docs
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

try:
    import yaml  # type: ignore
except Exception:  # pragma: no cover - exercised only when PyYAML is absent
    yaml = None


REQUIRED_FILES = (
    "index.md",
    "log.md",
    "specs/index.md",
    "specs/log.md",
    "adrs/index.md",
    "adrs/log.md",
)
RESERVED_NAMES = {"index.md", "log.md"}
DATE_HEADING_RE = re.compile(r"^##\s+(\d{4}-\d{2}-\d{2})(?:\s|$)")
ANY_DATE_HEADING_RE = re.compile(r"^##\s+(\S+)")
MARKDOWN_LINK_RE = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
URL_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


@dataclass
class ValidationResult:
    errors: list[str]
    warnings: list[str]

    @property
    def ok(self) -> bool:
        return not self.errors


def validate_bundle(docs_dir: Path, *, strict_links: bool = False) -> ValidationResult:
    docs_dir = docs_dir.resolve()
    errors: list[str] = []
    warnings: list[str] = []

    if not docs_dir.exists():
        return ValidationResult([f"Missing OKF docs directory: {docs_dir}"], warnings)
    if not docs_dir.is_dir():
        return ValidationResult([f"OKF docs path is not a directory: {docs_dir}"], warnings)

    for required in REQUIRED_FILES:
        path = docs_dir / required
        if not path.exists():
            errors.append(f"Missing required file: {required}")

    for path in sorted(docs_dir.rglob("*.md")):
        rel = _rel(path, docs_dir)
        if path.name in RESERVED_NAMES:
            _validate_reserved_file(path, docs_dir, rel, errors)
            continue
        _validate_concept_file(path, docs_dir, rel, errors)

    link_warnings = _find_broken_local_links(docs_dir)
    if strict_links:
        errors.extend(link_warnings)
    else:
        warnings.extend(link_warnings)

    return ValidationResult(errors, warnings)


def _validate_concept_file(path: Path, docs_dir: Path, rel: str, errors: list[str]) -> None:
    text = path.read_text(encoding="utf-8")
    frontmatter, parse_error = _parse_frontmatter(text)
    if parse_error:
        errors.append(f"{rel}: {parse_error}")
        return
    if frontmatter is None:
        errors.append(f"{rel}: missing YAML frontmatter")
        return
    concept_type = frontmatter.get("type") if isinstance(frontmatter, dict) else None
    if not isinstance(concept_type, str) or not concept_type.strip():
        errors.append(f"{rel}: missing non-empty frontmatter field 'type'")


def _validate_reserved_file(path: Path, docs_dir: Path, rel: str, errors: list[str]) -> None:
    text = path.read_text(encoding="utf-8")

    if path.name == "index.md":
        frontmatter, parse_error = _parse_frontmatter(text)
        if parse_error:
            errors.append(f"{rel}: {parse_error}")
            return
        if frontmatter is not None:
            allowed_root_index = path == docs_dir / "index.md" and set(frontmatter.keys()) <= {"okf_version"}
            if not allowed_root_index:
                errors.append(f"{rel}: reserved index.md should not contain frontmatter except docs/index.md okf_version")
        return

    if path.name == "log.md":
        _validate_log_file(path, docs_dir, rel, errors)


def _validate_log_file(path: Path, docs_dir: Path, rel: str, errors: list[str]) -> None:
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        match = ANY_DATE_HEADING_RE.match(line)
        if match and not DATE_HEADING_RE.match(line):
            errors.append(f"{rel}:{line_number}: log date headings must use YYYY-MM-DD")


def _parse_frontmatter(text: str) -> tuple[dict[str, Any] | None, str | None]:
    if not text.startswith("---\n") and text.strip() != "---":
        return None, None

    lines = text.splitlines()
    if not lines or lines[0] != "---":
        return None, None

    end_index = None
    for index, line in enumerate(lines[1:], start=1):
        if line == "---":
            end_index = index
            break

    if end_index is None:
        return None, "unterminated YAML frontmatter"

    raw = "\n".join(lines[1:end_index])
    if yaml is not None:
        try:
            parsed = yaml.safe_load(raw) if raw.strip() else {}
        except Exception as exc:
            return None, f"invalid YAML frontmatter: {exc}"
        if parsed is None:
            parsed = {}
        if not isinstance(parsed, dict):
            return None, "frontmatter must be a YAML mapping"
        return parsed, None

    return _parse_minimal_yaml_mapping(raw)


def _parse_minimal_yaml_mapping(raw: str) -> tuple[dict[str, Any] | None, str | None]:
    parsed: dict[str, Any] = {}
    for line_number, line in enumerate(raw.splitlines(), start=2):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if ":" not in line:
            return None, f"invalid YAML frontmatter near line {line_number}"
        key, value = line.split(":", 1)
        key = key.strip()
        if not key:
            return None, f"invalid YAML frontmatter near line {line_number}"
        parsed[key] = value.strip().strip('"\'')
    return parsed, None


def _find_broken_local_links(docs_dir: Path) -> list[str]:
    warnings: list[str] = []
    for path in sorted(docs_dir.rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        for href in MARKDOWN_LINK_RE.findall(text):
            target = href.strip().split()[0]
            if not target or target.startswith("#") or URL_RE.match(target):
                continue
            target = target.split("#", 1)[0]
            if not target:
                continue
            resolved = _resolve_markdown_target(docs_dir, path, target)
            if resolved is None:
                warnings.append(f"{_rel(path, docs_dir)}: broken local link: {href}")
    return warnings


def _resolve_markdown_target(docs_dir: Path, source: Path, target: str) -> Path | None:
    if target.startswith("/"):
        candidate = docs_dir / target.lstrip("/")
    else:
        candidate = source.parent / target

    candidates = [candidate]
    if candidate.suffix == "":
        candidates.append(candidate.with_suffix(".md"))
    if str(target).endswith("/") or candidate.is_dir():
        candidates.append(candidate / "index.md")

    docs_root = docs_dir.resolve()
    for item in candidates:
        try:
            resolved = item.resolve()
        except OSError:
            continue
        if docs_root not in (resolved, *resolved.parents):
            continue
        if resolved.exists():
            return resolved
    return None


def _rel(path: Path, docs_dir: Path) -> str:
    return path.relative_to(docs_dir).as_posix()


def _print_result(result: ValidationResult) -> None:
    if result.errors:
        print("OKF validation failed:", file=sys.stderr)
        for error in result.errors:
            print(f"  ERROR: {error}", file=sys.stderr)
    if result.warnings:
        if not result.errors:
            print("OKF validation warnings:", file=sys.stderr)
        for warning in result.warnings:
            print(f"  WARNING: {warning}", file=sys.stderr)
    if result.ok:
        print("OKF validation passed")


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate an OKF bundle under ./docs")
    parser.add_argument("repo_root", nargs="?", default=".", help="Repository root. Defaults to current directory.")
    parser.add_argument("--docs", help="Path to the OKF docs directory. Overrides repo_root/docs.")
    parser.add_argument("--strict-links", action="store_true", help="Treat broken local markdown links as errors.")
    args = parser.parse_args(list(argv) if argv is not None else None)

    docs_dir = Path(args.docs) if args.docs else Path(args.repo_root) / "docs"
    result = validate_bundle(docs_dir, strict_links=args.strict_links)
    _print_result(result)
    return 0 if result.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
