import json
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace
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


def test_lists_games_per_day_without_reports(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(bugs, "app_data_dir", lambda: tmp_path)
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path / "bugs"))
    now = datetime(2026, 11, 11, tzinfo=timezone.utc)
    monkeypatch.setattr(bugs, "datetime", SimpleNamespace(now=lambda _zone: now))
    today = now.date()
    first = today - timedelta(days=13)
    outside = first - timedelta(days=1)
    (tmp_path / "games.jsonl").write_text(
        "\n".join(
            [
                json.dumps({"date": today.isoformat()}),
                json.dumps({"date": today.isoformat()}),
                json.dumps({"date": first.isoformat()}),
                json.dumps({"date": outside.isoformat()}),
                "interrupted write",
            ]
        ),
        encoding="utf-8",
    )
    assert cli.main(["bugs"]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert lines[0] == "Finished human games (UTC)"
    assert lines[1].split() == [str(first), "1"]
    assert lines[13].split() == [str(today - timedelta(days=1)), "0"]
    assert lines[14].split() == [str(today), "2"]
    assert "no reports" in lines[15]


def test_file_and_done(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder = report(tmp_path)
    calls: list[dict[str, object]] = []

    def create(_client: str, fields: dict[str, object], *, request_key: str) -> str:
        calls.append(fields)
        body = str(fields["body"])
        assert "The arrow\njumps." in body and "seed 77, turn 5" in body
        return "by-x1"

    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(bugs.tickets, "list_issues", lambda *_: [])
    monkeypatch.setattr(bugs.tickets, "create_issue", create)
    r = bugs.load(folder)
    assert r is not None and r.title == "Arrow jumps: on block"

    assert cli.main(["bugs", "file"]) == 0
    assert calls[0]["title"] == "Arrow jumps: on block"
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
    calls: list[dict[str, object]] = []

    def create(_client: str, fields: dict[str, object], *, request_key: str) -> str:
        calls.append(fields)
        assert "docs/incidents/" in str(fields["body"])
        return "by-ai1"

    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(bugs.tickets, "list_issues", lambda *_: [])
    monkeypatch.setattr(bugs.tickets, "create_issue", create)
    assert cli.main(["bugs", "file"]) == 0
    assert calls[0]["labels"] == ["bug-report", "ai-play"]


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

    def create(_client: str, fields: dict[str, object], *, request_key: str) -> str:
        body = str(fields["body"])
        assert "bug in the spec viewer" in body
        assert page in body and "oust/review-0/theirs" in body
        assert "1280×800 CSS px at 2× scale" in body
        assert fields["labels"] == ["bug-report", "spec-viewer"]
        return "by-viewer"

    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(bugs.tickets, "list_issues", lambda *_: [])
    monkeypatch.setattr(bugs.tickets, "create_issue", create)
    assert cli.main(["bugs", "file"]) == 0
    saved = bugs.load(folder)
    assert saved is not None and saved.bead == "by-viewer"


def test_retry_recovers_existing_bead(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder: Path = report(tmp_path)

    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(
        bugs.tickets,
        "list_issues",
        lambda *_: [
            {"id": "by-recovered", "metadata": {"bug_report_id": folder.name}},
        ],
    )
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
    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(bugs.tickets, "list_issues", lambda *_: [])
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


def test_import_keeps_a_bead_this_tracker_has(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = tmp_path / "bugs"
    monkeypatch.setenv("BERYL_BUGS", str(root))
    synced: str = "20261108-143012-arrow-jumps"
    unsynced = "20261108-150000-other-report"
    bundle = tmp_path / "reports.zip"
    with ZipFile(bundle, "w") as archive:
        archive.writestr(
            "manifest.json",
            json.dumps(
                {
                    "format": "beryl-bug-reports",
                    "version": 1,
                    "reports": [synced, unsynced],
                }
            ),
        )
        for key, bead in ((synced, "by-sender"), (unsynced, "by-missing")):
            archive.writestr(
                f"reports/{key}/report.md",
                REPORT.replace("20261108-143012-arrow-jumps", key).replace(
                    "status: open\nbead:", f"status: filed\nbead: {bead}"
                ),
            )

    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(
        bugs.tickets,
        "list_issues",
        lambda *_: [
            {"id": "by-local", "metadata": {"bug_report_id": synced}},
            {"id": "by-unrelated", "metadata": {}},
        ],
    )
    assert cli.main(["bugs", "import", str(bundle)]) == 0
    kept = bugs.load(root / synced)
    reset = bugs.load(root / unsynced)
    assert kept is not None and kept.status == "filed" and kept.bead == "by-local"
    assert reset is not None and reset.status == "open" and not reset.bead


def test_import_with_unavailable_tracker_imports_as_open(
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
        archive.writestr(f"reports/{key}/report.md", REPORT)

    def unavailable() -> str:
        raise RuntimeError("ticket tracker unavailable")

    monkeypatch.setattr(bugs.tickets, "client", unavailable)
    assert cli.main(["bugs", "import", str(bundle)]) == 0
    saved = bugs.load(root / key)
    assert saved is not None and saved.status == "open" and not saved.bead


def test_filing_a_migrated_report_reuses_its_ticket(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_BUGS", str(tmp_path))
    folder = report(tmp_path)
    old_id = "by-old-ticket"
    monkeypatch.setattr(bugs.tickets, "client", lambda: "tk")
    monkeypatch.setattr(
        bugs.tickets,
        "list_issues",
        lambda *_: [
            {
                "id": "old-ticket",
                "metadata": {
                    "_enact_beads": {"external_ref": f"bug-report:{folder.name}"}
                },
            }
        ],
    )

    def unexpected_write(*_args: object, **_kwargs: object) -> str:
        pytest.fail("a migrated report must not create a second issue")

    monkeypatch.setattr(bugs.tickets, "create_issue", unexpected_write)
    assert cli.main(["bugs", "file"]) == 0
    saved = bugs.load(folder)
    assert saved is not None and saved.bead == "old-ticket"
    assert cli.main(["bugs", "done", old_id]) == 0
