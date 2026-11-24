"""`just client clear-save`: which replay it sets aside, and which it leaves alone."""

import json
from pathlib import Path

import pytest

from beryl_tools import bugs
from beryl_tools.client import clear_save


def replay(folder: Path, name: str, log: list[object]) -> Path:
    path = folder / name
    path.write_text(json.dumps({"formatVersion": 13, "log": log}))
    return path


def test_the_newest_unfinished_game_moves_to_cleared(tmp_path: Path) -> None:
    older = replay(tmp_path, "1794681021.330602000-1.json", [{"choice": {}}])
    # The newest by the start time in its name, not by the order names sort in.
    newest = replay(tmp_path, "1794882140.289415000-2.json", [{"choice": {}}])
    report = clear_save.clear(tmp_path)
    assert not newest.exists()
    assert (tmp_path / "cleared" / newest.name).exists()
    assert older.exists()
    assert "start screen" in report


def test_a_conceded_newest_game_is_left_alone(tmp_path: Path) -> None:
    newest = replay(
        tmp_path, "1794686649.691298000-3.json", [{"choice": {}}, "concede"]
    )
    assert "conceded" in clear_save.clear(tmp_path)
    assert newest.exists()


def test_files_the_apps_would_not_resume_are_ignored(tmp_path: Path) -> None:
    replay(tmp_path, "notes.json", [])
    (tmp_path / "1794882140.1-4.txt").write_text("")
    assert "no saved games" in clear_save.clear(tmp_path)
    assert "no saved games" in clear_save.clear(tmp_path / "missing")


def test_a_valid_profile_names_its_own_folder(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(bugs, "app_data_dir", lambda: tmp_path)
    assert clear_save.data_dir("Seat-1") == tmp_path / "profiles" / "seat-1"
    assert clear_save.data_dir(None) == tmp_path
    assert clear_save.data_dir("bad name") == tmp_path
    assert clear_save.data_dir("COM1") == tmp_path
