import json
from pathlib import Path
from zipfile import ZipFile

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
        if args[0] == "list":
            return "[]"
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
        if args[0] == "list":
            return "[]"
        calls.append(args)
        body = Path(args[args.index("--body-file") + 1]).read_text()
        assert "docs/incidents/" in body
        return "by-ai1"

    monkeypatch.setattr(bugs, "run_bd", fake_bd)
    assert cli.main(["bugs", "file"]) == 0
    assert calls[0][calls[0].index("--labels") + 1] == "bug-report,ai-play"


def test_spec_viewer_report_files_reproduction_context(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder = report(tmp_path)
    md = folder / "report.md"
    page: str = "/specs/oust#review-0-theirs"
    md.write_text(
        REPORT.replace('url: "http://localhost/#/game"', f'url: "{page}"').replace(
            "status: open",
            "kind: spec-viewer\nspec: oust/review-0/theirs\n"
            "viewport: 1280×800 CSS px at 2× scale\nstatus: open",
        )
    )

    def fake_bd(args: list[str]) -> str:
        if args[0] == "list":
            return "[]"
        body = Path(args[args.index("--body-file") + 1]).read_text()
        assert "bug in the spec viewer" in body
        assert page in body and "oust/review-0/theirs" in body
        assert "1280×800 CSS px at 2× scale" in body
        assert args[args.index("--labels") + 1] == "bug-report,spec-viewer"
        return "by-viewer"

    monkeypatch.setattr(bugs, "run_bd", fake_bd)
    assert cli.main(["bugs", "file"]) == 0
    saved = bugs.load(folder)
    assert saved is not None and saved.bead == "by-viewer"


def test_retry_recovers_existing_bead(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder: Path = report(tmp_path)

    def fake_bd(args: list[str]) -> str:
        assert args == [
            "list",
            "--all",
            "--external-ref",
            f"bug-report:{folder.name}",
            "--json",
        ]
        return '[{"id":"by-recovered"}]'

    monkeypatch.setattr(bugs, "run_bd", fake_bd)
    assert cli.main(["bugs", "file", "--", folder.name]) == 0
    saved = bugs.load(folder)
    assert (
        saved is not None and saved.status == "filed" and saved.bead == "by-recovered"
    )


def test_import_resets_sender_status_and_preserves_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = tmp_path / "bugs"
    monkeypatch.setenv("BERYL_BUGS", str(root))
    key = "20261108-143012-arrow-jumps"
    bundle = tmp_path / "reports.zip"
    with ZipFile(bundle, "w") as archive:
        archive.writestr(
            "manifest.json",
            json.dumps({"format": "beryl-bug-reports", "version": 1, "reports": [key]}),
        )
        archive.writestr(
            f"reports/{key}/report.md",
            REPORT.replace("status: open\nbead:", "status: filed\nbead: by-sender"),
        )
        archive.writestr(f"reports/{key}/replay.json", "{}")

    assert cli.main(["bugs", "import", str(bundle)]) == 0
    folder = root / key
    saved = bugs.load(folder)
    assert saved is not None and saved.status == "open" and not saved.bead
    assert (folder / "replay.json").read_text() == "{}"
    assert cli.main(["bugs", "done", key]) == 0
    assert cli.main(["bugs", "import", str(bundle)]) == 0
    assert not folder.exists()

    with ZipFile(bundle, "a") as archive:
        archive.writestr(f"reports/{key}/../../unexpected", "unexpected")
    assert cli.main(["bugs", "import", str(bundle)]) == 1
    assert not (root / "unexpected").exists()
