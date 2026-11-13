"""What a branch may not carry into staging, which the fast check refuses before any lane runs
(checks.py): conflict markers, commit subjects that are not short Conventional Commits, and
symlinks checked out or committed as plain files. The same module holds the cheap source checks
(file length, Markdown and Python formatting) that cause most failures of the fast check, so
they fail in seconds, before the build, and `just precheck` runs them on the staged files.
"""

from __future__ import annotations

import re
import subprocess
import sys

from . import changes, checks, file_length, paths

COMMIT: re.Pattern[str] = re.compile(
    r"^(feat|fix|perf|refactor|test|docs|build|ci|chore|style|revert)"
    r"(\([a-z0-9-]+\))?!?: \S"
)
COMMIT_MAX = 60
TEXT_SUFFIXES = (".py", ".rs", ".md", ".toml", ".json", ".ts", ".tsx", ".css", ".html")


def problems(base: str, changed: list[str]) -> list[str]:
    """Everything wrong with the branch since `base`, whose `changed` paths are checked for
    conflict markers; one line each."""
    return (
        conflict_markers(changed)
        + commit_problems(base)
        + symlink_problems(base)
        + source_problems(changed)
    )


def source_problems(changed: list[str]) -> list[str]:
    """The checks that need no build and take about 3 seconds in all: no source file over the
    length limit (file_length.py), and the `changed` Markdown and Python files formatted as
    mdformat and black would write them."""
    found = [
        f"file-length: {line}"
        for line in file_length.check(paths.REPO, file_length.ALLOWLIST)
    ]
    markdown = set(changes.markdown_files())
    found += unformatted(
        "mdformat", [p for p in changed if p in markdown and (paths.REPO / p).is_file()]
    )
    found += unformatted(
        "black",
        [p for p in changed if p.endswith(".py") and (paths.REPO / p).is_file()],
        "--quiet",
    )
    return found


def unformatted(tool: str, files: list[str], *flags: str) -> list[str]:
    """One line per file that `tool --check` would rewrite; none for no files."""
    if not files:
        return []
    out = subprocess.run(
        [sys.executable, "-m", tool, "--check", *flags, *files],
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode == 0:
        return []
    return [
        f"{tool}: {line}" for line in (out.stdout + out.stderr).splitlines() if line
    ] + [f"{tool}: run `just fmt`"]


def conflict_markers(files: list[str]) -> list[str]:
    hits = []
    for rel in files:
        f = paths.REPO / rel
        if not f.is_file() or not rel.endswith(TEXT_SUFFIXES):
            continue
        for i, line in enumerate(f.read_text(errors="replace").splitlines(), 1):
            if re.match(r"^(<{7}|>{7}|={7})( |$)", line):
                hits.append(f"{rel}:{i}: conflict marker")
    return hits


def commit_problems(base: str) -> list[str]:
    log = (
        checks.git("log", "--no-merges", "--format=%h%x09%s", f"{base}..HEAD")
        if base
        else ""
    )
    problems = []
    for row in log.splitlines():
        sha, _, subject = row.partition("\t")
        if not COMMIT.match(subject):
            problems.append(
                f"{sha} {subject!r}: not a Conventional Commit (`fix: ...`)"
            )
        elif len(subject) > COMMIT_MAX:
            problems.append(f"{sha} {subject!r}: over {COMMIT_MAX} characters")
    return problems


def symlink_problems(base: str) -> list[str]:
    """Tracked symlinks checked out, or committed since `base`, as plain files. Git for Windows
    writes a link's target into a file when `core.symlinks` is false, its system default.
    """
    fix = "git config --global core.symlinks true"
    problems = []
    for row in checks.git("ls-files", "-s").splitlines():
        mode, _, rest = row.partition(" ")
        rel = rest.partition("\t")[2]
        if mode == "120000" and not (paths.REPO / rel).is_symlink():
            problems.append(
                f"{rel}: a symlink checked out as a file; `{fix}`, then `git checkout -- {rel}`"
            )
    raw = checks.git("diff", "--raw", "--no-renames", base, "HEAD") if base else ""
    for row in raw.splitlines():
        old, new = row[1:].split()[:2]
        if old == "120000" and new not in ("120000", "000000"):
            rel = row.partition("\t")[2]
            problems.append(f"{rel}: committed a symlink as a file; `{fix}`")
    return problems


def staged() -> list[str]:
    """The added, copied, modified and renamed paths in the index."""
    out = checks.git("diff", "--cached", "--name-only", "--diff-filter=ACMR")
    return out.splitlines()


if __name__ == "__main__":
    # `just precheck`: the cheap source checks on the staged files, for a pre-commit hook.
    found: list[str] = source_problems(staged())
    for line in found:
        print(f"precheck: {line}")
    sys.exit(1 if found else 0)
