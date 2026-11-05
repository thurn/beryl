from pathlib import Path

import pytest

from beryl_tools import warm


def checkout(root: Path, lock: str) -> Path:
    (root / "ui").mkdir(parents=True)
    (root / "ui" / "package-lock.json").write_text(lock)
    return root


def test_a_saved_worktree_seeds_the_next_one(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    first = checkout(tmp_path / "first", "lock-v1")
    (first / "rust" / "target" / "debug").mkdir(parents=True)
    (first / "rust" / "target" / "debug" / "beryl").write_text("binary")
    modules = first / "ui" / "node_modules"
    modules.mkdir()
    (modules / "react").write_text("react")
    (modules / warm.STAMP).write_text(warm.lock_key(first))

    warm.save(first)
    assert not (first / "rust" / "target").exists() and not modules.exists()

    second = checkout(tmp_path / "second", "lock-v1")
    warm.ensure(second)
    assert (second / "rust" / "target" / "debug" / "beryl").read_text() == "binary"
    assert (second / "ui" / "node_modules" / "react").read_text() == "react"
