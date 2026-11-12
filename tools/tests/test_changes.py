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


def test_a_recheck_runs_only_the_lanes_what_landed_reaches(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    git(tmp_path, "init", "-q", "-b", "staging")
    git(tmp_path, "config", "user.email", "t@example.com")
    git(tmp_path, "config", "user.name", "t")
    old = commit(tmp_path, "rust/a.rs")
    git(tmp_path, "checkout", "-q", "-b", "work")
    checked = commit(tmp_path, "tools/mine.py")
    git(tmp_path, "checkout", "-q", "staging")
    base = commit(tmp_path, "ui/theirs.ts")
    monkeypatch.setattr(recheck.paths, "REPO", tmp_path)
    mine = ["repo", "tools", "ui"]
    # A first run checks every lane of the change.
    monkeypatch.delenv(recheck.CHECKED_ENV, raising=False)
    assert recheck.narrow(mine, base, lanes_for, str) == mine
    # After the race only the lanes the landed ui/ change also selects rerun.
    monkeypatch.setenv(recheck.CHECKED_ENV, checked)
    assert recheck.narrow(mine, base, lanes_for, str) == ["repo", "ui"]
    assert "reach none of tools, which stand" in capsys.readouterr().out
    # A commit git cannot place reruns everything.
    monkeypatch.setenv(recheck.CHECKED_ENV, "0" * 40)
    assert recheck.narrow(mine, base, lanes_for, str) == mine
    assert old
