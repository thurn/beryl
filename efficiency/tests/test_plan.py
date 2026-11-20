"""Each ticket's share of the Claude plan, from a small fixture of plan usage samples and priced
requests whose factor is known: every dollar of Claude requests uses 0.05% of the weekly
limit, and every dollar of Fable requests 0.2% of the Fable weekly window."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import facts, ignite, ingest, plan, rollups, spend, store
from beryl_efficiency.scope import Scope
from tests.test_ignite import HUB_SCHEMA
from tests.test_spend import HOST, ingested, one_ticket

pytestmark: pytest.MarkDecorator = pytest.mark.usefixtures("utc_days")

WEEKLY = "Weekly · all models"
FABLE = "Weekly · Fable"
FACTOR = 0.05
FABLE_FACTOR = 0.2
OPUS = "claude-opus-5-5"
FABLE_MODEL = "claude-fable-5-2"
# The first period resets at the end of 2026-11-16; the second a week later.
RESETS = "2026-11-16T23:00:00.000Z"
NEXT_RESETS = "2026-11-23T23:00:00.000Z"


def at(day: int, hour: int, minute: int = 0) -> str:
    return f"2026-11-{day:02d}T{hour:02d}:{minute:02d}:00.000Z"


def request(
    db: sqlite3.Connection, moment: str, dollars: float, model: str, bead: str
) -> None:
    db.execute(
        "INSERT INTO requests (host, provider, transcript, message, time, model, dollars, "
        "bead) VALUES (?, 'claude', 't', ?, ?, ?, ?, ?)",
        (HOST, f"m{moment}{model}", moment, model, dollars, bead),
    )


def sample(
    db: sqlite3.Connection, moment: str, window: str, percent: float, resets: str
) -> None:
    db.execute(
        "INSERT INTO plan_usage VALUES (?, 'm-1', ?, ?, ?, ?)",
        (HOST, moment, window, percent, resets),
    )


def fixture(tmp_path: Path) -> sqlite3.Connection:
    """Hourly samples from 10:00 to 14:00 on 2026-11-15. Ticket `by-a` spends $10 of Opus and
    $2 of Fable in each hour; the next period has one sample and a $4 request of `by-b`.
    """
    db = store.open_store(tmp_path / "store.sqlite")
    weekly, fable = 20.0, 5.0
    for hour in range(10, 15):
        if hour > 10:
            request(db, at(15, hour - 1, 30), 10.0, OPUS, "by-a")
            request(db, at(15, hour - 1, 40), 2.0, FABLE_MODEL, "by-a")
            weekly += FACTOR * 12
            fable += FABLE_FACTOR * 2
        sample(db, at(15, hour), WEEKLY, weekly, RESETS)
        sample(db, at(15, hour), FABLE, fable, RESETS)
    request(db, at(17, 1), 4.0, OPUS, "by-b")
    sample(db, at(17, 2), WEEKLY, 1.0, NEXT_RESETS)
    db.execute(
        "INSERT INTO beads (id, title, status, created_at, closed_at) VALUES "
        "('by-a', 'a', 'closed', ?, ?), ('by-b', 'b', 'closed', ?, ?)",
        (at(15, 9), at(15, 15), at(17, 0), at(17, 3)),
    )
    db.commit()
    return db


def test_each_period_fits_its_known_factor(tmp_path: Path) -> None:
    found = plan.load(fixture(tmp_path))
    [first, second] = found.periods[WEEKLY]
    assert first.factor == pytest.approx(FACTOR)
    assert first.spread == pytest.approx(0.0, abs=1e-9)
    assert first.spans == 4
    # One sample fits nothing.
    assert second.factor is None and second.samples == 1
    # The Fable window counts only Fable dollars.
    [fable] = found.periods[FABLE]
    assert fable.factor == pytest.approx(FABLE_FACTOR)
    assert found.primary() == WEEKLY


def test_a_ticket_is_priced_by_its_period_or_borrows_the_previous_one(
    tmp_path: Path,
) -> None:
    db = fixture(tmp_path)
    found = plan.load(db)
    by_ticket = plan.ticket_requests(db, ["by-a", "by-b"])
    a = found.estimate(WEEKLY, by_ticket["by-a"])
    assert a is not None and not a.borrowed
    assert a.percent == pytest.approx(FACTOR * 48)
    fable = found.estimate(FABLE, by_ticket["by-a"])
    assert fable is not None and fable.percent == pytest.approx(FABLE_FACTOR * 8)
    b = found.estimate(WEEKLY, by_ticket["by-b"])
    assert b is not None and b.borrowed
    assert b.percent == pytest.approx(FACTOR * 4)
    # A request before the first period is unknown, not free.
    assert found.estimate(WEEKLY, [(0.0, OPUS, 1.0)]) is None


def test_the_summary_prints_median_and_p95_plan_percent(tmp_path: Path) -> None:
    db = fixture(tmp_path)
    result = rollups.summary(facts.load(db), Scope())
    figure = result.data["plan"]
    assert isinstance(figure, dict)
    assert figure["window"] == WEEKLY
    assert figure["count"] == 2 and figure["borrowed"] == 1
    assert figure["median"] == pytest.approx(FACTOR * (48 + 4) / 2)
    assert figure["p95"] == pytest.approx(FACTOR * (4 + 0.95 * 44), abs=1e-3)
    assert figure["factor"] == pytest.approx(FACTOR)
    assert figure["busiest_day"] == "2026-11-15"
    assert figure["per_hour"] == pytest.approx(FACTOR * 48 / 24, abs=1e-3)
    assert any(line.startswith("plan ") for line in result.lines)


def test_spend_emits_each_windows_plan_percent(tmp_path: Path) -> None:
    one_ticket(tmp_path)
    db = ingested(tmp_path)
    [(dollars,)] = db.execute("SELECT sum(dollars) FROM requests").fetchall()
    sample(db, "2026-11-15T09:00:00.000Z", WEEKLY, 10.0, RESETS)
    sample(db, "2026-11-15T11:00:00.000Z", WEEKLY, 10.0 + FACTOR * dollars, RESETS)
    [window] = spend.windows(db, HOST, None, plan.load(db))
    assert window["plan_window"] == WEEKLY
    assert window["plan_borrowed"] is False
    assert window["plan_percent"] == pytest.approx(FACTOR * dollars, abs=1e-4)
    assert window["plan"] == [[WEEKLY, window["plan_percent"], False]]
    # Without samples a window carries no plan fields.
    db.execute("DELETE FROM plan_usage")
    [window] = spend.windows(db, HOST, None, plan.load(db))
    assert "plan_percent" not in window


def test_samples_are_read_from_the_hubs_metrics(tmp_path: Path) -> None:
    hub = sqlite3.connect(tmp_path / "hub.db")
    hub.executescript(HUB_SCHEMA)
    hub.execute(
        "INSERT INTO machines VALUES ('m-1', 'Jarvis.local', 'macos', 8, 1, '[]', 'i1', ?)",
        (at(15, 9),),
    )
    rows = [
        {"at": at(15, 10), "status": "ok", "window": WEEKLY, "percent_used": 20.0}
        | {"resets_at": RESETS, "note": None},
        {"at": at(15, 10), "status": "ok", "window": "5-hour", "percent_used": 3.0}
        | {"resets_at": 1763251200, "note": None},
        {"at": at(15, 11), "status": "unavailable", "window": None}
        | {"percent_used": None, "resets_at": None, "note": "signed out"},
    ]
    hub.executemany(
        "INSERT INTO metrics VALUES ('m-1', 'plan_usage', ?, ?)",
        [(json.dumps(r), at(15, 12)) for r in rows],
    )
    hub.commit()
    db = store.open_store(tmp_path / "store.sqlite")
    ingest.ingest(db, [ignite.PlanSamples(tmp_path / "hub.db")], HOST)
    found = db.execute(
        "SELECT host, window, percent_used, resets_at FROM plan_usage ORDER BY window"
    ).fetchall()
    assert found == [
        ("jarvis", "5-hour", 3.0, "2025-11-16T00:00:00.000Z"),
        ("jarvis", WEEKLY, 20.0, RESETS),
    ]
    # A second ingest reads nothing new.
    ingest.ingest(db, [ignite.PlanSamples(tmp_path / "hub.db")], HOST)
    assert db.execute("SELECT count(*) FROM plan_usage").fetchone()[0] == 2
