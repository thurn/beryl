from pathlib import Path

import pytest

from beryl_tools import bugs, cli

REPORT = """---
id: 20261108-143012-arrow-jumps
title: "Arrow jumps: on block"
status: open
bead:
created: 2026-11-08T14:30:12.000Z
commit: abc123
url: "http://localhost/#/game"
session: 2
seed: 77
turn: 5
step: declareBlockers
decision: 40
---

# Arrow jumps: on block

The arrow
jumps.

![What the player saw](screenshot.png)

## Reproduce
"""


def report(root: Path, name: str = "20261108-143012-arrow-jumps") -> Path:
    folder = root / name
    folder.mkdir(parents=True)
    (folder / "report.md").write_text(
        REPORT.replace("20261108-143012-arrow-jumps", name)
    )
    return folder


def test_file_and_done(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder = report(tmp_path)
    calls: list[list[str]] = []

    def fake_bd(args: list[str]) -> str:
        calls.append(args)
        body = Path(args[args.index("--body-file") + 1]).read_text()
        assert "The arrow\njumps." in body and "seed 77, turn 5" in body
        return "by-x1"

    monkeypatch.setattr(bugs, "run_bd", fake_bd)
    r = bugs.load(folder)
    assert r is not None and r.title == "Arrow jumps: on block"

    assert cli.main(["bugs", "file"]) == 0
    assert calls[0][calls[0].index("--title") + 1] == "Arrow jumps: on block"
    filed = bugs.load(folder)
    assert filed is not None and filed.status == "filed" and filed.bead == "by-x1"
    # Filed once only.
    assert cli.main(["bugs", "file"]) == 0 and len(calls) == 1

    assert cli.main(["bugs", "done", "by-x1", "--commit", "deadbeef"]) == 0
    moved = bugs.load(tmp_path / "done" / folder.name)
    assert moved is not None and moved.status == "done"
    assert moved.fields["fix"] == "deadbeef"
    assert bugs.reports(tmp_path) == []
    assert len(bugs.reports(tmp_path, done=True)) == 1


def test_an_ai_report_files_an_incident_bead(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder = report(tmp_path)
    md = folder / "report.md"
    md.write_text(md.read_text().replace("status: open", "kind: ai\nstatus: open"))
    calls: list[list[str]] = []

    def fake_bd(args: list[str]) -> str:
        calls.append(args)
        body = Path(args[args.index("--body-file") + 1]).read_text()
        assert "docs/incidents/" in body
        return "by-ai1"

    monkeypatch.setattr(bugs, "run_bd", fake_bd)
    assert cli.main(["bugs", "file"]) == 0
    assert calls[0][calls[0].index("--labels") + 1] == "bug-report,ai-play"
