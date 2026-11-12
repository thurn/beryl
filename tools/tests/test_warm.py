import os
import shutil
import time
from pathlib import Path

import pytest

from beryl_tools import caches, paths, warm


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
    assert warm.ensure(second) is False
    assert (second / "rust" / "target" / "debug" / "beryl").read_text() == "binary"
    assert (second / "ui" / "node_modules" / "react").read_text() == "react"


def test_a_new_checkouts_clone_first_prunes_a_full_disk(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    cache = tmp_path / "cache"
    monkeypatch.setenv("BERYL_CACHE", str(cache))
    monkeypatch.setattr(paths, "worktree_names", lambda: ["tree"])
    write(cache / "target" / "debug" / "beryl", "binary")
    incremental = write(
        cache / "target" / "debug" / "incremental" / "s-1" / "dep-graph"
    )
    gone = write(cache / "ui-shots" / "gone" / "shot.png", age=7200).parent
    os.utime(gone, (time.time() - 7200,) * 2)
    tree = checkout(tmp_path / "tree", "lock-v1")
    write(tree / "ui" / "node_modules" / warm.STAMP, warm.lock_key(tree))

    monkeypatch.setattr(caches, "free_space", lambda _: caches.LOW_DISK)
    warm.ensure(tree)
    assert gone.exists() and incremental.exists()

    shutil.rmtree(tree / "rust" / "target")
    monkeypatch.setattr(caches, "free_space", lambda _: caches.LOW_DISK - 1)
    warm.ensure(tree)
    assert not gone.exists() and not incremental.parent.parent.exists()
    assert (tree / "rust" / "target" / "debug" / "beryl").read_text() == "binary"


def write(path: Path, text: str = "", age: float = 0) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    stamp = time.time() - age
    os.utime(path, (stamp, stamp))
    return path


def test_a_target_with_nothing_to_clone_is_reported_cold(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    tree = checkout(tmp_path / "tree", "lock-v1")
    write(tree / "ui" / "node_modules" / warm.STAMP, warm.lock_key(tree))
    assert warm.ensure(tree) is True
    write(tree / "rust" / "target" / "debug" / "beryl", "built cold")
    assert warm.ensure(tree) is False


def test_the_tauri_target_is_cloned_only_for_the_tauri_cli(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    write(tmp_path / "cache" / "target" / "debug" / "beryl", "binary")
    write(tmp_path / "cache" / "target" / "tauri" / "debug" / "beryl", "app")
    tree = checkout(tmp_path / "tree", "lock-v1")
    write(tree / "ui" / "node_modules" / warm.STAMP, warm.lock_key(tree))
    target = tree / "rust" / "target"

    warm.ensure(tree)
    assert (target / "debug" / "beryl").read_text() == "binary"
    assert not (target / "tauri").exists()

    warm.ensure(tree, tauri=True)
    assert (target / "tauri" / "debug" / "beryl").read_text() == "app"


def test_saving_a_worktree_without_the_tauri_target_keeps_the_cached_one(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    write(tmp_path / "cache" / "target" / "tauri" / "debug" / "beryl", "app")
    tree = checkout(tmp_path / "tree", "lock-v1")
    write(tree / "rust" / "target" / "debug" / "beryl", "new binary")

    warm.save(tree)
    cached = tmp_path / "cache" / "target"
    assert (cached / "debug" / "beryl").read_text() == "new binary"
    assert (cached / "tauri" / "debug" / "beryl").read_text() == "app"


def test_saving_prunes_what_earlier_builds_left(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    tree = checkout(tmp_path / "tree", "lock-v1")
    target = tree / "rust" / "target"
    kept: list[str] = []
    gone: list[str] = []
    for profile in ("debug", "tauri/debug"):
        deps = target / profile / "deps"
        # Two builds of an incremental unit: only the newest one's objects stay.
        for cgu in ("0a1b", "2c3d"):
            gone.append(f"{profile}/deps/beryl_engine-1e06.{cgu}.08cgh6m.rcgu.o")
            write(deps / f"beryl_engine-1e06.{cgu}.08cgh6m.rcgu.o", age=3600)
            kept.append(f"{profile}/deps/beryl_engine-1e06.{cgu}.04pcmlt.rcgu.o")
            write(deps / f"beryl_engine-1e06.{cgu}.04pcmlt.rcgu.o")
        # Another unit of the same crate, built only long ago, and a dependency's objects.
        kept.append(f"{profile}/deps/beryl_engine-efea.0a1b.1sssjqw.rcgu.o")
        write(deps / "beryl_engine-efea.0a1b.1sssjqw.rcgu.o", age=7200)
        kept.append(f"{profile}/deps/serde-91.serde.79a6-cgu.00.rcgu.o")
        write(deps / "serde-91.serde.79a6-cgu.00.rcgu.o", age=7200)
        kept.append(f"{profile}/deps/libberyl_engine-1e06.rlib")
        write(deps / "libberyl_engine-1e06.rlib", age=3600)
        # Two finished incremental sessions and one in progress: the newest finished one stays.
        crate = f"{profile}/incremental/beryl_engine-0qkw"
        for session in ("s-hnvjezultb-1esuecj", "s-hnvjjl34q9-0nnpdpx"):
            write(target / crate / f"{session}.lock")
        write(target / crate / "s-hnvjezultb-1esuecj-efc722roy" / "a.o")
        write(target / crate / "s-hnvjjl34q9-0nnpdpx-7fp5ric0m" / "a.o")
        write(target / crate / "s-hnvjk00000-0aaaaaa-working" / "a.o")
        gone += [
            f"{crate}/s-hnvjezultb-1esuecj.lock",
            f"{crate}/s-hnvjezultb-1esuecj-efc722roy",
        ]
        kept += [
            f"{crate}/s-hnvjjl34q9-0nnpdpx.lock",
            f"{crate}/s-hnvjjl34q9-0nnpdpx-7fp5ric0m/a.o",
            f"{crate}/s-hnvjk00000-0aaaaaa-working/a.o",
        ]

    warm.save(tree)
    cached = tmp_path / "cache" / "target"
    assert [p for p in kept if not (cached / p).exists()] == []
    assert [p for p in gone if (cached / p).exists()] == []
