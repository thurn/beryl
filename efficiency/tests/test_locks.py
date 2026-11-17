"""Enact's ownership lock: the lock-wait log read into the store, joined to the tool calls
that ran each command, split out of their time and reported, over small synthetic sources.
"""

from __future__ import annotations

import json
import sqlite3
from datetime import datetime
from pathlib import Path

import pytest

from beryl_efficiency import claude, cli, ingest, locks, paths, rollups, store
from beryl_efficiency.beads import Beads
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record
from tests.test_page import NUMBER, checked, get, numbers_in

HOST = "jarvis"
SCRIPT = "/home/a/enact/scripts/enact.py"
ENACT = f"python3 {SCRIPT}"


def at(hour: int, minute: int, second: int = 0, day: int = 15) -> str:
    return f"2026-11-{day:02d}T{hour:02d}:{minute:02d}:{second:02d}.000Z"


def epoch(moment: str) -> float:
    return datetime.fromisoformat(moment.replace("Z", "+00:00")).timestamp()


def lock_line(
    began: str,
    command: str,
    waited: float,
    held: float | None,
    synced: float | None = 0.0,
    holder: str | None = None,
    pid: int = 100,
) -> str:
    """A line as Enact's lock-wait log has one; `held` None is a wait that gave up."""
    return json.dumps(
        {
            "time": began[:19] + "Z",
            "pid": pid,
            "command": command,
            "outcome": "gave_up" if held is None else "acquired",
            "waited_s": waited,
            "held_s": held,
            "sync_s": None if held is None else synced,
            "syncs": None if held is None else 1,
            "holder": holder,
        }
    )


BEADS: bytes = listing(
    bead_record(
        "by-c.1",
        status="closed",
        started_at="2026-11-15T10:00:00Z",
        closed_at="2026-11-15T10:30:00Z",
        metadata={"enact_spawn_host": HOST},
    ),
)


@pytest.fixture
def root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A ticket whose session runs `enact meta` through a variable for 62 seconds, which took
    the lock twice: once after waiting 30 seconds while a vizier's `enact next` held it,
    syncing for 12 of its 20 seconds, then at once, syncing for all of its 6. Another
    session runs `enact meta` on another ticket over the same stretch, and starts later.
    """
    projects = tmp_path / "projects" / PROJECT
    (
        Transcript(projects / "t1.jsonl")
        .bash(at(10, 0, 1), "m1", "c1", f"{ENACT} start by-c.1")
        .result(at(10, 0, 2), "c1", '{"bead": "by-c.1"}')
        .bash(at(10, 0, 58), "m2", "c2", f'E="{ENACT}"; $E meta by-c.1 key=value')
        .result(at(10, 2), "c2")
        .bash(at(10, 20), "m3", "c3", f"{ENACT} close by-c.1 --reason done")
        .result(at(10, 20), "c3")
        .save()
    )
    (
        Transcript(projects / "t2.jsonl")
        .bash(at(10, 1), "n1", "d1", f'E() {{ {ENACT} "$@"; }}; E meta by-d.1 k=v')
        .result(at(10, 2), "d1")
        .save()
    )
    # A long command whose stored text is cut before its second Enact call.
    chained = f"{ENACT} sync; " + "true; " * 60 + f"{ENACT} close by-e.9 --reason x"
    (
        Transcript(projects / "t3.jsonl")
        .bash(at(10, 10), "o1", "e1", chained)
        .result(at(10, 11), "e1")
        .save()
    )
    state = tmp_path / "enact"
    state.mkdir()
    (state / "lock-waits.1.jsonl").write_text(
        # Read from the rotated file too. A command that is not Enact's is not read.
        lock_line(at(9, 0), "python3 -m unittest tests.test_unit", 0.0, 1.0)
        + "\n"
    )
    (state / "lock-waits.jsonl").write_text(
        "\n".join(
            [
                lock_line(
                    at(10, 1),
                    f"{SCRIPT} meta by-c.1 key=value",
                    30.0,
                    20.0,
                    12.0,
                    f"{SCRIPT} next --epic by-e",
                ),
                lock_line(
                    at(10, 1, 50), f"{SCRIPT} meta by-c.1 key=value", 0.0, 6.0, 6.0
                ),
                # The vizier's loop, run outside any session.
                lock_line(
                    at(10, 0, 40), f"{SCRIPT} next --epic by-e", 0.0, 50.0, pid=7
                ),
                # A wait that gave up, with no call around it yet.
                lock_line(
                    at(10, 30),
                    f"{SCRIPT} meta by-z.1",
                    180.0,
                    None,
                    holder=f"{SCRIPT} push --bead by-q.1",
                    pid=8,
                ),
                lock_line(
                    at(10, 10, 10), f"{SCRIPT} close by-e.9 --reason x", 5.0, 7.0
                ),
                # A line far older than the store's latest call: no call is looked for again.
                lock_line(at(10, 0, day=13), f"{SCRIPT} close by-y.1", 0.0, 2.0, pid=9),
            ]
        )
        + "\n"
    )
    sources: list[ingest.Source] = [
        Beads(lambda: BEADS),
        claude.Sessions(tmp_path / "sessions", CHECKOUT, HOST),
        claude.Transcripts(tmp_path / "projects", CHECKOUT, HOST),
        locks.LockLog(state, HOST),
    ]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    # The range's length, which scales every finding to a week, runs to now. Pinned, the
    # fixture's two-hour spread stays the same size however long ago it is dated.
    monkeypatch.setattr(
        rollups,
        "now",
        lambda: datetime.fromisoformat(at(12, 0).replace("Z", "+00:00")),
    )
    assert cli.main(["ingest"]) == 0
    assert cli.main(["ingest"]) == 0  # A second ingest changes nothing.
    return tmp_path


def query(capsys: pytest.CaptureFixture[str], *argv: str) -> dict[str, object]:
    capsys.readouterr()
    assert cli.main([*argv, "--since", "all", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert isinstance(found, dict)
    return found


def db() -> sqlite3.Connection:
    return store.open_read_only(paths.store())


def test_each_line_joins_the_call_that_ran_its_command(root: Path) -> None:
    with db() as found:
        joined = found.execute(
            "SELECT subcommand, bead, transcript, call FROM lock_waits ORDER BY time"
        ).fetchall()
    # Both of `meta by-c.1`'s lines join the call that names its ticket, although the other
    # session's `meta` started later, and `close by-e.9` joins the long command whose stored
    # text was cut before it; the others join nothing.
    assert joined == [
        ("close", "by-y.1", None, ""),
        ("next", "by-e", None, None),
        ("meta", "by-c.1", "t1", "c2"),
        ("meta", "by-c.1", "t1", "c2"),
        ("close", "by-e.9", "t3", "e1"),
        ("meta", "by-z.1", None, None),
    ]


def test_staging_wait_is_charged_once_to_the_integrating_ticket(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    projects = root / "projects" / PROJECT
    for ticket, transcript, start in (
        ("by-c.1", "integrator", 0),
        ("by-c.10", "other-push", 5),
    ):
        (
            Transcript(projects / f"{transcript}.jsonl")
            .bash(at(11, 0, start), "m1", "start", f"{ENACT} start {ticket}")
            .result(at(11, 0, start + 1), "start", json.dumps({"bead": ticket}))
            .bash(at(11, 0, start + 2), "m2", "push", f"{ENACT} push --bead {ticket}")
            .result(at(11, 1), "push")
            .save()
        )
    state = root / "enact"
    record = json.dumps(
        {
            "time": "2026-11-15T11:00:10.500+00:00",
            "pid": 401,
            "bead": "c.1",
            "outcome": "acquired",
            "waited_s": 36.0,
        }
    )
    (state / "staging-lease-waits.jsonl").write_text(record + "\n")
    assert cli.main(["ingest"]) == 0
    # Re-read the same record under a different path, as log rotation can cause.
    (state / "staging-lease-waits.1.jsonl").write_text(record + "\n")
    assert cli.main(["ingest"]) == 0
    with db() as found:
        joined = found.execute(
            "SELECT transcript, call, waited_seconds FROM lock_waits WHERE staging_lease = 1"
        ).fetchall()
        waits = found.execute(
            "SELECT w.bead, t.seconds FROM window_time AS t "
            "JOIN windows AS w USING (host, provider, transcript, start) "
            "WHERE t.kind = 'enact lease wait'"
        ).fetchall()
    assert joined == [("integrator", "push", 36.0)]
    assert waits == [("by-c.1", 36.0)]
    breakdown = query(capsys, "time", "--by", "kind")
    assert "enact lease wait" in json.dumps(breakdown)
    assert query(capsys, "enact")["lines"] == 6
    assert "enact lease wait" in checked(get("/bead/by-c.1?since=all").body.decode())


def test_lock_waits_and_syncs_move_out_of_the_call_s_kind(root: Path) -> None:
    with db() as found:
        rows = found.execute(
            "SELECT w.start, w.end, t.category, t.kind, t.seconds FROM window_time AS t "
            "JOIN windows AS w USING (host, provider, transcript, start) "
            "WHERE w.bead = 'by-c.1'"
        ).fetchall()
    split = {(c, k): s for _, _, c, k, s in rows}
    assert split[("tool", "enact lock wait")] == pytest.approx(30.0)
    assert split[("tool", "enact sync")] == pytest.approx(18.0)
    # The rest of `meta`'s 62 seconds, and the second `enact start` took.
    assert split[("tool", "enact")] == pytest.approx(15.0)
    start, end = rows[0][0], rows[0][1]
    assert sum(split.values()) == pytest.approx(epoch(end) - epoch(start))


def test_the_time_split_names_the_new_kinds(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    found = query(capsys, "time", "--by", "kind")
    rows = {r["name"]: r["agent_minutes"] for r in found["rows"]}  # type: ignore[union-attr]
    # 30 seconds in `meta by-c.1` and 5 in the long command.
    assert rows["enact lock wait"] == 0.6 and rows["enact sync"] == 0.3


def test_the_enact_query(root: Path, capsys: pytest.CaptureFixture[str]) -> None:
    found = query(capsys, "enact")
    assert (found["lock_wait_minutes"], found["sync_minutes"]) == (0.6, 0.3)
    assert (found["lines"], found["joined"], found["unjoined"], found["pending"]) == (
        6,
        3,
        1,
        2,
    )
    assert found["subcommands"] == [
        {
            "subcommand": "enact meta",
            "lines": 3,
            "acquisitions": 2,
            "gave_up": 1,
            "wait_minutes": 3.5,
            "sync_minutes": 0.3,
            "held_minutes": 0.4,
            "calls": 1,
            "call_minutes": 1.0,
        },
        {
            "subcommand": "enact close",
            "lines": 2,
            "acquisitions": 2,
            "gave_up": 0,
            "wait_minutes": 0.1,
            "sync_minutes": 0.0,
            "held_minutes": 0.1,
            "calls": 1,
            "call_minutes": 1.0,
        },
        {
            "subcommand": "enact next",
            "lines": 1,
            "acquisitions": 1,
            "gave_up": 0,
            "wait_minutes": 0.0,
            "sync_minutes": 0.0,
            "held_minutes": 0.8,
            "calls": 0,
            "call_minutes": 0.0,
        },
    ]
    assert found["holders"] == [
        {"holder": "enact push", "waits": 1, "wait_minutes": 3.0},
        {"holder": "enact next", "waits": 1, "wait_minutes": 0.5},
        {"holder": "(not seen)", "waits": 1, "wait_minutes": 0.1},
    ]
    # A cohort, here the sessions of one provider, keeps only the lines joined to its calls.
    cohort = query(capsys, "enact", "--provider", "claude")
    assert (cohort["lines"], cohort["joined"]) == (3, 3)
    assert cli.main(["enact", "--since", "all", "--no-ingest"]) == 0
    text = capsys.readouterr().out
    assert "lock wait 0.6 | sync 0.3" in text and "held the lock" in text


def test_the_findings_rule_names_the_holder(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    found = query(capsys, "findings", "--rule", "enact-lock-wait")
    text = json.dumps(found)
    assert "enact next" in text and "enact push" not in text


def test_coverage_counts_the_lines_joined_and_not(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    capsys.readouterr()
    assert cli.main(["coverage", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert found["lock_lines"] == {"joined": 3, "no call": 1, "pending": 2}


def test_every_number_on_the_enact_page_is_a_number_of_its_json(root: Path) -> None:
    have = numbers_in(json.loads(get("/enact.json?since=all").body))
    shown = checked(get("/enact?since=all").body.decode())
    assert "enact push" in shown
    for token in NUMBER.findall(shown):
        assert any(abs(float(token) - n) < 1e-9 for n in have), token


@pytest.mark.parametrize(
    "command, sub, expected",
    [
        (f"{ENACT} meta by-c.1", "meta", True),
        (f'cd /work && E="{ENACT}"; "$E" start by-c.1', "start", True),
        (f'E() {{ {ENACT} "$@"; }}; E close by-c.1 --reason x', "close", True),
        (f"{ENACT} meta by-c.1", "close", False),
        ("cat /home/a/enact/notes meta", "meta", False),
    ],
)
def test_a_command_runs_a_subcommand_by_path_variable_or_function(
    command: str, sub: str, expected: bool
) -> None:
    assert locks.runs(command, sub) is expected
