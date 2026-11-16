import os
import time
from pathlib import Path

import pytest

from beryl_tools import caches, system, target_sweep

HOUR = 3600.0


def unit(profile: Path, name: str, built: float, read: bool) -> None:
    """A unit `name` (`<crate>-<hash>`) of `profile` built at `built`, whose fingerprint was
    `read` since its last mark or not, with an rlib and a build folder."""
    fingerprint = profile / ".fingerprint" / name
    fingerprint.mkdir(parents=True, exist_ok=True)
    (profile / "deps").mkdir(exist_ok=True)
    (profile / "build" / name).mkdir(parents=True, exist_ok=True)
    crate, hash = name.rsplit("-", 1)
    for path in (
        fingerprint / f"lib-{crate}",
        fingerprint / f"lib-{crate}.json",
        profile / "deps" / f"lib{name}.rlib",
    ):
        path.write_text('{"profile": 1, "features": "[]"}')
        os.utime(path, (built + 1 if read else built - 1, built))


def marked_target(tmp_path: Path, hours_ago: float) -> tuple[Path, float]:
    target = tmp_path / "target"
    (target / "debug").mkdir(parents=True)
    marked = time.time() - hours_ago * HOUR
    (target / target_sweep.MARK).touch()
    os.utime(target / target_sweep.MARK, (marked, marked))
    return target, marked


def names(folder: Path) -> list[str]:
    return sorted(p.name for p in folder.iterdir())


def test_a_build_a_newer_build_of_its_crate_replaced_is_swept(tmp_path: Path) -> None:
    target, marked = marked_target(tmp_path, 7)
    debug = target / "debug"
    old = "beryl_scene-" + "a" * 16
    new = "beryl_scene-" + "b" * 16
    fresh = "serde-" + "c" * 16
    unit(debug, old, marked - 2 * HOUR, read=False)
    unit(debug, new, marked + HOUR, read=False)  # Built after the mark.
    unit(debug, fresh, marked - 9 * HOUR, read=True)  # Built long ago, read since.
    assert target_sweep.sweep(target) == 3  # Its rlib, build and fingerprint.
    assert names(debug / "deps") == [f"lib{new}.rlib", f"lib{fresh}.rlib"]
    assert names(debug / ".fingerprint") == [new, fresh]
    assert names(debug / "build") == [new, fresh]


def test_a_target_nobody_built_in_since_the_mark_loses_nothing(
    tmp_path: Path,
) -> None:
    # A slot left idle overnight: no build read any unit, so none is known to be replaced.
    target, marked = marked_target(tmp_path, 30)
    debug = target / "debug"
    unit(debug, "beryl_scene-" + "a" * 16, marked - 9 * HOUR, read=False)
    unit(debug, "beryl_scene-" + "b" * 16, marked - 2 * HOUR, read=False)
    assert target_sweep.sweep(target) == 0
    assert len(names(debug / "deps")) == 2


def test_a_sweep_marks_every_unit_unread_for_the_next(tmp_path: Path) -> None:
    target, marked = marked_target(tmp_path, 7)
    debug = target / "debug"
    name = "serde-" + "c" * 16
    unit(debug, name, marked - HOUR, read=True)
    target_sweep.sweep(target)
    stat = (debug / ".fingerprint" / name / "lib-serde").stat()
    assert stat.st_atime < stat.st_mtime
    assert not target_sweep.sweep_due(target, time.time())


def test_an_incremental_cache_unwritten_while_its_crate_was_rebuilt_is_swept(
    tmp_path: Path,
) -> None:
    target, marked = marked_target(tmp_path, 7)
    incremental = target / "debug" / "incremental"
    for name, written in (
        ("beryl_scene-old", marked - HOUR),
        ("beryl_scene-new", marked + HOUR),
        ("serde-idle", marked - HOUR),  # No cache of its crate was written: it stays.
    ):
        session = incremental / name / "s-abc-def"
        session.mkdir(parents=True)
        os.utime(session, (written, written))
    target_sweep.sweep(target)
    assert names(incremental) == ["beryl_scene-new", "serde-idle"]


def test_a_test_build_is_not_replaced_by_a_library_build(tmp_path: Path) -> None:
    # Each kind of unit of a crate (its library, its tests, clippy's check of it) is replaced
    # only by a newer build of the same kind.
    target, marked = marked_target(tmp_path, 7)
    debug = target / "debug"
    unit(debug, "beryl_scene-" + "a" * 16, marked + HOUR, read=False)
    tests = debug / ".fingerprint" / ("beryl_scene-" + "b" * 16)
    unit(debug, "beryl_scene-" + "b" * 16, marked - HOUR, read=False)
    (tests / "lib-beryl_scene").rename(tests / "test-lib-beryl_scene")
    (tests / "lib-beryl_scene.json").rename(tests / "test-lib-beryl_scene.json")
    assert target_sweep.sweep(target) == 0


def test_tidying_sweeps_only_when_the_mark_is_old(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    swept: list[Path] = []
    monkeypatch.setattr(target_sweep, "sweep", lambda t: len(swept.append(t) or swept))
    target, _ = marked_target(tmp_path, 1)
    assert not caches.tidy_target(target)
    os.utime(target / target_sweep.MARK, (0, 0))
    assert caches.tidy_target(target)
    assert swept == [target]


def test_tidying_leaves_a_target_a_build_holds(tmp_path: Path) -> None:
    target, _ = marked_target(tmp_path, 7)
    with system.file_lock(target / "debug" / ".cargo-lock"):
        assert not caches.tidy_target(target)
    assert target_sweep.sweep_due(target, time.time())


def test_tidying_leaves_a_target_whose_clippy_build_runs(tmp_path: Path) -> None:
    target, _ = marked_target(tmp_path, 7)
    (target / "clippy" / "debug").mkdir(parents=True)
    with system.file_lock(target / "clippy" / "debug" / ".cargo-lock"):
        assert not caches.tidy_target(target)
    assert target_sweep.sweep_due(target, time.time())
