"""CPU accounting: the sampler's rows and the token pool's leases, read into the store and
reported per bead and for the machine, over small synthetic sources."""

from __future__ import annotations

import json
import sqlite3
from datetime import datetime
from pathlib import Path

import pytest

from beryl_efficiency import claude, cli, cpu, facts, ingest, machine, paths, store
from beryl_efficiency.beads import Beads
from beryl_efficiency.scope import Scope
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record

HOST = "jarvis"
ENACT = "python3 ~/enact/scripts/enact.py"
CORES = 4


def at(hour: int, minute: int, second: int = 0) -> str:
    return f"2026-11-15T{hour:02d}:{minute:02d}:{second:02d}.000Z"


def epoch(moment: str) -> float:
    return datetime.fromisoformat(moment.replace("Z", "+00:00")).timestamp()


BEADS: bytes = listing(
    bead_record(
        "by-c.1",
        status="closed",
        started_at="2026-11-15T10:00:00Z",
        closed_at="2026-11-15T10:30:00Z",
        metadata={
            "enact_worktree": "/work/beryl/.worktrees/by-c.1",
            "enact_spawn_host": HOST,
        },
    ),
)


def minute(moment: str, used: dict[str, float]) -> str:
    """A sampler row as `by cpu sample` writes one."""
    return json.dumps(
        {
            "time": epoch(moment),
            "seconds": 60.0,
            "cores": CORES,
            "cpu": used,
            "unnamed": {},
            "peak_cores": 4.0,
            "top_cores": 3.0,
            "load": 5.0,
        }
    )


def lease(released: str, worktree: str, waited: float, held: float) -> str:
    """A lease line as the token pool writes one."""
    return json.dumps(
        {
            "time": released[:19] + "Z",
            "class": "agent",
            "worktree": worktree,
            "command": "test-rust",
            "cores": 6,
            "wanted": 12,
            "waited_s": waited,
            "held_s": held,
        }
    )


@pytest.fixture
def root(tmp_path: Path, cache: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """One bead in its own worktree: a session runs `just test-rust` for two minutes, of
    which its lease waited 30 seconds, and the sampler saw the worktree's CPU before the
    bead started, inside its window, after its window but before its close, and after it
    closed."""
    (
        Transcript(tmp_path / "projects" / PROJECT / "t1.jsonl")
        .bash(at(10, 0, 1), "m1", "c1", f"{ENACT} start by-c.1")
        .result(at(10, 0, 2), "c1", '{"bead": "by-c.1"}')
        .bash(at(10, 5), "m2", "c2", "just test-rust")
        .result(at(10, 7), "c2")
        .bash(at(10, 20), "m3", "c3", f"{ENACT} close by-c.1 --reason done")
        .result(at(10, 20), "c3")
        .save()
    )
    samples = cpu.samples_path(cache)
    samples.parent.mkdir(parents=True)
    samples.write_text(
        "\n".join(
            [
                minute(at(9, 58), {"system/system/-": 100, "session/tool/by-c.1": 50}),
                minute(
                    at(10, 5),
                    {
                        "system/system/-": 100,
                        "session/compile/by-c.1": 120,
                        "session/tool/by-c.1": 3,
                    },
                ),
                # Exactly 90% of the cores: saturated.
                minute(at(10, 10), {"system/system/-": 216}),
                minute(at(10, 25), {"system/system/-": 100, "session/tool/by-c.1": 57}),
                minute(at(10, 40), {"system/system/-": 100, "session/tool/by-c.1": 44}),
            ]
        )
        + "\n"
    )
    leases = cpu.leases_path(cache)
    leases.parent.mkdir(parents=True)
    leases.write_text(
        "\n".join(
            [
                # Waited from 10:05:00 to 10:05:30, inside the test run.
                lease(at(10, 6, 50), "by-c.1", 30.0, 80.0),
                lease(at(10, 6, 50), "elsewhere", 10.0, 80.0),
                lease(at(10, 8), "by-c.1", 0.0, 5.0),
            ]
        )
        + "\n"
    )
    sources: list[ingest.Source] = [
        Beads(lambda: BEADS),
        claude.Sessions(tmp_path / "sessions", CHECKOUT, HOST),
        claude.Transcripts(tmp_path / "projects", CHECKOUT, HOST),
        cpu.Samples(samples, HOST),
        cpu.Leases(leases, HOST),
    ]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    monkeypatch.setattr(cpu, "sampler_cpu", lambda: (36.0, 3600.0))
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


def test_core_minutes_are_the_worktree_s_cpu_while_the_bead_was_open(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    # 123 s inside the window, and 57 s after it but before the close.
    assert query(capsys, "bead", "by-c.1")["core_minutes"] == 3.0
    summary = query(capsys, "summary")
    figure = summary["core_minutes"]
    assert isinstance(figure, dict)
    trend = figure.pop("trend")
    figure.pop("marks")  # From the project's own changes file; test_page covers them.
    assert figure == {
        "median": 3.0,
        "p90": 3.0,
        "count": 1,
        "before": None,
        "lower_bound": False,
    }
    assert isinstance(trend, list) and [p["median"] for p in trend] == [3.0]


def test_queued_seconds_move_out_of_the_tool_s_kind(root: Path) -> None:
    with db() as found:
        joined = found.execute(
            "SELECT worktree, call FROM leases ORDER BY worktree, waited_seconds"
        ).fetchall()
        rows = found.execute(
            "SELECT w.start, w.end, t.category, t.kind, t.seconds FROM window_time AS t "
            "JOIN windows AS w USING (host, provider, transcript, start) "
            "WHERE w.bead = 'by-c.1'"
        ).fetchall()
    # The lease that waited joins the test run; one that waited not at all joins nothing,
    # and one in a worktree no session worked in is tried again later.
    assert joined == [("by-c.1", ""), ("by-c.1", "c2"), ("elsewhere", None)]
    split = {(c, k): s for _, _, c, k, s in rows}
    assert split[("queued", "rust tests")] == pytest.approx(30.0)
    assert split[("tool", "rust tests")] == pytest.approx(90.0)
    start, end = rows[0][0], rows[0][1]
    assert sum(split.values()) == pytest.approx(epoch(end) - epoch(start))


def test_the_machine_query(root: Path, capsys: pytest.CaptureFixture[str]) -> None:
    found = query(capsys, "machine")
    assert found["cores"] == CORES
    assert found["minutes"] == 5
    # The minute at 10:05 (3.7 cores) and the one at exactly 90% (3.6 cores); the others
    # used under 3 cores.
    assert found["saturation"] == 40.0 and found["over_cores"] == 0
    assert found["queued"] == [
        {
            "klass": "agent",
            "command": "test-rust",
            "leases": 3,
            "waited_hours": 0.01,
            "core_hours": 0.13,
        }
    ]
    assert found["narrow"] == [
        {
            "klass": "agent",
            "command": "test-rust",
            "leases": 3,
            "narrower": 3,
            "mean_cores": 6.0,
            "mean_wanted": 12.0,
        }
    ]
    # The bead claims 180 s; its worktree's 94 s before it started and after it closed are
    # shared, beside the system's 616 s.
    assert found["bead_core_hours"] == 0.05
    shared = {p["name"]: p["value"] for p in found["shared"]}  # type: ignore[union-attr]
    assert shared == {"system/system": 0.17, "worktree by-c.1": 0.03}
    own = found["own"]
    assert isinstance(own, dict) and own["sampler_cores"] == 0.01
    capacity = found["capacity"]
    assert isinstance(capacity, dict)
    assert capacity["beads"] == 1 and capacity["machine_cores"] == CORES
    assert cli.main(["machine", "--since", "all", "--no-ingest"]) == 0
    text = capsys.readouterr().out
    assert "saturation 40.0% of 5 minutes" in text
    assert text.rstrip().splitlines()[-1].startswith("more: ")


def test_the_machine_metrics(root: Path, capsys: pytest.CaptureFixture[str]) -> None:
    saturation = query(capsys, "metrics", "machine.saturation")
    assert saturation["median"] == 40.0
    queued = query(capsys, "metrics", "machine.queued-core-hours-per-day")
    assert queued["count"] == 1 and queued["median"] == pytest.approx(0.13)
    assert query(capsys, "metrics", "bead.core-minutes")["median"] == 3.0


def test_the_sampler_s_cpu_counts_every_process_under_its_agent() -> None:
    table = "\n".join(
        [
            "  10     1   0:00.03 01:00:00",
            "  11    10   0:18.00 01:00:00",
            "  12    11   0:01.50    00:02",
            "  13     1   9:00.00 01:00:00",
        ]
    )
    assert cpu.tree_cpu(table, "10") == (19.53, 3600.0)
    assert cpu.tree_cpu(table, "99") is None
    assert cpu.clock_seconds("2-01:02:03") == 2 * 86400 + 3723


def test_a_minute_sampled_over_the_cores_counts_as_all_busy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(cpu, "sampler_cpu", lambda: None)
    found = store.open_store(tmp_path / "store.sqlite")
    with found:
        found.executemany(
            "INSERT INTO cpu_minutes (host, time, seconds, cores, cpu_seconds) "
            "VALUES (?, ?, 60, ?, ?)",
            [(HOST, at(10, 0), CORES, 60.0), (HOST, at(10, 1), CORES, 6000.0)],
        )
    data = machine.report(found, facts.load(found), Scope()).data
    assert data["over_cores"] == 1
    assert data["peak_cores"] == CORES
    assert data["cores_in_use"] == 2.5
    assert data["saturation"] == 50.0
