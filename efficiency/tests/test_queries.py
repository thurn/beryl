"""The query layer: `summary`, `epic`, `bead`, `session`, `time`, `dollars` and `metrics` over
a small synthetic store, through the command as an agent runs it."""

from __future__ import annotations

import argparse
import json
import re
from datetime import datetime, timezone
from pathlib import Path

import pytest

from beryl_efficiency import claude, cli, facts, ingest, store
from beryl_efficiency.beads import Beads
from beryl_efficiency.scope import Scope, parse_since
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record

HOST = "jarvis"
OPUS = "claude-opus-5-5"
ENACT = "python3 ~/enact/scripts/enact.py"
RECENT = "2026-11-10"


def at(day: int, hour: int, minute: int) -> str:
    return f"2026-11-{day:02d}T{hour:02d}:{minute:02d}:00.000Z"


def transcript(root: Path, name: str) -> Transcript:
    return Transcript(root / "projects" / PROJECT / f"{name}.jsonl")


def usage(context: int, output: int) -> tuple[int, int, int, int]:
    return (10, 90, context - 100, output)


BEADS: bytes = listing(
    bead_record("by-e", issue_type="epic", title="The epic"),
    bead_record(
        "by-e.1",
        parent="by-e",
        status="closed",
        closed_at="2026-11-15T10:30:05Z",
        labels=["tier:opus", "role:executor"],
        metadata={"enact_vizier": "v1", "enact_spawn_host": HOST},
    ),
    bead_record(
        "by-e.2",
        parent="by-e",
        status="closed",
        closed_at="2026-11-15T11:00:01Z",
        labels=["tier:sonnet", "role:executor"],
    ),
    bead_record(
        "by-x", status="closed", closed_at="2026-11-01T09:30:00Z", labels=["tier:opus"]
    ),
    bead_record(
        "by-far",
        status="closed",
        closed_at="2026-11-14T09:00:00Z",
        metadata={"enact_spawn_host": "cygnus"},
    ),
)


@pytest.fixture
def root(tmp_path: Path, cache: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A store with a lane of two tickets in one epic, a stretch on no ticket, an orchestrator
    charged to the epic, an old ticket outside the recent range, and one request by a model
    with no price."""
    (
        transcript(tmp_path, "t1")
        .request(at(15, 10, 0), "m0", usage(20_000, 300), model=OPUS)
        .bash(
            at(15, 10, 1),
            "m1",
            "c1",
            f"cd /work/beryl && {ENACT} start by-e.1",
            model=OPUS,
        )
        .result(at(15, 10, 2), "c1", '{"bead": "by-e.1"}')
        .request(
            at(15, 10, 5),
            "m2",
            usage(24_000, 500),
            model=OPUS,
            tools=(("c2", "Bash", {"command": "cargo test -p beryl_kernel"}),),
        )
        .result(at(15, 10, 20), "c2", stdout="x" * 400)
        .request(at(15, 10, 25), "m3", usage(30_000, 800), model=OPUS)
        .bash(
            at(15, 10, 30),
            "m4",
            "c4",
            f"{ENACT} close by-e.1 --reason d --continue",
            model=OPUS,
        )
        .result(at(15, 10, 30), "c4", '{"next": {"id": "by-e.2"}}')
        .request(at(15, 10, 40), "m5", usage(31_000, 200), model="claude-unknown-9")
        .request(at(15, 10, 50), "m6", usage(32_000, 400), model=OPUS)
        .bash(at(15, 11, 0), "m7", "c7", f"{ENACT} close by-e.2 --reason x", model=OPUS)
        .result(at(15, 11, 0), "c7")
        .prompt(at(15, 11, 3))
        .request(at(15, 11, 5), "m8", usage(33_000, 100), model=OPUS)
        .save()
    )
    (
        transcript(tmp_path, "v1")
        .request(at(15, 9, 0), "o1", usage(15_000, 100), model=OPUS)
        .bash(at(15, 9, 5), "o2", "c1", f"{ENACT} next --epic by-e", model=OPUS)
        .result(at(15, 9, 6), "c1")
        .request(at(15, 9, 7), "o3", usage(16_000, 100), model=OPUS)
        .save()
    )
    (
        transcript(tmp_path, "t2")
        .bash(at(1, 9, 0), "p1", "c1", f"{ENACT} start by-x", model=OPUS)
        .result(at(1, 9, 1), "c1")
        .request(at(1, 9, 10), "p2", usage(25_000, 900), model=OPUS)
        .bash(at(1, 9, 30), "p3", "c3", f"{ENACT} close by-x --reason y", model=OPUS)
        .result(at(1, 9, 30), "c3")
        .save()
    )
    sources: list[ingest.Source] = [
        Beads(lambda: BEADS),
        claude.Sessions(tmp_path / "sessions", CHECKOUT, HOST),
        claude.Transcripts(tmp_path / "projects", CHECKOUT, HOST),
    ]
    monkeypatch.setattr(cli, "sources", lambda: sources)
    assert cli.main(["ingest"]) == 0
    return tmp_path


def run(
    capsys: pytest.CaptureFixture[str], *argv: str
) -> tuple[str, dict[str, object]]:
    """The query's text and its JSON."""
    assert cli.main([*argv, "--no-ingest"]) == 0
    shown = capsys.readouterr().out
    assert cli.main([*argv, "--no-ingest", "--json"]) == 0
    found = json.loads(capsys.readouterr().out)
    assert isinstance(found, dict)
    return shown, found


def numbers_in(value: object) -> list[float]:
    if isinstance(value, bool) or value is None:
        return []
    if isinstance(value, (int, float)):
        return [float(value)]
    if isinstance(value, dict):
        return [n for v in value.values() for n in numbers_in(v)]
    if isinstance(value, list):
        return [n for v in value for n in numbers_in(v)]
    return []


# A number standing alone in text: not part of a date, a time, an id or a name.
NUMBER: re.Pattern[str] = re.compile(r"(?<![\w.:/-])-?\d+(?:\.\d+)?(?![\w.:/-])")

QUERIES: list[list[str]] = [
    ["summary"],
    ["epic", "by-e"],
    ["bead", "by-e.1"],
    ["session", "t1"],
    ["session", "t1", "--bead", "by-e.2"],
    ["metrics", "bead.dollars"],
    ["metrics", "request.context-tokens"],
    *(["time", "--by", by] for by in ("kind", "model", "tier", "role", "epic", "day")),
    *(
        ["dollars", "--by", by]
        for by in ("class", "source", "purpose", "model", "tier", "lane", "day")
    ),
]

NAMES: list[str] = [" ".join(q) for q in QUERIES]


@pytest.mark.parametrize("since", ["all", RECENT])
@pytest.mark.parametrize("query", QUERIES, ids=NAMES)
def test_every_number_in_the_text_is_in_the_json(
    root: Path, capsys: pytest.CaptureFixture[str], query: list[str], since: str
) -> None:
    shown, found = run(capsys, *query, "--since", since)
    have = numbers_in(found)
    body = shown.split("\nmore: ")[0]
    seen = NUMBER.findall(body)
    assert seen, body
    for token in seen:
        assert any(abs(float(token) - n) < 1e-9 for n in have), (token, body)
    assert shown.rstrip().splitlines()[-1].startswith("more: ")
    assert found["more"]


def lines_of(found: dict[str, object]) -> list[dict[str, object]]:
    lines = found["lines"]
    assert isinstance(lines, list)
    return lines


def number(found: dict[str, object], *path: str) -> float:
    value: object = found
    for key in path:
        assert isinstance(value, dict)
        value = value[key]
    assert isinstance(value, (int, float))
    return float(value)


def test_the_summarys_lines_sum_to_its_total_and_the_store(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, found = run(capsys, "summary", "--since", "all")
    lines = lines_of(found)
    assert [l["name"] for l in lines] == [
        "by-e",
        "(no epic)",
        "orchestration",
        "(no ticket)",
    ]
    for key in ("dollars", "agent_minutes", "person_minutes", "idle_minutes"):
        parts = sum(float(l[key]) for l in lines)
        assert parts == pytest.approx(
            number(found, "total", key), abs=0.01 * len(lines)
        )
    db = store.open_read_only(root / "cache" / "efficiency" / "store.sqlite")
    stored = float(db.execute("SELECT sum(dollars) FROM requests").fetchone()[0])
    assert number(found, "total", "dollars") == pytest.approx(stored, abs=0.005)
    # A model with no price makes every sum that holds its request a lower bound.
    assert found["unpriced"] == ["claude-unknown-9"]
    assert [l["lower_bound"] for l in lines] == [True, False, False, False]
    assert found["beads_closed"] == 4 and found["no_data"] == 1
    agent = found["agent_minutes"]
    assert isinstance(agent, dict) and agent["count"] == 3


def test_time_and_dollars_by_anything_sum_to_the_same_total(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, summary = run(capsys, "summary", "--since", "all")
    for by in ("kind", "tier", "epic", "day", "model"):
        _, found = run(capsys, "time", "--by", by, "--since", "all")
        rows = found["rows"]
        assert isinstance(rows, list)
        agent = sum(float(r["agent_minutes"]) for r in rows)
        assert agent == pytest.approx(
            number(summary, "total", "agent_minutes"), abs=0.05 * len(rows)
        ), by
    for by in ("class", "source", "purpose", "model", "role", "epic"):
        _, found = run(capsys, "dollars", "--by", by, "--since", "all")
        rows = found["rows"]
        assert isinstance(rows, list)
        dollars = sum(float(r["dollars"]) for r in rows)
        assert dollars == pytest.approx(
            number(summary, "total", "dollars"), abs=0.01 * len(rows)
        ), by
    _, tiers = run(capsys, "dollars", "--by", "tier", "--since", "all")
    rows = tiers["rows"]
    assert isinstance(rows, list)
    assert {r["name"] for r in rows} == {
        "opus",
        "sonnet",
        "orchestration",
        "(no ticket)",
    }


def test_a_cohort_filter_on_a_label_chooses_its_beads(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, everything = run(capsys, "summary", "--since", "all")
    _, opus = run(capsys, "summary", "--since", "all", "--tier", "opus")
    _, sonnet = run(capsys, "summary", "--since", "all", "--where", "tier=sonnet")
    assert (opus["beads_closed"], sonnet["beads_closed"]) == (2, 1)
    # Work on no ticket and orchestration fall outside a cohort chosen by label.
    for found in (opus, sonnet):
        tail = {str(l["name"]): float(l["dollars"]) for l in lines_of(found)}
        assert tail["(no ticket)"] == 0 and tail["orchestration"] == 0
    assert number(opus, "total", "dollars") + number(
        sonnet, "total", "dollars"
    ) < number(everything, "total", "dollars")
    db = store.open_read_only(root / "cache" / "efficiency" / "store.sqlite")
    loaded = facts.load(db)
    labels = Scope(labels=("role:executor",))
    assert loaded.cohort(labels) == {"by-e.1", "by-e.2"}
    assert loaded.cohort(Scope(epic="by-e")) == {"by-e", "by-e.1", "by-e.2"}
    assert loaded.cohort(Scope()) is None
    _, epic = run(capsys, "summary", "--since", "all", "--epic", "by-e")
    tail = {str(l["name"]): float(l["dollars"]) for l in lines_of(epic)}
    assert tail["orchestration"] > 0 and tail["(no ticket)"] == 0


def test_since_bounds_every_part_of_a_result(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, everything = run(capsys, "summary", "--since", "all")
    _, recent = run(capsys, "summary", "--since", RECENT)
    # by-x closed and was worked before the range.
    assert (everything["beads_closed"], recent["beads_closed"]) == (4, 3)
    names = {str(l["name"]): float(l["dollars"]) for l in lines_of(recent)}
    assert names["(no epic)"] == 0
    old = {str(l["name"]): float(l["dollars"]) for l in lines_of(everything)}
    assert number(recent, "total", "dollars") == pytest.approx(
        number(everything, "total", "dollars") - old["(no epic)"], abs=0.02
    )
    _, by_day = run(capsys, "dollars", "--by", "day", "--since", RECENT)
    rows = by_day["rows"]
    assert isinstance(rows, list) and all(str(r["name"]) >= RECENT for r in rows)
    _, series = run(capsys, "metrics", "request.context-tokens", "--since", RECENT)
    _, all_series = run(capsys, "metrics", "request.context-tokens", "--since", "all")
    assert series["count"] == 12 and all_series["count"] == 15
    _, bead = run(capsys, "bead", "by-x", "--since", RECENT)
    assert bead["requests"] == 0 and bead["sessions"] == []
    _, session = run(capsys, "session", "t2", "--since", RECENT)
    assert session["rows"] == []
    _, epic = run(capsys, "epic", "by-e", "--since", "2026-11-16")
    assert epic["beads_closed"] == 0 and number(epic, "total", "dollars") == 0


def test_the_epic_lists_its_closed_beads_and_orchestration(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    shown, found = run(capsys, "epic", "by-e")
    lines = lines_of(found)
    assert [l["name"] for l in lines] == ["by-e.1", "by-e.2", "orchestration"]
    assert all(float(l["agent_minutes"]) > 0 for l in lines)
    assert all(float(l["dollars"]) > 0 for l in lines)
    assert sum(float(l["dollars"]) for l in lines) == pytest.approx(
        number(found, "total", "dollars"), abs=0.03
    )
    assert "+" in shown and "no price for claude-unknown-9" in shown
    assert found["more"] == [
        "efficiency ticket by-e.1",
        "efficiency time --by kind --since all --epic by-e",
        "efficiency dollars --by source --since all --epic by-e",
    ]


def test_the_bead_query_follows_the_designs_layout(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    shown, found = run(capsys, "bead", "by-e.1")
    lines = shown.splitlines()
    assert lines[0] == "by-e.1  Ticket by-e.1"
    assert lines[1].startswith("closed 2026-11-15 10:30   role:executor   tier:opus")
    assert lines[2].startswith("agent min ") and "core min -" in lines[2]
    assert "rework 0.00" in lines[2]
    assert lines[5].startswith("lead min  queued ")
    assert found["more"] == ["efficiency session t1 --ticket by-e.1"]
    time = found["time"]
    assert isinstance(time, list) and {p["name"] for p in time} >= {"model"}
    unknown, _ = run(capsys, "bead", "by-far")
    assert "no data: it ran on cygnus" in unknown


def test_a_session_lists_its_rows_in_order(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, found = run(capsys, "session", "t1")
    rows = found["rows"]
    assert isinstance(rows, list)
    times = [str(r["time"]) for r in rows]
    assert times == sorted(times)
    assert found["requests"] == 9 and found["tool_calls"] == 4
    assert found["beads"] == ["by-e.1", "by-e.2"] and found["lower_bound"] is True
    _, one = run(capsys, "session", "t1", "--bead", "by-e.2")
    assert one["beads"] == ["by-e.2"] and one["requests"] == 3
    assert cli.main(["session", "t", "--no-ingest"]) == 1  # t1 and t2 both match.
    assert "names 2 sessions" in capsys.readouterr().err


def test_metrics_lists_and_draws_its_series(
    root: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _, listed = run(capsys, "metrics")
    found = listed["metrics"]
    assert isinstance(found, list)
    assert {"bead.dollars", "bead.agent-minutes", "request.context-tokens"} <= {
        m["name"] for m in found
    }
    _, series = run(capsys, "metrics", "bead.dollars", "--since", "all")
    days = series["series"]
    assert isinstance(days, list)
    # A ticket is dated by when it began; these tickets have no start, so by their creation.
    assert [(d["day"], d["count"]) for d in days] == [("2026-11-01", 3)]
    # Marks come from the checkout's own data/efficiency-changes.json, so only their metric is fixed.
    marks = series["marks"]
    assert isinstance(marks, list)
    assert all(m["metric"] == "bead.dollars" for m in marks)
    assert cli.main(["metrics", "no.such", "--no-ingest"]) == 1


def test_since_takes_hours_days_all_and_dates() -> None:
    noon = datetime(2026, 11, 15, 12, 0, tzinfo=timezone.utc)
    assert parse_since("24h", noon).since == "2026-11-14T12:00:00"
    assert parse_since("7d", noon).since == "2026-11-08T12:00:00"
    assert parse_since("all").since is None
    day = parse_since("2026-11-01")
    assert day.since is not None and day.since[:10] in ("2026-10-31", "2026-11-01")
    before = parse_since("2026-11-01").before()
    assert before is not None and before.until == day.since
    with pytest.raises(argparse.ArgumentTypeError):
        parse_since("yesterday")


def test_an_empty_store_answers_every_query(capsys: pytest.CaptureFixture[str]) -> None:
    for query in (["summary"], ["epic", "by-e"], ["bead", "by-e"], ["time"]):
        shown, found = run(capsys, *query, "--since", "all")
        assert found["query"] == query[0] and shown


@pytest.mark.parametrize("query", ["ticket", "bead"])
def test_ticket_query_aliases_keep_the_existing_result(query: str) -> None:
    args = cli.parser().parse_args([query, "Tk7mq", "--no-ingest"])
    assert args.command == "bead"
    assert args.id == "Tk7mq"


@pytest.mark.parametrize("flag", ["--ticket", "--bead"])
def test_ticket_session_flags_keep_the_existing_filter(flag: str) -> None:
    args = cli.parser().parse_args(["session", "session-1", flag, "Tk7mq"])
    assert args.bead == "Tk7mq"
