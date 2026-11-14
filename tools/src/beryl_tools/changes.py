"""What changed, for the checks (checks.py re-exports these): the paths a branch changed since
it left staging, and the fast check run again on a tree that holds branches already checked
(checks.fast, `narrow`).

Enact names the commits whose fast check passed in `$ENACT_CHECKED`, separated by spaces. A push
checks its branch in its own tree, then Enact's merge queue checks a batch of such branches
together on the staging tip; a push that lost a race to staging checks again after it rebases.
Running every lane again repeated work that nothing new could affect: on 2026-11-14 the queue's
checks took 150-300 s a batch, one batch at a time, and pushes waited up to 25 minutes.

So a lane runs again only where a checked branch's own change and the rest of the tree both
select it. The rest is what the tree holds that the branch's check did not see: the commits that
landed on staging since, and the batch's other branches. A lane none of their paths selects
reads nothing that changed under the branch, by the same rule that let the first pass leave out
the lanes the change itself did not select.
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


def master_base() -> str:
    """Where HEAD left `origin/master`, for slow CI: master moves only when slow CI passes on
    staging, so the commits since are those no slow CI run has passed yet."""
    return git("merge-base", "origin/master", "HEAD")


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


def own_paths(checked: str, base: str) -> list[str] | None:
    """The paths the branch that passed at `checked` changed since it left staging, the old
    staging it was built on (its merge base with `base`); None when git cannot tell."""
    old = run_git("merge-base", checked, base)
    if not old:
        return None
    diff = run_git("diff", "--name-only", "--no-renames", old, checked)
    return None if diff is None else [p for p in diff.splitlines() if p]


def landed(checked: str) -> list[str] | None:
    """The paths that the tree at HEAD changed beyond what passed at `checked`: those of each
    commit HEAD has and `checked` lacks, but the replays of `checked`'s own commits (a rebase or
    the merge queue's cherry-picks: `--cherry-pick` matches them by their patch). None when git
    cannot tell, and every lane runs."""
    if run_git("rev-parse", "--verify", "--quiet", f"{checked}^{{commit}}") is None:
        return None
    log = run_git(
        "log",
        "--cherry-pick",
        "--right-only",
        "--no-merges",
        "--name-only",
        "--format=",
        f"{checked}...HEAD",
    )
    return None if log is None else sorted({p for p in log.splitlines() if p})


def narrow(
    lanes: list[L],
    base: str,
    lanes_for: Callable[[list[str] | None], list[L]],
    name: Callable[[L], str],
) -> list[L]:
    """`lanes`, the change's own, less those no branch checked in `$ENACT_CHECKED` needs again:
    a branch needs a lane again when its own paths and the paths landed beyond it both select it
    (`lanes_for`). All of them on a first run, or when git cannot place a checked commit.
    """
    checked = os.environ.get(CHECKED_ENV, "").split()
    if not checked or not base:
        return lanes
    reached: set[str] = set()
    for commit in checked:
        own, found = own_paths(commit, base), landed(commit)
        if own is None or found is None:
            return lanes
        reached |= {name(lane) for lane in lanes_for(own)} & {
            name(lane) for lane in lanes_for(found)
        }
    kept = [lane for lane in lanes if name(lane) in reached]
    left = sorted(name(lane) for lane in lanes if name(lane) not in reached)
    if left:
        print(
            f"check: {' '.join(c[:8] for c in checked)} passed their fast checks; what the tree"
            f" holds beyond them reaches none of {', '.join(left)}, which stand"
        )
    return kept
