import subprocess
from pathlib import Path

import pytest

from beryl_tools import changes as recheck


def git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=root, check=True, capture_output=True, text=True
    ).stdout.strip()


def commit(root: Path, path: str) -> str:
    (root / path).parent.mkdir(parents=True, exist_ok=True)
    (root / path).write_text(path)
    git(root, "add", path)
    git(root, "commit", "-q", "-m", path)
    return git(root, "rev-parse", "HEAD")


def lanes_for(changed: list[str] | None) -> list[str]:
    """Lanes by top folder, as checks.fast_lanes chooses them, with `repo` always."""
    tops = {p.split("/")[0] for p in changed or []}
    return ["repo", *sorted(tops)]


def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    git(tmp_path, "init", "-q", "-b", "staging")
    git(tmp_path, "config", "user.email", "t@example.com")
    git(tmp_path, "config", "user.name", "t")
    monkeypatch.setattr(recheck.paths, "REPO", tmp_path)


def test_a_recheck_runs_only_the_lanes_what_landed_reaches(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    repo(tmp_path, monkeypatch)
    commit(tmp_path, "rust/a.rs")
    git(tmp_path, "checkout", "-q", "-b", "work")
    commit(tmp_path, "tools/mine.py")
    checked = commit(tmp_path, "ui/mine.ts")
    git(tmp_path, "checkout", "-q", "staging")
    commit(tmp_path, "ui/theirs.ts")
    # The push rebases its branch onto the staging that moved.
    git(tmp_path, "checkout", "-q", "-b", "rebased")
    git(tmp_path, "cherry-pick", f"staging..{checked}")
    base = git(tmp_path, "merge-base", "staging", "HEAD")
    mine = ["repo", "tools", "ui"]
    # A first run checks every lane of the change.
    monkeypatch.delenv(recheck.CHECKED_ENV, raising=False)
    assert recheck.narrow(mine, base, lanes_for, str) == mine
    # After the race only the lanes the landed ui/ change also selects rerun.
    monkeypatch.setenv(recheck.CHECKED_ENV, checked)
    assert recheck.narrow(mine, base, lanes_for, str) == ["repo", "ui"]
    assert "reaches none of tools, which stand" in capsys.readouterr().out
    # A commit git cannot place reruns everything.
    monkeypatch.setenv(recheck.CHECKED_ENV, "0" * 40)
    assert recheck.narrow(mine, base, lanes_for, str) == mine


def test_a_batch_reruns_a_lane_only_where_two_checked_branches_meet(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # Enact's merge queue replays three branches, each checked on its own, onto staging.
    # Two touch rust/, so the rust lane runs again; only one touches ui/ or tools/, and
    # nothing else in the tree reaches those, so they stand.
    repo(tmp_path, monkeypatch)
    start = commit(tmp_path, "README")
    branches: dict[str, str] = {}
    for branch, paths in {
        "a": ["rust/a.rs", "ui/a.ts"],
        "b": ["rust/b.rs"],
        "c": ["tools/c.py"],
    }.items():
        git(tmp_path, "checkout", "-q", "-b", branch, start)
        for path in paths:
            branches[branch] = commit(tmp_path, path)
    git(tmp_path, "checkout", "-q", "staging")
    base = git(tmp_path, "rev-parse", "HEAD")
    git(tmp_path, "checkout", "-q", "-b", "batch")
    for head in branches.values():
        git(tmp_path, "cherry-pick", f"{start}..{head}")
    lanes = ["repo", "rust", "tools", "ui"]
    monkeypatch.setenv(recheck.CHECKED_ENV, " ".join(branches.values()))
    assert recheck.narrow(lanes, base, lanes_for, str) == ["repo", "rust"]
    # A lone branch on the tip it was checked on needs nothing again but the repo lane.
    monkeypatch.setenv(recheck.CHECKED_ENV, branches["c"])
    git(tmp_path, "checkout", "-q", "c")
    assert recheck.narrow(["repo", "tools"], start, lanes_for, str) == ["repo"]
