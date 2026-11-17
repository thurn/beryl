"""Token accounting: dollars, the parts of each context and carrying cost."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import claude, ingest, store, tokens
from beryl_efficiency.beads import Beads
from tests.synthetic import CHECKOUT, PROJECT, Transcript, listing
from tests.synthetic import bead as bead_record

HOST = "jarvis"
OPUS = "claude-opus-5-5"
ENACT = "python3 ~/enact/scripts/enact.py"
# Opus 5.5's prices on the test table, in dollars per token.
INPUT, WRITE, WRITE_HOUR, READ, OUTPUT = 4e-6, 5e-6, 8e-6, 0.2e-6, 20e-6


def at(seconds: int) -> str:
    return f"2026-11-15T10:{seconds // 60:02d}:{seconds % 60:02d}.000Z"


def transcript(root: Path, name: str = "t1") -> Transcript:
    return Transcript(root / "projects" / PROJECT / f"{name}.jsonl")


def ingested(root: Path, beads: bytes = b"[]") -> sqlite3.Connection:
    db = store.open_store(root / "store.sqlite")
    sources: list[ingest.Source] = [
        Beads(lambda: beads),
        claude.Sessions(root / "sessions", CHECKOUT, HOST),
        claude.Transcripts(root / "projects", CHECKOUT, HOST),
    ]
    ingest.ingest(db, sources, HOST)
    return db


def usage(context: int, output: int = 0) -> tuple[int, int, int, int]:
    """Usage with `context` tokens of context, nearly all of it read from the cache."""
    return (10, 90, context - 100, output)


def requests(db: sqlite3.Connection) -> list[tuple[str, float | None, int, int, int]]:
    """Each request's message, dollars, context, fixed prefix and new content."""
    return [
        (str(r[0]), r[1], int(r[2]), int(r[3]), int(r[4]))
        for r in db.execute(
            "SELECT message, dollars, context_tokens, prefix_tokens, new_tokens "
            "FROM requests ORDER BY time"
        )
    ]


def test_dollars_are_the_four_classes_times_their_prices(tmp_path: Path) -> None:
    transcript(tmp_path).request(
        at(0), "m1", usage=(1000, 2000, 30000, 400), model=OPUS, hour=500
    ).save()
    db = ingested(tmp_path)
    expected = (
        1000 * INPUT + 1500 * WRITE + 500 * WRITE_HOUR + 30000 * READ + 400 * OUTPUT
    )
    assert requests(db)[0][1] == pytest.approx(expected)


def test_a_model_with_no_price_has_no_dollars_and_marks_a_lower_bound(
    tmp_path: Path,
) -> None:
    (
        transcript(tmp_path)
        .request(at(0), "m1", model=OPUS)
        .request(at(1), "m2", model="claude-unknown-9")
        .save()
    )
    db = ingested(tmp_path)
    assert [r[1] is None for r in requests(db)] == [False, True]
    unpriced = db.execute(
        "SELECT name FROM window_dollars WHERE split = 'unpriced'"
    ).fetchall()
    assert [r[0] for r in unpriced] == ["claude-unknown-9"]


def grown(root: Path) -> Transcript:
    """A session whose context grows by two tool results, of 300 and 100 bytes, then by a
    person's message, then is compacted."""
    return (
        transcript(root)
        .request(
            at(0),
            "m1",
            usage=usage(30_000, 1000),
            model=OPUS,
            tools=(
                ("big", "Bash", {"command": "cargo test"}),
                ("small", "Read", {"file_path": "/work/a.rs"}),
            ),
        )
        .result(at(5), "big", stdout="x" * 300)
        .result(at(6), "small", stdout="y" * 100)
        .request(at(10), "m2", usage=usage(39_000, 500), model=OPUS)
        .prompt(at(20))
        .request(at(30), "m3", usage=usage(100_000, 200), model=OPUS)
        .request(at(40), "m4", usage=usage(99_000, 100), model=OPUS)
        .request(at(50), "m5", usage=usage(45_000, 100), model=OPUS)
        .request(at(60), "m6", usage=usage(46_000), model=OPUS)
    )


def test_the_three_parts_sum_to_the_context_and_growth_follows_result_size(
    tmp_path: Path,
) -> None:
    grown(tmp_path).save()
    db = ingested(tmp_path)
    found = requests(db)
    for message, _, context, prefix, new in found:
        carried = context - prefix - new
        assert carried >= 0, message
    assert [(m, p, n) for m, _, _, p, n in found] == [
        ("m1", 30_000, 0),
        # 39,000 less 30,000 less the previous output of 1,000.
        ("m2", 30_000, 8_000),
        ("m3", 30_000, 60_500),
        # The context shrank by less than half: nothing new.
        ("m4", 30_000, 0),
        # A compaction: everything beyond the prefix is new.
        ("m5", 30_000, 15_000),
        ("m6", 30_000, 900),
    ]
    added = dict(db.execute("SELECT call, added_tokens FROM tool_calls").fetchall())
    assert added == {"big": 6_000, "small": 2_000}


def test_carrying_cost_stops_at_a_compaction(tmp_path: Path) -> None:
    grown(tmp_path).save()
    db = ingested(tmp_path)
    cost = dict(db.execute("SELECT call, carrying_dollars FROM tool_calls").fetchall())
    # Read again by m3 and m4; m5 is a compaction, so it and m6 no longer carry it.
    assert cost["big"] == pytest.approx(6_000 * READ * 2)
    assert cost["small"] == pytest.approx(2_000 * READ * 2)


def test_a_beads_splits_each_sum_to_its_dollars(tmp_path: Path) -> None:
    sub = tmp_path / "projects" / PROJECT / "t1" / "subagents" / "agent-x.jsonl"
    (
        grown(tmp_path)
        .bash(at(70), "m7", "s", f"{ENACT} start by-a")
        .result(at(71), "s")
        .request(
            at(80),
            "m8",
            usage=usage(45_000),
            model=OPUS,
            tools=(("w", "Bash", {"command": f"{ENACT} wait by-a"}),),
        )
        .result(at(90), "w", stdout="z" * 50)
        .request(at(95), "m9", usage=usage(46_000, 300), model=OPUS)
        .save()
    )
    Transcript(sub).request(at(82), "s1", usage=usage(20_000, 50), model=OPUS).save()
    db = ingested(tmp_path, listing(bead_record("by-a")))
    total = db.execute(
        "SELECT sum(dollars) FROM requests WHERE bead = 'by-a'"
    ).fetchone()[0]
    splits = dict(db.execute("""SELECT d.split, sum(d.dollars) FROM window_dollars AS d
            JOIN windows AS w ON w.host = d.host AND w.provider = d.provider
                AND w.transcript = d.transcript AND w.start = d.start
            WHERE w.bead = 'by-a' GROUP BY d.split""").fetchall())
    # The `enact start` request is by a model with no price, so the ticket's dollars are a
    # lower bound, and every split leaves that request out alike.
    assert set(splits) == {"class", "source", "purpose", "model", "unpriced"}
    assert splits.pop("unpriced") == 0
    for split, dollars in splits.items():
        assert dollars == pytest.approx(total), split
    purpose = dict(
        db.execute(
            "SELECT name, count(*) FROM window_dollars WHERE split = 'purpose' "
            "GROUP BY name"
        ).fetchall()
    )
    assert "wake-up" in purpose


def test_prices_follow_the_table_in_force_and_long_prompts(tmp_path: Path) -> None:
    prices = tokens.parse(
        json.dumps(
            {
                "tables": [
                    {
                        "as_of": "2026-11-10",
                        "source": "https://example.com/a",
                        "models": {"m": price(1.0)},
                    },
                    {
                        "as_of": "2026-11-20",
                        "source": "https://example.com/b",
                        "models": {"m": {**price(2.0), "above": tier(100, 9.0)}},
                    },
                ]
            }
        ).encode()
    )
    early, late = prices.find("m", "2026-11-01"), prices.find(
        "m-20261101", "2026-11-25"
    )
    assert early is not None and early.input == pytest.approx(1e-6)
    assert late is not None and late.input == pytest.approx(2e-6)
    assert prices.find("other", "2026-11-25") is None
    small = tokens.Request("a", "t", "m", 50, 0, 0, 0, 0, 50)
    large = tokens.Request("b", "t", "m", 150, 0, 0, 0, 0, 150)
    assert tokens.class_dollars(late, small) == pytest.approx(
        {"input": 100e-6, "cache_write": 0, "cache_read": 0, "output": 0}
    )
    found = tokens.class_dollars(late, large)
    assert found is not None and found["input"] == pytest.approx(1350e-6)


def price(value: float) -> dict[str, float]:
    return {
        "input": value,
        "cache_write": value,
        "cache_write_1h": value,
        "cache_read": value,
        "output": value,
    }


def tier(above: int, value: float) -> dict[str, object]:
    return {"tokens": above, **price(value)}


def test_the_price_table_is_valid() -> None:
    prices = tokens.load()
    assert prices.find(OPUS, "2026-11-15") is not None
    for _, models in prices.tables:
        assert all(p.cache_read < p.input for p in models.values())
    with pytest.raises(ValueError, match="source"):
        tokens.parse(
            json.dumps({"tables": [{"as_of": "2026-11-15", "models": {}}]}).encode()
        )
