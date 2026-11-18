"""Time accounting: every second of a window in exactly one category, and command kinds."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import claude, ingest, kinds, store, timing
from tests.synthetic import CHECKOUT, PROJECT, Transcript

HOST = "jarvis"
FINISHED = "    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.00s"


def at(seconds: int) -> str:
    """A time `seconds` after 10:00 on the test day."""
    return f"2026-11-15T10:{seconds // 60:02d}:{seconds % 60:02d}.000Z"


def transcript(root: Path, name: str = "t1") -> Transcript:
    return Transcript(root / "projects" / PROJECT / f"{name}.jsonl")


def ingested(root: Path) -> sqlite3.Connection:
    db = store.open_store(root / "store.sqlite")
    sources: list[ingest.Source] = [
        claude.Sessions(root / "sessions", CHECKOUT, HOST),
        claude.Transcripts(root / "projects", CHECKOUT, HOST),
    ]
    ingest.ingest(db, sources, HOST)
    return db


def split_of(
    db: sqlite3.Connection, name: str = "t1"
) -> dict[tuple[str, str, str], float]:
    """The one window of transcript `name`, split, with seconds rounded to hundredths."""
    rows = db.execute(
        "SELECT category, kind, part, seconds FROM window_time WHERE transcript = ?",
        (name,),
    ).fetchall()
    return {(str(r[0]), str(r[1]), str(r[2])): round(float(r[3]), 2) for r in rows}


def window_lengths(db: sqlite3.Connection) -> list[tuple[float, float]]:
    """Each window's length and the sum of its split."""
    found = []
    for transcript, start, end in db.execute(
        "SELECT transcript, start, end FROM windows"
    ).fetchall():
        total = db.execute(
            "SELECT coalesce(sum(seconds), 0) FROM window_time WHERE transcript = ? "
            "AND start = ?",
            (transcript, start),
        ).fetchone()[0]
        length = timing.seconds(str(end)) - timing.seconds(str(start))
        found.append((round(length, 6), round(float(total), 6)))
    return found


def three_calls(root: Path) -> None:
    """Three shell commands started at once and ending at 10, 20 and 30 seconds, the model
    ending its turn at 40, a person answering at 100 and the model ending at 110."""
    (
        transcript(root)
        .request(
            at(0),
            "m1",
            tools=(
                ("a", "Bash", {"command": "ls"}),
                ("b", "Bash", {"command": "git status"}),
                (
                    "c",
                    "Bash",
                    {"command": "cd /x/.worktrees/enact-thing && cargo test"},
                ),
            ),
        )
        .result(at(10), "a")
        .result(at(20), "b")
        .result(at(30), "c", stderr=FINISHED)
        .request(at(40), "m2")
        .prompt(at(100))
        .request(at(110), "m3")
        .save()
    )


def test_overlapping_calls_share_their_seconds_and_the_split_sums(
    tmp_path: Path,
) -> None:
    three_calls(tmp_path)
    db = ingested(tmp_path)
    # 0-10 s is shared by three calls, 10-20 s by two and 20-30 s is the last one's alone.
    # The cargo command spent 6 of its 30 seconds compiling, so 6/30 of its share.
    assert split_of(db) == {
        ("tool", "read and search", ""): 3.33,
        ("tool", "git", ""): 8.33,
        ("tool", "rust tests", "compile"): 3.67,
        ("tool", "rust tests", "run"): 14.67,
        ("model", "", ""): 20.0,
        ("person", "", ""): 60.0,
    }
    assert window_lengths(db) == [(110.0, 110.0)]


def test_a_light_command_reading_a_build_log_compiles_nothing(tmp_path: Path) -> None:
    (
        transcript(tmp_path)
        .bash(at(0), "m1", "t", "tail -5 build.log")
        .result(at(10), "t", stdout=FINISHED)
        .request(at(11), "m2")
        .save()
    )
    db = ingested(tmp_path)
    assert split_of(db) == {
        ("tool", "read and search", ""): 10.0,
        ("model", "", ""): 1.0,
    }


def test_a_question_to_the_user_is_waiting_on_a_person(tmp_path: Path) -> None:
    (
        transcript(tmp_path)
        .request(at(0), "m1", tools=(("q", "AskUserQuestion", {"questions": []}),))
        .result(at(50), "q")
        .request(at(55), "m2")
        .save()
    )
    db = ingested(tmp_path)
    assert split_of(db) == {("person", "question", ""): 50.0, ("model", "", ""): 5.0}


def test_after_a_turn_ends_a_wake_up_is_idle_and_a_notification_is_background(
    tmp_path: Path,
) -> None:
    (
        transcript(tmp_path)
        .request(
            at(0),
            "m1",
            tools=(
                ("bg", "Bash", {"command": "cargo build", "run_in_background": True}),
            ),
        )
        .result(at(1), "bg", stdout="started in the background")
        .request(at(2), "m2")
        .prompt(
            at(62),
            "<task-notification><tool-use-id>bg</tool-use-id></task-notification>",
            {"kind": "task-notification", "producer": "session-task"},
        )
        .request(at(64), "m3")
        .prompt(
            at(124),
            "<scheduled-task>check</scheduled-task>",
            {"kind": "task-notification", "subkind": "scheduled-trigger"},
        )
        .request(at(126), "m4")
        .prompt(at(136), "Another Claude session sent a message", {"kind": "peer"})
        .request(at(137), "m5")
        .save()
    )
    db = ingested(tmp_path)
    found = split_of(db)
    assert found == {
        ("tool", "build", ""): 1.0,
        ("model", "", ""): 6.0,
        ("background", "build", ""): 60.0,
        ("idle", "", ""): 60.0,
        ("background", "session", ""): 10.0,
    }
    agent = sum(v for k, v in found.items() if k[0] in timing.AGENT_CATEGORIES)
    assert agent == 77.0  # The idle minute is not agent time.


def test_a_subagent_adds_nothing_to_its_parents_window(tmp_path: Path) -> None:
    three_calls(tmp_path)
    sub = tmp_path / "projects" / PROJECT / "t1" / "subagents" / "agent-x.jsonl"
    (
        Transcript(sub)
        .request(at(1), "s1", tools=(("s", "Bash", {"command": "cargo build"}),))
        .result(at(25), "s")
        .request(at(26), "s2")
        .save()
    )
    db = ingested(tmp_path)
    assert window_lengths(db) == [(110.0, 110.0)]
    assert ("tool", "build", "") not in split_of(db)
    assert split_of(db, "t1/agent-x") == {}


def test_a_change_to_the_kinds_table_gives_every_call_its_kind_again(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    three_calls(tmp_path)
    db = ingested(tmp_path)
    table = tmp_path / "kinds.json"
    table.write_text(json.dumps([{"kind": "listing", "pattern": "^ls\\b"}]))
    monkeypatch.setattr(kinds, "table_path", lambda: table)
    ingest.ingest(db, [claude.Transcripts(tmp_path / "projects", CHECKOUT, HOST)], HOST)
    assert split_of(db)[("tool", "listing", "")] == 3.33
    assert ("tool", "other", "") in split_of(db)


@pytest.mark.parametrize(
    ("command", "kind", "chained"),
    [
        (
            "cd /x/.worktrees/enact-thing && cargo test -p beryl_rules",
            "rust tests",
            False,
        ),
        ("cd /x/.worktrees/enact-thing && ls", "read and search", False),
        ("ls ~/enact/.worktrees", "read and search", False),
        ("python3 ~/enact/scripts/enact.py push --bead by-a", "push", False),
        ("E='python3 enact.py'; eval $E push --bead by-a 2>&1 | tail", "push", False),
        (
            "timeout 900 python3 ~/enact/scripts/enact.py ci --wait abc",
            "slow CI wait",
            False,
        ),
        ("FOO=1 just test-rust 2>&1 | tail -5", "rust tests", False),
        ("just check && enact push", "fast check", True),
        ("git status && cargo build", "build", False),
        ("git log --oneline -3", "git", False),
        ("ignite land -m 'done'", "push", False),
        ("ignite start Tq44s.9.48", "ignite start", False),
        ("ignite test badges_manual", "ignite test", False),
        ("ignite check", "fast check", False),
        ("ignite wait L-5521", "ignite wait", False),
        ("ignite ticket wait Tig9a.1 --status closed", "ignite wait", False),
        ("ignite sync", "ignite", False),
        ('ignite ticket create "x" -p P2', "ignite", False),
        ("frobnicate --all", kinds.OTHER, False),
    ],
)
def test_a_command_is_classified_by_what_follows_its_prefix(
    command: str, kind: str, chained: bool
) -> None:
    table, _ = kinds.load()
    assert kinds.classify(table, "Bash", command) == (kind, chained)


def test_other_tools_are_their_own_kinds() -> None:
    table, _ = kinds.load()
    assert kinds.classify(table, "Read", "/work/a.rs") == ("Read", False)


def test_cargo_finished_lines_give_compile_seconds() -> None:
    output = {
        "stdout": "    Finished `dev` profile [unoptimized] target(s) in 1m 02s\nok",
        "stderr": "    Finished `test` profile [unoptimized] target(s) in 3.50s",
    }
    assert claude.compile_seconds(output) == 65.5
    assert claude.compile_seconds({"stdout": "test result: ok"}) is None


def test_the_kinds_table_is_valid() -> None:
    table, digest = kinds.load()
    assert table and len(digest) == 16
    assert len({k.kind for k in table}) == len(table)
    twice = json.dumps([{"kind": "a", "pattern": "x"}, {"kind": "a", "pattern": "y"}])
    with pytest.raises(ValueError, match="twice"):
        kinds.parse(twice.encode())
    with pytest.raises(Exception):
        kinds.parse(json.dumps([{"kind": "a", "pattern": "("}]).encode())
