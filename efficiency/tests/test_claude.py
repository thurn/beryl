"""Claude Code transcripts: requests counted once, tool calls with their results, and every row
placed in the ticket window it belongs to."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import bead, claude, cli, ingest, store
from tests.legacy_sources import Beads
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record

HOST = "jarvis"
ENACT = "python3 ~/enact/scripts/enact.py"


def sources(root: Path, beads: bytes = b"[]") -> list[ingest.Source]:
    return [
        Beads(lambda: beads),
        claude.Sessions(root / "sessions", CHECKOUT, HOST),
        claude.Transcripts(root / "projects", CHECKOUT, HOST),
    ]


def transcript(root: Path, name: str, folder: str = PROJECT) -> Transcript:
    return Transcript(root / "projects" / folder / f"{name}.jsonl")


def open_db(root: Path) -> sqlite3.Connection:
    return store.open_store(root / "store.sqlite")


def windows(
    db: sqlite3.Connection, name: str
) -> list[tuple[str, str, str | None, str]]:
    return [
        (str(r[0]), str(r[1]), r[2], str(r[3]))
        for r in db.execute(
            "SELECT start, end, bead, kind FROM windows WHERE transcript = ? ORDER BY start",
            (name,),
        )
    ]


def beads_of(db: sqlite3.Connection, table: str) -> dict[str, str | None]:
    key = "message" if table == "requests" else "call"
    return {str(r[0]): r[1] for r in db.execute(f"SELECT {key}, bead FROM {table}")}


def lane(root: Path) -> Path:
    """A session that reads its instructions, works by-a, continues to by-b in its lane,
    closes it and then does something else."""
    return (
        transcript(root, "t1")
        .request("2026-11-15T10:00:00.000Z", "m0")
        .bash(
            "2026-11-15T10:00:01.000Z",
            "m1",
            "c1",
            f"cd /work/beryl && {ENACT} start by-a",
        )
        .result("2026-11-15T10:00:02.000Z", "c1", '{"bead": "by-a"}')
        .request("2026-11-15T10:10:00.000Z", "m2")
        .bash(
            "2026-11-15T10:30:00.000Z",
            "m3",
            "c3",
            f'{ENACT} close by-a --reason "done" --continue',
        )
        .result(
            "2026-11-15T10:30:05.000Z",
            "c3",
            '{"next": {"description": "a \\"id\\": \\"by-c\\"", "id": "by-b"}}',
        )
        .request("2026-11-15T10:40:00.000Z", "m4")
        .bash("2026-11-15T11:00:00.000Z", "m5", "c5", f"{ENACT} close by-b --reason x")
        .result("2026-11-15T11:00:01.000Z", "c5")
        .request("2026-11-15T11:05:00.000Z", "m6")
        .save()
    )


def test_one_message_on_several_lines_is_counted_once(tmp_path: Path) -> None:
    transcript(tmp_path, "t1").request(
        "2026-11-15T10:00:00.000Z",
        "m1",
        usage=(3, 40, 500, 7),
        tools=(
            ("c1", "Read", {"file_path": "/work/beryl/a.rs"}),
            ("c2", "Bash", {"command": "ls"}),
        ),
    ).save()
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path), HOST)
    rows = db.execute(
        "SELECT count(*), sum(input_tokens), sum(cache_write_tokens), "
        "sum(cache_read_tokens), sum(output_tokens), sum(context_tokens) FROM requests"
    ).fetchone()
    assert tuple(rows) == (1, 3, 40, 500, 7, 543)
    calls = db.execute(
        "SELECT call, tool, command FROM tool_calls ORDER BY call"
    ).fetchall()
    assert [tuple(r) for r in calls] == [
        ("c1", "Read", "/work/beryl/a.rs"),
        ("c2", "Bash", "ls"),
    ]


def test_a_lane_gives_two_windows_with_no_overlap_and_no_gap(tmp_path: Path) -> None:
    lane(tmp_path)
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path), HOST)
    assert windows(db, "t1") == [
        # The first window starts at the first record, so m0 joins it.
        ("2026-11-15T10:00:00.000Z", "2026-11-15T10:30:05.000Z", "by-a", "bead"),
        ("2026-11-15T10:30:05.000Z", "2026-11-15T11:00:01.000Z", "by-b", "bead"),
        ("2026-11-15T11:00:01.000Z", "2026-11-15T11:05:00.000Z", None, "none"),
    ]
    assert beads_of(db, "requests") == {
        "m0": "by-a",
        "m1": "by-a",
        "m2": "by-a",
        "m3": "by-a",
        "m4": "by-b",
        "m5": "by-b",
        "m6": None,
    }
    ended = db.execute("SELECT end, result_bytes FROM tool_calls WHERE call = 'c5'")
    assert tuple(ended.fetchone()) == ("2026-11-15T11:00:01.000Z", 2)


def test_a_subagent_joins_its_parents_window(tmp_path: Path) -> None:
    lane(tmp_path)
    sub = tmp_path / "projects" / PROJECT / "t1" / "subagents" / "agent-x.jsonl"
    Transcript(sub).request("2026-11-15T10:45:00.000Z", "s1").request(
        "2026-11-15T11:20:00.000Z", "s2"
    ).save()
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path), HOST)
    found = beads_of(db, "requests")
    # Its later request is past the parent's window, but the whole subagent is by-b's.
    assert (found["s1"], found["s2"]) == ("by-b", "by-b")
    parent = db.execute(
        "SELECT parent FROM transcripts WHERE transcript = 't1/agent-x'"
    )
    assert parent.fetchone()[0] == "t1"


def test_an_orchestrators_cost_goes_to_its_epic(tmp_path: Path) -> None:
    transcript(tmp_path, "v1").request("2026-11-15T09:00:00.000Z", "o1").bash(
        "2026-11-15T09:05:00.000Z", "o2", "c1", f"{ENACT} next --epic by-e"
    ).save()
    beads = listing(
        bead_record("by-e", issue_type="epic"),
        bead_record("by-e.1", parent="by-e", metadata={"enact_vizier": "v1"}),
    )
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path, beads), HOST)
    assert windows(db, "v1") == [
        (
            "2026-11-15T09:00:00.000Z",
            "2026-11-15T09:05:00.000Z",
            "by-e",
            "orchestration",
        )
    ]
    assert set(beads_of(db, "requests").values()) == {"by-e"}


def test_a_session_with_no_bead_is_named_by_its_worktree(tmp_path: Path) -> None:
    transcript(tmp_path, "w1").bash(
        "2026-11-15T09:00:00.000Z", "m1", "c1", f"{ENACT} worktree create fix-thing"
    ).request("2026-11-15T09:10:00.000Z", "m2").save()
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path), HOST)
    found = db.execute("SELECT kind, bead, label FROM windows WHERE transcript = 'w1'")
    assert [tuple(r) for r in found] == [("none", None, "fix-thing")]


def test_ingest_reads_each_line_once_and_waits_for_a_partial_one(
    tmp_path: Path,
) -> None:
    path = lane(tmp_path)
    whole = path.read_bytes()
    cut = whole.rstrip(b"\n").rfind(b"\n") + 1  # The start of the last line, m6.
    path.write_bytes(whole[: cut + 40])
    db = open_db(tmp_path)
    first = ingest.ingest(db, sources(tmp_path), HOST).sources["claude"]
    assert first.bytes_read == cut and first.skipped == 0
    assert "m6" not in beads_of(db, "requests")

    assert ingest.ingest(db, sources(tmp_path), HOST).sources["claude"].bytes_read == 0

    path.write_bytes(whole)
    third = ingest.ingest(db, sources(tmp_path), HOST).sources["claude"]
    assert third.bytes_read == len(whole) - cut
    assert beads_of(db, "requests")["m6"] is None
    assert windows(db, "t1")[-1][2:] == (None, "none")
    assert ingest.ingest(db, sources(tmp_path), HOST).sources["claude"].bytes_read == 0


def test_a_continue_whose_result_was_cut_short_opens_the_next_named_bead(
    tmp_path: Path,
) -> None:
    transcript(tmp_path, "t3").bash(
        "2026-11-15T09:00:00.000Z", "m1", "c1", f"{ENACT} start by-a"
    ).bash(
        "2026-11-15T09:10:00.000Z",
        "m2",
        "c2",
        f"{ENACT} close by-a --reason r --continue | tail -3",
    ).result(
        "2026-11-15T09:10:02.000Z", "c2", '    "labels": []\n  }\n}'
    ).bash(
        "2026-11-15T09:12:00.000Z", "m3", "c3", f"{ENACT} worktree create by-b"
    ).bash(
        "2026-11-15T09:30:00.000Z", "m4", "c4", f"{ENACT} close by-b --reason r"
    ).save()
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path), HOST)
    assert windows(db, "t3") == [
        ("2026-11-15T09:00:00.000Z", "2026-11-15T09:10:02.000Z", "by-a", "bead"),
        ("2026-11-15T09:10:02.000Z", "2026-11-15T09:30:00.000Z", "by-b", "bead"),
    ]
    assert cli.gaps(db)["stray_closes"] == 0


def test_only_real_enact_commands_count() -> None:
    assert claude.enact_commands(
        f"cd /work && timeout 100 {ENACT} start by-ig5e.2 --reservation abc"
    ) == [("open", "by-ig5e.2")]
    assert claude.enact_commands(
        "enact claim by-x.1 && enact close by-x.1 --reason r"
    ) == [
        ("open", "by-x.1"),
        ("close", "by-x.1"),
    ]
    assert claude.enact_commands(
        'E=\'"/opt/python3" "/home/enact/scripts/enact.py"\'; eval $E start by-a --reservation r'
    ) == [("open", "by-a")]
    assert claude.enact_commands("grep -n 'enact start by-a' notes.txt") == []
    assert claude.enact_commands("git log | grep enact") == []


def test_a_close_with_no_open_window_is_ignored_and_counted(tmp_path: Path) -> None:
    transcript(tmp_path, "t2").bash(
        "2026-11-15T09:00:00.000Z", "m1", "c1", f"{ENACT} close by-z --reason r"
    ).bash("2026-11-15T09:01:00.000Z", "m2", "c2", f"{ENACT} start by-y").bash(
        "2026-11-15T09:02:00.000Z", "m3", "c3", f"{ENACT} close by-y --reason r"
    ).bash(
        # A retried close of the ticket just closed is not counted.
        "2026-11-15T09:03:00.000Z",
        "m4",
        "c4",
        f"{ENACT} close by-y --reason r",
    ).save()
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path), HOST)
    assert [w[2:] for w in windows(db, "t2")] == [("by-y", "bead"), (None, "none")]
    assert cli.gaps(db)["stray_closes"] == 1


def test_session_records_name_transcripts_and_the_bead_query_reads_them(
    tmp_path: Path,
) -> None:
    lane(tmp_path)
    record = tmp_path / "sessions" / "a" / "b" / "local_s1.json"
    record.parent.mkdir(parents=True)
    record.write_text(
        json.dumps(
            {
                "sessionId": "local_s1",
                "cliSessionId": "t9",
                "priorCliSessionIds": ["t1"],
                "priorCliSessionLeftAt": {"t1": 1794760000000},
                "title": "Work by-a",
                "cwd": "/work/beryl",
            }
        )
    )
    elsewhere = tmp_path / "sessions" / "a" / "b" / "local_s2.json"
    elsewhere.write_text(
        json.dumps({"sessionId": "local_s2", "cliSessionId": "t8", "cwd": "/other"})
    )
    beads = listing(
        bead_record("by-a", title="A"),
        bead_record("by-q", metadata={"enact_spawn_host": "cygnus"}),
    )
    db = open_db(tmp_path)
    ingest.ingest(db, sources(tmp_path, beads), HOST)
    names = db.execute("SELECT transcript, session FROM sessions ORDER BY transcript")
    assert [tuple(r) for r in names] == [("t1", "local_s1"), ("t9", "local_s1")]

    found = bead.report(db, "by-a")
    assert found.data["requests"] == 4
    assert found.data["tokens"] == {
        "input_tokens": 4,
        "cache_write_tokens": 40,
        "cache_read_tokens": 400,
        "output_tokens": 20,
    }
    assert found.data["wall_minutes"] == 30.1
    sessions = found.data["sessions"]
    assert isinstance(sessions, list) and sessions[0]["session"] == "local_s1"
    assert sessions[0]["title"] == "Work by-a"

    # A ticket that ran where the store has no data says so, never zero.
    other = bead.report(db, "by-q")
    assert other.data["no_data"] and other.data["requests"] is None
    assert "no data" in other.lines[1]
    assert cli.gaps(db)["beads_no_data"] == {"cygnus": 1}


def test_coverage_counts_transcripts_not_yet_read(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    lane(tmp_path)
    monkeypatch.setattr(cli, "sources", lambda: sources(tmp_path))
    assert cli.main(["coverage", "--json", "--no-ingest"]) == 0
    before = json.loads(capsys.readouterr().out)
    assert before["unread"]["never_read"] == 1 and before["unread"]["bytes"] > 0
    assert cli.main(["ingest"]) == 0
    capsys.readouterr()
    assert cli.main(["coverage", "--json", "--no-ingest"]) == 0
    after = json.loads(capsys.readouterr().out)
    assert after["unread"] == {"never_read": 0, "partly_read": 0, "bytes": 0}
    assert after["sessions_without_window"] == {}


def test_ignite_start_opens_a_window_and_its_other_verbs_do_not() -> None:
    assert claude.enact_commands("ignite start Tq44s.9.48 --reservation 7f3a") == [
        ("open", "Tq44s.9.48")
    ]
    assert claude.enact_commands('ignite start "Fix the badge"') == []
    assert claude.enact_commands("ignite land -m done") == []
    assert claude.enact_commands("grep 'ignite start Tq44s' notes.txt") == []
