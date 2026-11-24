"""Ignite's hub as a source: a small synthetic hub database whose records stand in for the run
log, the CPU token pool's leases and the CPU sampler, whose stored exports bring a second
machine's rows, and whose tickets, tips and hand-offs feed the Ignite series."""

from __future__ import annotations

import gzip
import json
import sqlite3
from contextlib import closing
from pathlib import Path

import pytest

from beryl_efficiency import claude, cli, exchange, fleet, ignite, ingest, paths, store
from tests.hub_server import TOKEN, FleetHub
from tests.hub_server import record as fleet_record
from tests.legacy_sources import Beads
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record

HOST = "jarvis"
CORES = 8

pytestmark: pytest.MarkDecorator = pytest.mark.usefixtures("utc_days")

# The hub's tables this source reads, with the columns it uses, as the hub's schema declares
# them (ignite/crates/ignite_store/src/hub/schema.sql and schema_metrics.sql).
HUB_SCHEMA = """
CREATE TABLE machines (id TEXT PRIMARY KEY, name TEXT NOT NULL, os TEXT NOT NULL,
  cores INTEGER NOT NULL, memory_mb INTEGER NOT NULL, families TEXT NOT NULL DEFAULT '[]',
  version TEXT NOT NULL, last_seen TEXT NOT NULL);
CREATE TABLE workers (machine TEXT NOT NULL, session TEXT NOT NULL, app TEXT NOT NULL,
  role TEXT NOT NULL, state TEXT NOT NULL, ticket TEXT, files TEXT NOT NULL DEFAULT '[]',
  PRIMARY KEY (machine, session));
CREATE TABLE tickets (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, claimed_at TEXT,
  landed_at TEXT);
CREATE TABLE handoffs (seq INTEGER PRIMARY KEY, machine TEXT NOT NULL, ticket TEXT NOT NULL,
  kind TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE regressions (check_name TEXT PRIMARY KEY, ticket TEXT NOT NULL,
  first_bad TEXT NOT NULL, last_good TEXT NOT NULL, state TEXT NOT NULL,
  culprit_ticket TEXT);
CREATE TABLE tips (commit_sha TEXT PRIMARY KEY, seen TEXT NOT NULL, verified TEXT,
  state TEXT NOT NULL);
CREATE TABLE records (id INTEGER PRIMARY KEY, machine TEXT NOT NULL, ticket TEXT,
  session TEXT, tree TEXT, verb TEXT NOT NULL, class TEXT NOT NULL, family TEXT,
  queued_s REAL NOT NULL, wall_s REAL NOT NULL, cpu_s REAL NOT NULL, instructions REAL,
  peak_mb INTEGER NOT NULL, outcome TEXT, commit_sha TEXT, ignite TEXT NOT NULL,
  at TEXT NOT NULL);
CREATE TABLE metrics (machine TEXT NOT NULL, source TEXT NOT NULL, data TEXT NOT NULL,
  at TEXT NOT NULL, PRIMARY KEY (machine, source, data)) WITHOUT ROWID;
"""


def at(hour: int, minute: int, second: int = 0) -> str:
    return f"2026-11-15T{hour:02d}:{minute:02d}:{second:02d}.000Z"


def record(
    hub: sqlite3.Connection,
    end: str,
    verb: str,
    klass: str,
    family: str | None,
    wall: float,
    cpu: float,
    queued: float = 0.0,
    outcome: str | None = "pass",
    commit: str = "k1",
    instructions: float | None = None,
) -> None:
    hub.execute(
        "INSERT INTO records (machine, ticket, session, tree, verb, class, family, queued_s, "
        "wall_s, cpu_s, instructions, peak_mb, outcome, commit_sha, ignite, at) "
        "VALUES ('m-1', 'p.1', 'local_w', 'tree-07', ?, ?, ?, ?, ?, ?, ?, 100, ?, ?, 'i1', ?)",
        (verb, klass, family, queued, wall, cpu, instructions, outcome, commit, end),
    )


def make_hub(path: Path) -> sqlite3.Connection:
    hub = sqlite3.connect(path)
    hub.executescript(HUB_SCHEMA)
    hub.execute(
        "INSERT INTO machines VALUES ('m-1', 'Jarvis.local', 'macos', ?, 1, '[]', 'i1', ?)",
        (CORES, at(9, 0)),
    )
    # The gate of one landing, 10:10:30 to 10:12 and a repeat on a rebased commit, with a
    # family the cache answered; tip verification afterwards.
    record(hub, at(10, 12), "land", "land", "rust", 90, 100, 30, instructions=4e11)
    record(hub, at(10, 11), "land", "land", "names", 20, 10, instructions=1e11)
    record(hub, at(10, 14), "land", "land", "rust", 30, 40, commit="k2")
    record(hub, at(10, 12), "cached", "land", "format", 0, 0)
    record(hub, at(10, 40), "tip", "tip", "rust", 120, 480, outcome="fail")
    hub.executemany(
        "INSERT INTO tickets VALUES (?, ?, ?, ?)",
        [
            ("p.1", at(9, 0), at(10, 0), at(10, 30)),
            ("p.2", at(9, 30), at(10, 30), at(11, 30)),
        ],
    )
    hub.execute(
        "INSERT INTO regressions VALUES ('rust:x', 'p.2', 'k1', 'k0', 'open', 'p.1')",
    )
    hub.execute(
        "INSERT INTO handoffs VALUES (1, 'm-1', 'p.2', 'regression', ?)", (at(10, 50),)
    )
    hub.execute(
        "INSERT INTO tips VALUES ('k1', ?, ?, 'verified')", (at(10, 30), at(10, 45))
    )
    hub.commit()
    return hub


ENACT = "python3 ~/enact/scripts/enact.py"


@pytest.fixture
def hub(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> sqlite3.Connection:
    """One ticket lands with `ignite land` from 10:10 to 10:15, and the hub holds the
    records of that landing's gate and of the tip verification after it."""
    path = tmp_path / "hub.sqlite"
    monkeypatch.setenv("IGNITE_HUB_DB", str(path))
    found = make_hub(path)
    (
        Transcript(tmp_path / "projects" / PROJECT / "t1.jsonl")
        .bash(at(10, 0, 1), "m1", "c1", f"{ENACT} start p.1")
        .result(at(10, 0, 2), "c1", '{"bead": "p.1"}')
        .bash(at(10, 10), "m2", "c2", "ignite land -m 'done'")
        .result(at(10, 15), "c2", "landed")
        .bash(at(10, 29), "m3", "c3", f"{ENACT} close p.1 --reason done")
        .result(at(10, 29, 30), "c3")
        .save()
    )
    beads = listing(
        bead_record(
            "by-p.1",
            status="closed",
            started_at=at(10, 0),
            closed_at=at(10, 30),
            metadata={"enact_worktree": "/work/beryl/.worktrees/tree-07"},
        )
    )
    sources: list[ingest.Source] = [
        Beads(lambda: beads),
        claude.Transcripts(tmp_path / "projects", CHECKOUT, HOST),
        *ignite.sources(paths.cache_root(), HOST),
    ]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    assert cli.main(["ingest"]) == 0
    return found


def query(capsys: pytest.CaptureFixture[str], *argv: str) -> dict[str, object]:
    capsys.readouterr()
    assert cli.main([*argv, "--since", "all", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert isinstance(found, dict)
    return found


def test_the_ci_query_reads_tip_verification_and_the_gate(
    hub: sqlite3.Connection, capsys: pytest.CaptureFixture[str]
) -> None:
    tips = query(capsys, "ci")
    assert tips["script_runs"] == 1
    rows = {r["name"]: r for r in tips["rows"]}  # type: ignore[union-attr]
    assert list(rows) == ["rust"] and rows["rust"]["runs"] == 1
    gate = query(capsys, "ci", "--script", "check")
    names = {r["name"]: r["runs"] for r in gate["rows"]}  # type: ignore[union-attr]
    # The family the cache answered ran nothing.
    assert names == {"rust": 2, "names": 1}


def test_a_landing_is_split_by_its_gate(
    hub: sqlite3.Connection, capsys: pytest.CaptureFixture[str]
) -> None:
    (push,) = query(capsys, "pushes")["rows"]  # type: ignore[misc]
    assert push["seconds"] == 300.0
    # Covered by the gate's runs: 10:10:30 to 10:12 and 10:13:30 to 10:14.
    assert push["fast_check_seconds"] == 120.0
    assert (push["checks"], push["repeats"]) == (2, 1)


def test_the_machine_query_reads_the_records_minutes_and_waits(
    hub: sqlite3.Connection, capsys: pytest.CaptureFixture[str]
) -> None:
    found = query(capsys, "machine")
    assert found["cores"] == CORES
    minutes = found["minutes"]
    assert isinstance(minutes, int) and minutes > 0
    queued = {q["command"]: q["waited_hours"] for q in found["queued"]}  # type: ignore[union-attr]
    assert queued["land rust"] == round(30 / 3600, 2)
    assert queued["tip rust"] == 0.0


def test_records_are_read_once(hub: sqlite3.Connection) -> None:
    assert cli.main(["ingest"]) == 0
    db = store.open_read_only(paths.store())
    try:
        assert db.execute("SELECT count(*) FROM ignite_records").fetchone()[0] == 5
        runs = db.execute("SELECT sum(runs) FROM checks").fetchone()[0]
        assert runs == 4
        assert db.execute(
            "SELECT check_name, ticket, last_good, first_bad, culprit_ticket "
            "FROM ignite_regressions"
        ).fetchall() == [("rust:x", "p.2", "k0", "k1", "p.1")]
    finally:
        db.close()


def test_the_ignite_series(
    hub: sqlite3.Connection, capsys: pytest.CaptureFixture[str]
) -> None:
    def day(name: str) -> tuple[int, float]:
        found = query(capsys, "metrics", name)
        (only,) = found["series"]  # type: ignore[misc]
        return int(only["count"]), float(only["median"])

    assert day("ignite.cycle-minutes") == (2, 45.0)
    # 60 of 90 minutes and 60 of 120 minutes before a claim.
    assert day("ignite.worker-wait-percent") == (2, 58.3)
    assert day("ignite.repair-percent") == (1, 50.0)
    assert day("ignite.master-lag-minutes") == (1, 15.0)
    assert day("ignite.escapes-per-100-landings") == (1, 50.0)
    # One of the five gate and tip families was answered from the cache.
    assert day("ignite.cache-hit-percent") == (1, 20.0)
    # Two landings: commit k1 (rust and names) and the rebased k2.
    assert day("ignite.gate-instructions") == (2, 250.0)
    assert day("ignite.gate-seconds") == (2, 70.0)
    assert day("ignite.gate-wait-seconds") == (2, 15.0)


def export_of(tmp_path: Path, host: str, minute: int = 0) -> str:
    """An efficiency export from another machine, as JSON lines, of one request."""
    other = store.open_store(tmp_path / f"{host}.sqlite")
    other.execute(
        "INSERT INTO requests (host, provider, transcript, message, time) "
        "VALUES (?, 'claude', 't', ?, ?)",
        (host, f"m{minute}", at(9, minute)),
    )
    other.commit()
    out = tmp_path / f"{host}.jsonl"
    exchange.export(other, host, None, out)
    return out.read_text()


def post(
    hub: sqlite3.Connection, machine: str, source: str, text: str, when: str
) -> None:
    """Stores a posted export as the hub's `/metrics/import` route does: each line once
    per machine and source, with when it first arrived."""
    hub.executemany(
        "INSERT OR IGNORE INTO metrics (machine, source, data, at) VALUES (?, ?, ?, ?)",
        [(machine, source, line, when) for line in text.splitlines() if line.strip()],
    )
    hub.commit()


def test_a_posted_export_brings_a_second_machine_into_coverage(
    tmp_path: Path, hub: sqlite3.Connection, capsys: pytest.CaptureFixture[str]
) -> None:
    post(hub, "m-2", "multivac", export_of(tmp_path, "multivac"), at(11, 0))
    # The hub's own machine posts its rows too; ingest here already has them.
    post(hub, "m-1", "jarvis", export_of(tmp_path, "jarvis"), at(11, 0))
    assert cli.main(["ingest"]) == 0
    capsys.readouterr()
    assert cli.main(["coverage", "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    hosts = found["hosts"]
    assert isinstance(hosts, dict) and "multivac" in hosts
    db = store.open_read_only(paths.store())
    try:
        # The export's request is transcript `t`; this machine's own come from ingest.
        assert db.execute(
            "SELECT host FROM requests WHERE transcript = 't'"
        ).fetchall() == [("multivac",)]
    finally:
        db.close()


def test_a_renamed_machine_keeps_its_first_name(
    tmp_path: Path, hub: sqlite3.Connection
) -> None:
    # A later post, read from where the last ingest stopped, under the machine's new name.
    for minute, name in ((0, "multivac"), (5, "mbp")):
        post(hub, "m-2", name, export_of(tmp_path, name, minute // 5), at(11, minute))
        assert cli.main(["ingest"]) == 0
    db = store.open_read_only(paths.store())
    try:
        rows = db.execute(
            "SELECT host, time FROM requests WHERE transcript = 't' ORDER BY time"
        ).fetchall()
        assert rows == [("multivac", at(9, 0)), ("multivac", at(9, 1))]
    finally:
        db.close()


def test_a_plain_export_imports_as_a_compressed_one_does(tmp_path: Path) -> None:
    text = export_of(tmp_path, "cygnus")
    packed = tmp_path / "cygnus.gz"
    with gzip.open(packed, "wt", encoding="utf-8") as handle:
        handle.write(text)
    for path in (tmp_path / "cygnus.jsonl", packed):
        target = store.open_store(tmp_path / f"target-{path.suffix}.sqlite")
        assert exchange.import_file(target, path, HOST).added == {"requests": 1}


def test_hub_tickets_become_beads_labels_and_aliases(tmp_path: Path) -> None:
    hub = tmp_path / "hub.sqlite"
    db = sqlite3.connect(hub)
    db.executescript("""
        CREATE TABLE tickets (id TEXT PRIMARY KEY, parent TEXT, title TEXT, type TEXT,
          status TEXT, priority INTEGER, labels TEXT, metadata TEXT, created_at TEXT,
          claimed_at TEXT, closed_at TEXT, tree TEXT, session TEXT, landed TEXT);
        CREATE TABLE aliases (alias TEXT PRIMARY KEY, id TEXT NOT NULL);
        INSERT INTO tickets VALUES ('k7mq', NULL, 'Epic', 'epic', 'open', 2, '[]', '{}',
          '2026-11-15T10:00:00Z', NULL, NULL, NULL, NULL, NULL);
        INSERT INTO tickets VALUES ('k7mq.2', 'k7mq', 'Child', 'task', 'closed', 1,
          '["tier:opus"]', '{}', '2026-11-15T10:00:00Z', '2026-11-15T10:05:00Z',
          '2026-11-15T11:00:00Z', 'tree-07', 'local_1', '{"commits": ["abc", "def"]}');
        INSERT INTO aliases VALUES ('by-old.2', 'k7mq.2');
        """)
    db.commit()
    db.close()
    store_db = store.open_store(tmp_path / "store.sqlite")
    ingest.ingest(store_db, [ignite.HubTickets(hub)], HOST)
    beads = {
        r[0]: r[1:]
        for r in store_db.execute("SELECT id, parent, status, started_at FROM beads")
    }
    assert beads == {
        "by-k7mq": (None, "open", None),
        "by-old.2": ("by-k7mq", "closed", "2026-11-15T10:05:00Z"),
    }
    assert store_db.execute("SELECT label FROM labels").fetchall() == [("tier:opus",)]
    assert store_db.execute(
        "SELECT bead FROM ticket_aliases WHERE alias IN ('Tk7mq.2', 'k7mq.2')"
    ).fetchall() == [("by-old.2",), ("by-old.2",)]
    commits = store_db.execute(
        "SELECT json_extract(enact, '$.enact_commits') FROM beads WHERE id = 'by-old.2'"
    ).fetchone()
    assert commits == ("abc def",)


def test_the_hub_defaults_to_the_ignite_home(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv("IGNITE_HUB_DB", raising=False)
    monkeypatch.setenv("IGNITE_HOME", str(tmp_path / "home"))
    assert ignite.hub_path(tmp_path / "cache") == tmp_path / "home" / "hub" / "hub.db"
    monkeypatch.setenv("IGNITE_HUB_DB", str(tmp_path / "x.db"))
    assert ignite.hub_path(tmp_path / "cache") == tmp_path / "x.db"


def test_daemon_ingest_resumes_without_recounting_passes(
    ignite_daemon: Path, cache: Path
) -> None:
    sources = [
        *ignite.ticket_sources(cache),
        *fleet.sources(ignite_daemon, cache, "cygnus"),
    ]
    assert [s.name for s in sources] == [
        "ignite-mirror",
        "ignite-local-passes",
        "ignite-local-metrics",
    ]
    with closing(store.open_store(paths.store())) as db:
        ingest.ingest(db, sources, "cygnus")
        assert db.execute(
            "SELECT id, title, parent, status FROM beads WHERE id = 'by-older.1'"
        ).fetchone() == ("by-older.1", "A check regression", "by-t", "closed")
        assert db.execute(
            "SELECT id FROM ignite_tickets WHERE repair = 1"
        ).fetchall() == [("t.1",)]
        assert db.execute(
            'SELECT "commit", state FROM ignite_tip_rounds'
        ).fetchall() == [("abc", "verified")]
        assert db.execute(
            "SELECT host, provider, percent_used FROM plan_usage"
        ).fetchall() == [("cygnus", "claude", 12.0)]
        with closing(sqlite3.connect(ignite_daemon / "daemon.db")) as daemon, daemon:
            daemon.execute(
                "INSERT INTO passes VALUES ('rust', 'run', 2.0, ?)", (at(11, 1),)
            )
        ingest.ingest(db, sources, "cygnus")
        assert ingest.ingest(db, sources, "cygnus").total().rows == 0
        assert db.execute(
            "SELECT host, script, runs, seconds FROM checks"
        ).fetchall() == [("cygnus", "local", 3, 17.0)]


def test_local_hub_takes_precedence(ignite_daemon: Path, cache: Path) -> None:
    hub = ignite_daemon / "hub" / "hub.db"
    hub.parent.mkdir()
    with closing(make_hub(hub)):
        assert [s.name for s in ignite.ticket_sources(cache)] == ["ignite-beads"]
        assert [s.name for s in ignite.sources(cache, HOST)] == [
            "ignite-hub",
            "ignite-records",
            "ignite-imports",
            "ignite-plan-usage",
        ]
        assert fleet.sources(ignite_daemon, cache, HOST) == []


def fleet_ingest(
    hub: FleetHub, cache: Path
) -> tuple[sqlite3.Connection, list[ingest.Source]]:
    """This machine's store and its Ignite sources, with the fleet read from `hub`."""
    sources = [
        *ignite.ticket_sources(cache),
        *fleet.sources(hub.checkout, cache, "cygnus"),
    ]
    return store.open_store(paths.store()), sources


def counts(db: sqlite3.Connection) -> dict[str, object]:
    """What the store holds from the fleet, by what each figure is."""
    return {
        "records": db.execute(
            "SELECT host, count(*) FROM ignite_records GROUP BY host ORDER BY host"
        ).fetchall(),
        "checks": db.execute(
            "SELECT script, host, sum(runs), sum(cpu_seconds) FROM checks "
            "GROUP BY script, host ORDER BY script, host"
        ).fetchall(),
        "requests": db.execute(
            "SELECT host, count(*) FROM requests GROUP BY host"
        ).fetchall(),
        "samples": db.execute(
            "SELECT host, percent_used FROM plan_usage ORDER BY host"
        ).fetchall(),
    }


def test_fleet_ingest_reads_two_machines_and_resumes_without_recounting(
    fleet_hub: FleetHub, cache: Path
) -> None:
    db, sources = fleet_ingest(fleet_hub, cache)
    with closing(db):
        assert [s.name for s in sources] == [
            "ignite-mirror",
            "ignite-fleet-state",
            "ignite-fleet-records",
            "ignite-fleet-metrics",
            "ignite-local-passes",
            "ignite-local-metrics",
        ]
        summary = ingest.ingest(db, sources, "cygnus")
        assert {n: t.error for n, t in summary.sources.items() if t.error} == {}
        # This machine's own posted export is skipped: ingest here reads its sources.
        assert counts(db) == {
            "records": [("cygnus", 3), ("jarvis", 3)],
            "checks": [
                ("check", "jarvis", 1, 100.0),
                ("ci", "cygnus", 2, 360.0),
                ("ci", "jarvis", 2, 960.0),
                ("local", "cygnus", 2, 0.0),
            ],
            "requests": [("jarvis", 1)],
            "samples": [("cygnus", 12.0), ("jarvis", 40.0)],
        }
        assert db.execute(
            "SELECT ticket, first_bad, culprit_ticket FROM ignite_regressions"
        ).fetchall() == [("t.1", "k2", "t.2")]
        assert db.execute(
            "SELECT id, repair FROM ignite_tickets ORDER BY id"
        ).fetchall() == [("t", 0), ("t.1", 1)]
        before = counts(db)
        assert ingest.ingest(db, sources, "cygnus").total().rows == 0
        assert counts(db) == before
        fleet_hub.records.append(
            fleet_record(7, "m-1", "tip", "rust", 60, 60, "2026-11-15T13:00:00.000Z")
        )
        fleet_hub.requests.clear()
        ingest.ingest(db, sources, "cygnus")
        assert counts(db)["checks"] == [
            ("check", "jarvis", 1, 100.0),
            ("ci", "cygnus", 2, 360.0),
            ("ci", "jarvis", 3, 1020.0),
            ("local", "cygnus", 2, 0.0),
        ]
    # The hub is asked only for what arrived after the stored positions.
    assert "records?limit=5000&after=6" in fleet_hub.requests
    assert f"metrics?limit=5000&cursor=v1-{len(fleet_hub.metrics):x}" in (
        fleet_hub.requests
    )


def test_an_interrupted_fleet_ingest_resumes_where_it_stopped(
    fleet_hub: FleetHub, cache: Path
) -> None:
    # The state and the first page of records arrive; then the hub fails.
    fleet_hub.budget = 2
    db, sources = fleet_ingest(fleet_hub, cache)
    with closing(db):
        summary = ingest.ingest(db, sources, "cygnus")
        assert summary.sources["ignite-fleet-records"].error == (
            "the hub answered records with HTTP 503"
        )
        assert summary.sources["ignite-fleet-metrics"].error != ""
        assert counts(db)["records"] == [("cygnus", 1), ("jarvis", 1)]
        fleet_hub.budget = None
        summary = ingest.ingest(db, sources, "cygnus")
        assert {n: t.error for n, t in summary.sources.items() if t.error} == {}
        assert counts(db)["records"] == [("cygnus", 3), ("jarvis", 3)]
        assert counts(db)["checks"] == [
            ("check", "jarvis", 1, 100.0),
            ("ci", "cygnus", 2, 360.0),
            ("ci", "jarvis", 2, 960.0),
            ("local", "cygnus", 2, 0.0),
        ]


def test_an_unreachable_hub_leaves_the_local_measurements(
    fleet_hub: FleetHub, cache: Path
) -> None:
    fleet_hub.stop()
    db, sources = fleet_ingest(fleet_hub, cache)
    with closing(db):
        summary = ingest.ingest(db, sources, "cygnus")
        errors = {n: t.error for n, t in summary.sources.items() if t.error}
        assert list(errors) == [
            "ignite-fleet-state",
            "ignite-fleet-records",
            "ignite-fleet-metrics",
        ]
        # The first failure is remembered, so the hub's timeout is waited for once.
        assert len(set(errors.values())) == 1
        assert "cannot reach the hub" in errors["ignite-fleet-state"]
        assert counts(db)["checks"] == [("local", "cygnus", 2, 0.0)]
        assert counts(db)["samples"] == [("cygnus", 12.0)]
        assert db.execute("SELECT count(*) FROM beads").fetchone()[0] == 2


def test_fleet_reads_use_the_machine_token_the_ignite_command_uses(
    fleet_hub: FleetHub, ignite_daemon: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    expected = fleet.Hub(f"{fleet_hub.url}/test/api", TOKEN)
    hub = fleet.load(fleet_hub.checkout / "nested")
    assert hub == expected
    assert hub is not None
    _, found = hub.get("state", {})
    assert found["tips"] == [["k1", at(10, 30), at(10, 45), "verified"]]
    monkeypatch.setenv("IGNITE_TOKEN", "another")
    other = fleet.load(fleet_hub.checkout)
    assert other is not None
    with pytest.raises(ingest.SourceError, match="HTTP 401"):
        other.get("state", {})
    monkeypatch.delenv("IGNITE_TOKEN")
    (ignite_daemon / "machine.json").unlink()
    (ignite_daemon / "token").write_text(f" {TOKEN}\n", encoding="utf-8")
    assert fleet.load(fleet_hub.checkout) == expected
    (ignite_daemon / "token").unlink()
    missing = fleet.load(fleet_hub.checkout)
    assert missing is not None
    with pytest.raises(ingest.SourceError, match="no registered machine token"):
        missing.get("state", {})
