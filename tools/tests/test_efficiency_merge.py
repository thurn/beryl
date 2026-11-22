"""efficiency_merge.py: the merge driver that keeps both sides' new entries of
`data/efficiency-changes.json`."""

from __future__ import annotations

import json
import shlex
import subprocess
import sys
from pathlib import Path

import pytest

from beryl_tools import efficiency_merge


def text(*labels: str) -> str:
    return json.dumps([{"label": label} for label in labels], indent=2) + "\n"


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, capture_output=True, text=True, check=True
    ).stdout


def test_both_sides_appending_keeps_both_in_order() -> None:
    merged = efficiency_merge.merge(text("a"), text("a", "b"), text("a", "c"))
    assert merged == text("a", "b", "c")


def test_the_same_entry_added_on_both_sides_is_kept_once() -> None:
    merged = efficiency_merge.merge(text("a"), text("a", "b"), text("a", "b", "c"))
    assert merged == text("a", "b", "c")


def test_one_side_editing_an_entry_keeps_the_edit_and_the_other_append() -> None:
    merged = efficiency_merge.merge(text("a"), text("a2"), text("a", "c"))
    assert merged == text("a2", "c")


def test_different_edits_to_one_entry_conflict() -> None:
    with pytest.raises(efficiency_merge.Conflict):
        efficiency_merge.merge(text("a"), text("a1"), text("a2"))


def test_a_side_that_is_not_json_conflicts_with_markers(tmp_path: Path) -> None:
    base, ours, theirs = (tmp_path / n for n in ("base", "ours", "theirs"))
    base.write_text(text("a"))
    ours.write_text("[\n<<<<\n")
    theirs.write_text(text("a", "c"))
    assert efficiency_merge.main([str(base), str(ours), str(theirs)]) == 1


def test_two_branches_rebase_with_both_appends(tmp_path: Path) -> None:
    """Two branches that each append an entry rebase with no conflict, through the driver as
    git runs it."""
    repo = tmp_path
    git(repo, "init", "-q", "-b", "staging")
    git(repo, "config", "user.email", "test@example.com")
    git(repo, "config", "user.name", "Test")
    (repo / ".gitattributes").write_text("log.json merge=efficiency-changes\n")
    script = Path(efficiency_merge.__file__)
    git(
        repo,
        "config",
        "merge.efficiency-changes.driver",
        shlex.join([Path(sys.executable).as_posix(), script.as_posix()])
        + " %O %A %B %P",
    )
    target = repo / "log.json"
    target.write_text(text("a"))
    git(repo, "add", ".")
    git(repo, "commit", "-q", "-m", "base")
    git(repo, "checkout", "-q", "-b", "one")
    target.write_text(text("a", "one"))
    git(repo, "commit", "-q", "-am", "one")
    git(repo, "checkout", "-q", "staging")
    target.write_text(text("a", "two"))
    git(repo, "commit", "-q", "-am", "two")
    git(repo, "checkout", "-q", "one")
    git(repo, "rebase", "-q", "staging")
    assert target.read_text() == text("a", "two", "one")
