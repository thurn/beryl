"""`just client bug`: what it prints for report folders built by hand, from a replayed recording,
from a live game and from the web interface."""

import json
from pathlib import Path

from beryl_tools.client import bug


def report_folder(root: Path, log: object, replay: bool = False) -> Path:
    folder = root / "20261116-120000-abc-the-pay-bar"
    folder.mkdir(parents=True)
    (folder / "report.md").write_text(
        "---\nid: 20261116-120000-abc-the-pay-bar\nstatus: open\ncommit: 0309dfe\n---\n\n"
        "# the pay bar\n\nthe pay bar never filled\n\n![What the player saw](screenshot.png)\n"
    )
    (folder / "ui.json").write_text(json.dumps({"page": {}, "log": log}))
    (folder / "tree.txt").write_text(
        "\n".join(f"line {n}" for n in range(bug.TREE_LINES + 5))
    )
    if replay:
        (folder / "replay.json").write_text("{}")
    return folder


def printed(folder: Path) -> str:
    opened = bug.open_report(str(folder), folder.parent)
    assert opened is not None
    return bug.text(opened)


def test_a_replayed_report_prints_the_recordings_moment(tmp_path: Path) -> None:
    folder = report_folder(
        tmp_path,
        {"client": "Beryl", "recording": "pay-default", "seat": "you", "at": 1000},
    )
    out = printed(folder)
    assert "the pay bar never filled" in out
    assert "built from 0309dfe" in out
    assert "just client tree pay-default --seat you --at 1000ms" in out
    assert "just client png pay-default --seat you --at 1000ms" in out
    assert f"line {bug.TREE_LINES - 1}\n" in out
    assert f"line {bug.TREE_LINES}\n" not in out


def test_a_report_saved_after_the_replay_ended_prints_its_end(tmp_path: Path) -> None:
    log = {"client": "Beryl", "recording": "pay-default", "seat": "opponent"}
    folder = report_folder(tmp_path, {**log, "at": 1783, "ended": True})
    assert "just client tree pay-default --seat opponent --at end" in printed(folder)


def test_a_live_report_prints_the_resume_command(tmp_path: Path) -> None:
    folder = report_folder(tmp_path, {"client": "Beryl"}, replay=True)
    out = printed(folder)
    assert (
        f"just client app --replay {folder / 'replay.json'} --window none --for 120s"
        in out
    )
    assert "just client live tree" in out


def test_a_report_from_before_the_rename_is_the_clients(tmp_path: Path) -> None:
    folder = report_folder(tmp_path, {"client": "Beryl 3D"}, replay=True)
    out = printed(folder)
    assert "a web report" not in out
    assert "just client app --replay" in out


def test_a_web_report_says_so(tmp_path: Path) -> None:
    folder = report_folder(tmp_path, [{"kind": "route"}], replay=True)
    out = printed(folder)
    assert "a web report" in out
    assert "bugfix" in out
    assert "just client" not in out


def test_a_short_description_is_the_title(tmp_path: Path) -> None:
    folder = report_folder(tmp_path, {"client": "Beryl"})
    (folder / "report.md").write_text(
        '---\ntitle: "pay bar check"\n---\n\n# pay bar check\n\n\n\n![What](screenshot.png)\n'
    )
    assert "\npay bar check\n" in printed(folder)


def test_a_report_is_found_by_its_id_prefix(tmp_path: Path) -> None:
    report_folder(tmp_path, {"client": "Beryl"})
    opened = bug.open_report("20261116-120000", tmp_path)
    assert opened is not None
    assert opened.report.id == "20261116-120000-abc-the-pay-bar"
