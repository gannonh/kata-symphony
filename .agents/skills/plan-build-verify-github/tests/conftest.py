from __future__ import annotations

import os
import shutil
import stat
import subprocess
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
MIGRATE = SCRIPTS / "migrate_specs.sh"

# Prefer the system bash. On macOS that is 3.2, the oldest interpreter the
# script has to survive, which catches array and `set -u` regressions that a
# newer Homebrew bash would hide.
BASH = "/bin/bash" if Path("/bin/bash").exists() else "bash"

# A fake `gh` that records every invocation and answers the handful of
# subcommands the migration uses. Issue numbers start at 100 and increment.
GH_STUB = r"""#!/usr/bin/env bash
set -euo pipefail
LOG="${GH_STUB_LOG:?}"
STATE="${GH_STUB_STATE:?}"
printf '%s\n' "$*" >> "$LOG"

[ -f "$STATE/counter" ] || echo 99 > "$STATE/counter"
mkdir -p "$STATE/issues"

if [ "${GH_STUB_FAIL_CREATE:-0}" = "1" ] && [ "${1:-}" = "issue" ] && [ "${2:-}" = "create" ]; then
  echo "simulated create failure" >&2
  exit 1
fi

case "${1:-}" in
  auth)
    [ "${GH_STUB_UNAUTHENTICATED:-0}" = "1" ] && exit 1
    exit 0 ;;
  repo)
    echo "acme/widgets" ; exit 0 ;;
  label)
    case "${2:-}" in
      list) [ -f "$STATE/labels" ] && cat "$STATE/labels" || true ; exit 0 ;;
      create)
        name="$3"
        grep -qxF "$name" "$STATE/labels" 2>/dev/null || echo "$name" >> "$STATE/labels"
        exit 0 ;;
    esac
    exit 0 ;;
  issue)
    case "${2:-}" in
      create)
        body_file=""
        title=""
        labels=""
        shift 2
        while [ $# -gt 0 ]; do
          case "$1" in
            --body-file) body_file="$2"; shift 2 ;;
            --title) title="$2"; shift 2 ;;
            --label) labels="$2"; shift 2 ;;
            --repo) shift 2 ;;
            *) shift ;;
          esac
        done
        n=$(( $(cat "$STATE/counter") + 1 ))
        echo "$n" > "$STATE/counter"
        printf '%s\n' "$title" > "$STATE/issues/$n.title"
        printf '%s\n' "$labels" > "$STATE/issues/$n.labels"
        cp "$body_file" "$STATE/issues/$n.body"
        if [ "${GH_STUB_UNPARSEABLE_CREATE:-0}" = "1" ]; then
          echo "created"
        else
          echo "https://github.com/acme/widgets/issues/$n"
        fi
        exit 0 ;;
      list)
        shift 2
        q=""
        while [ $# -gt 0 ]; do
          case "$1" in -q|--jq) q="$2"; shift 2 ;; *) shift ;; esac
        done
        case "$q" in
          *length*) echo 0; exit 0 ;;
          *select*) exit 0 ;;
        esac
        # The body-scan form used for source-key recovery.
        for f in "$STATE"/issues/*.body; do
          [ -e "$f" ] || continue
          n="$(basename "$f" .body)"
          printf '%s\t%s\n' "$n" "$(tr '\n' ' ' < "$f")"
        done
        exit 0 ;;
    esac
    exit 0 ;;
esac
exit 0
"""


@pytest.fixture()
def repo(tmp_path: Path):
    """A git repo with a docs/ bundle and a stubbed gh on PATH."""
    root = tmp_path / "repo"
    (root / "docs" / "specs").mkdir(parents=True)
    (root / "docs" / "adrs").mkdir(parents=True)

    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    gh = bin_dir / "gh"
    gh.write_text(GH_STUB, encoding="utf-8")
    gh.chmod(gh.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)

    state = tmp_path / "ghstate"
    state.mkdir()
    log = tmp_path / "gh.log"
    log.write_text("", encoding="utf-8")

    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}:{env['PATH']}"
    env["GH_STUB_LOG"] = str(log)
    env["GH_STUB_STATE"] = str(state)

    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.email", "t@example.com"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)

    class Repo:
        def __init__(self) -> None:
            self.root = root
            self.env = env
            self.log = log
            self.state = state

        def write(self, rel: str, content: str) -> Path:
            p = self.root / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
            return p

        def read(self, rel: str) -> str:
            return (self.root / rel).read_text(encoding="utf-8")

        def commit(self, message: str = "init") -> None:
            subprocess.run(["git", "add", "-A"], cwd=self.root, check=True)
            subprocess.run(["git", "commit", "-qm", message], cwd=self.root, check=True)

        def migrate(self, *args: str, expect: int | None = 0, **envvars: str):
            env = dict(self.env)
            env.update(envvars)
            proc = subprocess.run(
                [BASH, str(MIGRATE), *args],
                cwd=self.root, env=env, capture_output=True, text=True,
            )
            if expect is not None and proc.returncode != expect:
                raise AssertionError(
                    f"exit {proc.returncode} (wanted {expect})\n"
                    f"--- stdout ---\n{proc.stdout}\n--- stderr ---\n{proc.stderr}"
                )
            return proc

        def gh_calls(self) -> list[str]:
            return self.log.read_text(encoding="utf-8").splitlines()

        def issue_body(self, number: int) -> str:
            return (self.state / "issues" / f"{number}.body").read_text(encoding="utf-8")

        def issue_labels(self, number: int) -> str:
            return (self.state / "issues" / f"{number}.labels").read_text(encoding="utf-8").strip()

    r = Repo()
    r.write("docs/specs/index.md", "# Specs\n\n* [Some spec](2026-01-01-some-spec.md) - a spec\n")
    r.write("docs/specs/log.md", "# Specs Log\n\n## 2020-01-01\n\nCreated.\n")
    yield r
    shutil.rmtree(tmp_path, ignore_errors=True)
