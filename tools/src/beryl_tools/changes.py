"""What changed, for the checks (checks.py re-exports these): the paths a branch changed since
it left staging, and the fast check run again after `enact push` lost the race to staging
(checks.fast, `narrow`): Enact
names the commit the last pass checked in `$ENACT_CHECKED`. Staging takes about twenty pushes
an hour, so a two-minute check often loses that race, and running every lane again repeated
work the commits that landed meanwhile could not affect. A lane reruns only when those commits
select it too: a lane none of their paths selects reads nothing they changed, by the same rule
that let the first pass leave out the lanes the change itself did not select.
"""

from __future__ import annotations

import os
import subprocess
from collections.abc import Callable
from typing import TypeVar

from . import paths

L = TypeVar("L")

CHECKED_ENV = "ENACT_CHECKED"


def run_git(*args: str) -> str | None:
    """Git's output, or None when it failed."""
    run = subprocess.run(
        ["git", *args], cwd=paths.REPO, capture_output=True, text=True, check=False
    )
    return run.stdout.strip() if run.returncode == 0 else None


def git(*args: str) -> str:
    """Git's output, or nothing when it failed."""
    return run_git(*args) or ""


def branch_base() -> str:
    for ref in ("origin/staging", "staging", "origin/master"):
        base = git("merge-base", ref, "HEAD")
        if base:
            return base
    return git("rev-parse", "--verify", "--quiet", "HEAD^")


def changed_paths(base: str) -> list[str] | None:
    """Paths changed since `base`, committed or not; None when there is no base."""
    if not base:
        return None
    diff = git("diff", "--name-only", "--no-renames", base).splitlines()
    untracked = git("ls-files", "--others", "--exclude-standard").splitlines()
    return sorted(set(diff + untracked))


def markdown_files() -> list[str]:
    """Every Markdown file but the beads' and the agent skills' (whose YAML front matter
    mdformat would rewrite)."""
    return [
        p
        for p in git("ls-files", "-co", "--exclude-standard", "*.md").splitlines()
        if not p.startswith((".beads/", ".agents/"))
    ]


def landed(checked: str, base: str) -> list[str] | None:
    """The paths the commits between the base `checked` was built on and the new `base`
    changed; None when git cannot tell, and every lane runs."""
    old = run_git("merge-base", checked, base)
    if not old:
        return None
    diff = run_git("diff", "--name-only", "--no-renames", old, base)
    return None if diff is None else [p for p in diff.splitlines() if p]


def narrow(
    lanes: list[L],
    base: str,
    lanes_for: Callable[[list[str] | None], list[L]],
    name: Callable[[L], str],
) -> list[L]:
    """`lanes`, the change's own, less those the commits landed since `$ENACT_CHECKED`
    passed cannot reach (`lanes_for` their paths); all of them on a first run."""
    checked = os.environ.get(CHECKED_ENV)
    if not checked or not base:
        return lanes
    found = landed(checked, base)
    if found is None:
        return lanes
    reached = {name(lane) for lane in lanes_for(found)}
    kept = [lane for lane in lanes if name(lane) in reached]
    left = sorted(name(lane) for lane in lanes if name(lane) not in reached)
    if left:
        print(
            f"check: {checked[:8]} passed before staging moved; the {len(found)} paths"
            f" that landed since reach none of {', '.join(left)}, which stand"
        )
    return kept
