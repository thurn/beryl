"""What a branch may not carry into staging, which the fast check refuses before any lane runs
(checks.py): conflict markers, commit subjects that are not short Conventional Commits, and
symlinks checked out or committed as plain files."""

from __future__ import annotations

import re

from . import checks, paths

COMMIT: re.Pattern[str] = re.compile(
    r"^(feat|fix|perf|refactor|test|docs|build|ci|chore|style|revert)"
    r"(\([a-z0-9-]+\))?!?: \S"
)
COMMIT_MAX = 60
TEXT_SUFFIXES = (".py", ".rs", ".md", ".toml", ".json", ".ts", ".tsx", ".css", ".html")


def problems(base: str, changed: list[str]) -> list[str]:
    """Everything wrong with the branch since `base`, whose `changed` paths are checked for
    conflict markers; one line each."""
    return conflict_markers(changed) + commit_problems(base) + symlink_problems(base)


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
