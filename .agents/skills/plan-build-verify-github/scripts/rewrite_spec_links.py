#!/usr/bin/env python3
"""Rewrite Markdown link destinations after spec files move into the archive.

Reads a path map of `<old-path>\\t<new-path>` lines (repo-relative or absolute),
scans tracked Markdown files, and repoints every link destination that resolved
to a moved file. Code fences, indented code blocks, and inline code spans are
left alone so historical path references inside examples survive.

Output is line-oriented so the calling script can act on it:

    rewrote <file> (<n> links)
    unresolved <file>:<line> -> <destination>

Exit status is 0 when the scan completes, 1 on an unusable path map.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

# [text](dest "title")  and  [text](<dest>)
INLINE_LINK = re.compile(r"(?<!\\)(!?\[(?:[^\]\\]|\\.)*\])\(\s*(<[^>]*>|[^()\s]+)((?:\s+(?:\"[^\"]*\"|'[^']*'|\([^)]*\)))?)\s*\)")
# [label]: dest "title"
REF_LINK = re.compile(r"^(\s{0,3}\[[^\]]+\]:\s*)(<[^>]*>|\S+)(\s.*)?$")
FENCE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")
INLINE_CODE = re.compile(r"(`+)(?:.|\n)*?\1")

SKIP_PREFIXES = ("http://", "https://", "mailto:", "tel:", "ftp://", "//", "#", "data:")


def load_map(map_path: Path, root: Path) -> dict[Path, Path]:
    moves: dict[Path, Path] = {}
    for lineno, raw in enumerate(map_path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw.strip():
            continue
        parts = raw.split("\t")
        if len(parts) != 2:
            print(f"error: {map_path}:{lineno}: expected two tab-separated paths", file=sys.stderr)
            raise SystemExit(1)
        old = (root / parts[0]).resolve() if not os.path.isabs(parts[0]) else Path(parts[0]).resolve()
        new = (root / parts[1]).resolve() if not os.path.isabs(parts[1]) else Path(parts[1]).resolve()
        moves[old] = new
    return moves


def tracked_markdown(root: Path) -> list[Path]:
    try:
        out = subprocess.run(
            ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "*.md", "*.markdown"],
            cwd=root, capture_output=True, text=True, check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"error: git ls-files failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
    return [root / p for p in out.split("\0") if p]


def code_spans(line: str) -> list[tuple[int, int]]:
    return [m.span() for m in INLINE_CODE.finditer(line)]


def inside(spans: list[tuple[int, int]], start: int, end: int) -> bool:
    return any(s <= start and end <= e for s, e in spans)


def split_dest(dest: str) -> tuple[str, str, str]:
    """Return (prefix, path, suffix) so anchors and query strings survive."""
    if dest.startswith("<") and dest.endswith(">"):
        return "<", dest[1:-1], ">"
    return "", dest, ""


def resolve(dest: str, source: Path, root: Path, docs_root: Path) -> Path | None:
    if not dest or dest.startswith(SKIP_PREFIXES):
        return None
    path_part = dest.split("#", 1)[0].split("?", 1)[0]
    if not path_part:
        return None
    path_part = path_part.replace("%20", " ")
    if path_part.startswith("/"):
        # OKF bundles use docs-root-absolute links such as /adrs/0001-x.md.
        candidates = [docs_root / path_part.lstrip("/"), root / path_part.lstrip("/")]
    else:
        candidates = [source.parent / path_part]
    resolved_candidates = []
    for cand in candidates:
        try:
            resolved_candidates.append(cand.resolve())
        except OSError:
            continue
    for cand in resolved_candidates:
        if cand.exists():
            return cand
    return resolved_candidates[0] if resolved_candidates else None


def relocate(dest: str, new_target: Path, source: Path, root: Path, docs_root: Path) -> str:
    path_part, sep, tail = dest.partition("#")
    if not sep:
        path_part, sep, tail = dest.partition("?")
    if path_part.startswith("/"):
        try:
            new_path = "/" + str(new_target.relative_to(docs_root))
        except ValueError:
            new_path = "/" + str(new_target.relative_to(root))
    else:
        new_path = os.path.relpath(new_target, start=source.parent)
        # Keep the author's prefix style so the diff shows the path change only.
        if path_part.startswith("./") and not new_path.startswith((".", "/")):
            new_path = "./" + new_path
    return new_path + sep + tail


def process(path: Path, moves: dict[Path, Path], root: Path, docs_root: Path,
            scope: Path, unresolved: list[str], dry_run: bool) -> int:
    try:
        original = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return 0

    lines = original.splitlines(keepends=True)
    changed = 0
    in_fence = False
    fence_marker = ""
    out: list[str] = []

    for lineno, line in enumerate(lines, 1):
        fence = FENCE.match(line)
        if fence:
            marker = fence.group(1)
            if not in_fence:
                in_fence, fence_marker = True, marker[0] * 3
            elif marker.startswith(fence_marker):
                in_fence, fence_marker = False, ""
            out.append(line)
            continue
        if in_fence or line.startswith(("    ", "\t")) and not line.strip().startswith(("-", "*", "+", "[")):
            out.append(line)
            continue

        spans = code_spans(line)
        new_line = line
        offset = 0

        def handle(dest_raw: str, start: int, end: int) -> str | None:
            nonlocal changed
            if inside(spans, start, end):
                return None
            pre, dest, post = split_dest(dest_raw)
            target = resolve(dest, path, root, docs_root)
            if target is None:
                return None
            if target in moves:
                changed += 1
                return pre + relocate(dest, moves[target], path, root, docs_root) + post
            # Only report breakage inside the migrated tree. Pre-existing
            # broken links elsewhere are not this script's business.
            if not target.exists() and target.is_relative_to(scope):
                unresolved.append(f"{path.relative_to(root)}:{lineno} -> {dest}")
            return None

        ref = REF_LINK.match(line)
        if ref:
            replacement = handle(ref.group(2), ref.start(2), ref.end(2))
            if replacement is not None:
                new_line = ref.group(1) + replacement + (ref.group(3) or "")
                if not new_line.endswith("\n") and line.endswith("\n"):
                    new_line += "\n"
            out.append(new_line)
            continue

        for m in INLINE_LINK.finditer(line):
            replacement = handle(m.group(2), m.start(2), m.end(2))
            if replacement is None:
                continue
            s, e = m.start(2) + offset, m.end(2) + offset
            new_line = new_line[:s] + replacement + new_line[e:]
            offset += len(replacement) - (m.end(2) - m.start(2))

        out.append(new_line)

    if changed and not dry_run:
        path.write_text("".join(out), encoding="utf-8")
    return changed


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--map", required=True, help="TSV of old<TAB>new paths")
    ap.add_argument("--root", default=".", help="repo root")
    ap.add_argument("--docs-root", default="docs", help="root for /-absolute links")
    ap.add_argument("--scope", default="docs/specs",
                    help="only report unresolved links pointing inside this tree")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    root = Path(args.root).resolve()
    docs_root = (root / args.docs_root).resolve()
    scope = (root / args.scope).resolve()
    moves = load_map(Path(args.map), root)
    if not moves:
        print("no moved paths in the map; nothing to rewrite")
        return 0

    unresolved: list[str] = []
    total_files = 0
    total_links = 0

    for md in tracked_markdown(root):
        if not md.exists():
            continue
        count = process(md, moves, root, docs_root, scope, unresolved, args.dry_run)
        if count:
            total_files += 1
            total_links += count
            print(f"rewrote {md.relative_to(root)} ({count} links)")

    for item in sorted(set(unresolved)):
        print(f"unresolved {item}")

    print(f"link rewrite: {total_links} destinations in {total_files} files; "
          f"{len(set(unresolved))} unresolved")
    return 0


if __name__ == "__main__":
    sys.exit(main())
