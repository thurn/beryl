"""Token accounting: each request's dollars, the parts of its context and what each tool result
costs to carry (docs/efficiency-dashboard.md, "Token accounting").

- Price: `data/efficiency-prices.json` holds dated tables of US dollars per million tokens.
  A request is priced by the latest table dated on or before its day, or by the earliest
  table for a request older than every table. A model in no table has null dollars.
- Parts of the context: the fixed prefix is the context of the transcript's first request.
  New content is the growth since the previous request less that request's output, never
  below zero, and zero when the context shrank. Carried content is the rest. A context under
  half the previous one is a compaction: everything beyond the prefix is new, from
  "compaction", and nothing earlier is carried any more.
- New content is assigned to the tool calls whose results arrived since the previous
  request, in proportion to their results' bytes, else to "user". Carried content is divided
  among everything added since the last compaction (by tool kind, "user", "compaction" and
  the model's own output) in proportion to how much each added.
- Carrying cost: a tool call's added tokens times the cache-read price of every later request
  in its transcript up to the next compaction.

Each window's dollars are stored in `window_dollars` three ways, each summing to the window's
dollars: by class, by source (fixed prefix, new and carried content by who added it, output)
by purpose (wake-up requests, whose every tool call is a wait, against working ones) and by
model.
"""

from __future__ import annotations

import bisect
import hashlib
import json
import sqlite3
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

# The token classes, as the `requests` table names them without `_tokens`.
CLASSES: tuple[str, ...] = ("input", "cache_write", "cache_read", "output")
# Kinds of command that only wait: a request whose every tool call is one woke for nothing.
WAIT_KINDS: frozenset[str] = frozenset({"orchestration wait", "slow CI wait"})
# A context under this share of the previous request's is a compaction.
COMPACTION = 0.5
# Raised whenever a change to this file changes stored figures, so that the next ingest
# accounts every transcript again, as it does when the price table changes.
VERSION = 2
MILLION = 1_000_000.0


@dataclass(frozen=True)
class Price:
    """Dollars per token of each class. `cache_write` is a 5-minute cache write and
    `cache_write_1h` a 1-hour one. A request whose context is over `above_tokens` is priced
    by `above` instead."""

    input: float
    cache_write: float
    cache_write_1h: float
    cache_read: float
    output: float
    above_tokens: int | None = None
    above: Price | None = None


@dataclass(frozen=True)
class Prices:
    tables: list[tuple[str, dict[str, Price]]]
    digest: str

    def find(self, model: str | None, day: str) -> Price | None:
        """The price of `model` on `day` (`YYYY-MM-DD`), or None when it has none. A model id
        with a suffix, such as `claude-x-1-20261101` or `claude-x-1[1m]`, takes its base
        id's price."""
        if not model or not self.tables:
            return None
        days = [d for d, _ in self.tables]
        index = max(bisect.bisect_right(days, day) - 1, 0)
        models = self.tables[index][1]
        if model in models:
            return models[model]
        for name, price in models.items():
            if model.startswith((name + "-", name + "[")):
                return price
        return None


def table_path() -> Path:
    """`data/efficiency-prices.json` in the checkout this package is in."""
    return Path(__file__).resolve().parents[3] / "data" / "efficiency-prices.json"


def number(entry: dict[str, object], key: str, model: str) -> float:
    value = entry.get(key)
    if not isinstance(value, (int, float)) or value < 0:
        raise ValueError(f"{model}: {key} must be a price per million tokens")
    return float(value) / MILLION


def price_of(entry: object, model: str) -> Price:
    if not isinstance(entry, dict):
        raise ValueError(f"{model}: not an object")
    above = entry.get("above")
    limit = above.get("tokens") if isinstance(above, dict) else None
    if above is not None and not isinstance(limit, int):
        raise ValueError(f"{model}: above needs a whole number of tokens")
    return Price(
        number(entry, "input", model),
        number(entry, "cache_write", model),
        number(entry, "cache_write_1h", model),
        number(entry, "cache_read", model),
        number(entry, "output", model),
        limit if isinstance(limit, int) else None,
        price_of(above, model) if isinstance(above, dict) else None,
    )


def parse(data: bytes) -> Prices:
    """The price tables in `data`, checked and sorted by date."""
    found = json.loads(data)
    tables = found.get("tables") if isinstance(found, dict) else None
    if not isinstance(tables, list) or not tables:
        raise ValueError("the price file needs a list of tables")
    parsed: list[tuple[str, dict[str, Price]]] = []
    for table in tables:
        if not isinstance(table, dict):
            raise ValueError("a price table is not an object")
        day, source, models = (
            table.get("as_of"),
            table.get("source"),
            table.get("models"),
        )
        if not isinstance(day, str) or len(day) != 10:
            raise ValueError("a price table needs as_of as YYYY-MM-DD")
        if not isinstance(source, str) or not source.startswith("https://"):
            raise ValueError(f"the {day} table needs the address of its source")
        if not isinstance(models, dict):
            raise ValueError(f"the {day} table needs models")
        parsed.append((day, {str(k): price_of(v, str(k)) for k, v in models.items()}))
    parsed.sort(key=lambda t: t[0])
    if len({d for d, _ in parsed}) != len(parsed):
        raise ValueError("two price tables have the same date")
    return Prices(parsed, hashlib.sha256(data).hexdigest()[:16])


def load(path: Path | None = None) -> Prices:
    return parse((path or table_path()).read_bytes())


@dataclass
class Request:
    message: str
    time: str
    model: str | None
    input: int
    cache_write: int
    cache_write_1h: int
    cache_read: int
    output: int
    context: int
    calls: list[str] = field(default_factory=list)


def class_dollars(price: Price | None, r: Request) -> dict[str, float] | None:
    """A request's dollars by class, or None when it has tokens and no price."""
    if price is not None and price.above is not None and price.above_tokens is not None:
        if r.context > price.above_tokens:
            price = price.above
    if price is None:
        if r.context or r.output:
            return None
        return {c: 0.0 for c in CLASSES}
    five_minute = max(r.cache_write - r.cache_write_1h, 0)
    return {
        "input": r.input * price.input,
        "cache_write": five_minute * price.cache_write
        + r.cache_write_1h * price.cache_write_1h,
        "cache_read": r.cache_read * price.cache_read,
        "output": r.output * price.output,
    }


def requests_of(db: sqlite3.Connection, key: tuple[str, str, str]) -> list[Request]:
    found: list[Request] = []
    for r in db.execute(
        """SELECT message, time, model, input_tokens, cache_write_tokens,
                coalesce(cache_write_1h_tokens, 0), cache_read_tokens, output_tokens,
                context_tokens
            FROM requests WHERE host = ? AND provider = ? AND transcript = ?
            ORDER BY time, message""",
        key,
    ):
        counts = [int(v or 0) for v in r[3:9]]
        written, hour, read, output, context = counts[1:]
        found.append(
            Request(
                str(r[0]),
                str(r[1]),
                r[2],
                counts[0],
                written,
                hour,
                read,
                output,
                context,
            )
        )
    return found


Totals = dict[tuple[str, str], list[float]]  # (split, name) -> [tokens, dollars]


def add(totals: Totals, split: str, name: str, tokens: float, dollars: float) -> None:
    entry = totals.setdefault((split, name), [0.0, 0.0])
    entry[0] += tokens
    entry[1] += dollars


def account_transcript(
    db: sqlite3.Connection, key: tuple[str, str, str], prices: Prices
) -> tuple[list[Request], dict[str, Totals]]:
    """Prices one transcript's requests, splits their contexts, writes each request's
    figures and each tool call's added tokens and carrying cost, and returns the requests
    with each one's totals by split, keyed by message."""
    requests = requests_of(db, key)
    calls = db.execute(
        """SELECT call, start, end, coalesce(kind, 'other'), coalesce(result_bytes, 0)
        FROM tool_calls WHERE host = ? AND provider = ? AND transcript = ?""",
        key,
    ).fetchall()
    times = [r.time for r in requests]
    kind_of = {str(c[0]): str(c[3]) for c in calls}
    arrived: dict[int, list[tuple[str, int]]] = defaultdict(list)
    for call, start, end, _, size in calls:
        # A call belongs to the request whose message holds it: the first at or after it.
        owner = bisect.bisect_left(times, str(start))
        if owner < len(requests):
            requests[owner].calls.append(str(call))
        if end is not None:
            # Its result is new content for the first request after it ended.
            reader = bisect.bisect_right(times, str(end))
            if reader < len(requests):
                arrived[reader].append((str(call), int(size)))
    figures: dict[str, Totals] = {}
    added: dict[str, float] = {}
    first_reader: dict[str, int] = {}
    pool: dict[str, float] = defaultdict(float)  # carried content by who added it
    read_prices: list[float] = []
    compactions: list[int] = []
    prefix = requests[0].context if requests else 0
    for index, r in enumerate(requests):
        price = prices.find(r.model, r.time[:10])
        found = class_dollars(price, r)
        totals: Totals = {}
        read_prices.append(price.cache_read if price else 0.0)
        fixed = min(prefix, r.context)
        sources: dict[str, float] = {}
        if index == 0:
            new = 0
        else:
            before = requests[index - 1]
            if r.context < before.context * COMPACTION:
                new = r.context - fixed
                sources = {"compaction": float(new)}
                pool.clear()
                compactions.append(index)
            elif r.context < before.context:
                new = 0
            else:
                new = min(
                    max(r.context - before.context - before.output, 0),
                    r.context - fixed,
                )
            if new and not sources:
                results = arrived.get(index, [])
                size = sum(s for _, s in results)
                if size:
                    for call, s in results:
                        share = new * s / size
                        added[call] = added.get(call, 0.0) + share
                        first_reader.setdefault(call, index)
                        sources[kind_of[call]] = sources.get(kind_of[call], 0.0) + share
                else:
                    sources = {"user": float(new)}
        carried = r.context - fixed - new
        if found is None:
            add(totals, "unpriced", str(r.model), r.context + r.output, 0.0)
        else:
            dollars = sum(found.values())
            for name, value in found.items():
                tokens = getattr(r, name)
                add(totals, "class", name, tokens, value)
            input_side = dollars - found["output"]
            per_token = input_side / r.context if r.context else 0.0
            add(totals, "source", "fixed prefix", fixed, fixed * per_token)
            for name, tokens in sources.items():
                add(totals, "source", f"new: {name}", tokens, tokens * per_token)
            held: float = sum(pool.values())
            for name, tokens in pool.items() if held and carried else []:
                share = float(carried) * tokens / held
                add(totals, "source", f"carried: {name}", share, share * per_token)
            if carried and not held:
                add(totals, "source", "carried: unknown", carried, carried * per_token)
            add(totals, "source", "output", r.output, found["output"])
            waits = [kind_of.get(c) in WAIT_KINDS for c in r.calls]
            purpose = "wake-up" if waits and all(waits) else "working"
            add(totals, "purpose", purpose, r.context + r.output, dollars)
            add(totals, "model", str(r.model), r.context + r.output, dollars)
        for name, tokens in sources.items():
            pool[name] += tokens
        pool["model output"] += r.output
        figures[r.message] = totals
        db.execute(
            """UPDATE requests SET dollars = ?, input_dollars = ?, cache_write_dollars = ?,
                cache_read_dollars = ?, output_dollars = ?, prefix_tokens = ?,
                new_tokens = ?
            WHERE host = ? AND provider = ? AND transcript = ? AND message = ?""",
            (
                sum(found.values()) if found is not None else None,
                *((found[c] for c in CLASSES) if found is not None else [None] * 4),
                fixed,
                new,
                *key,
                r.message,
            ),
        )
    # Carrying cost: the cache-read price of each later request up to the next compaction.
    later = [0.0] * (len(requests) + 1)
    stops = set(compactions)
    for index in range(len(requests) - 1, -1, -1):
        later[index] = read_prices[index] + (
            later[index + 1] if index + 1 not in stops else 0.0
        )
    db.execute(
        "UPDATE tool_calls SET added_tokens = NULL, carrying_dollars = NULL "
        "WHERE host = ? AND provider = ? AND transcript = ?",
        key,
    )
    for call, tokens in added.items():
        start = first_reader[call] + 1
        reread = later[start] if start < len(requests) and start not in stops else 0.0
        db.execute(
            "UPDATE tool_calls SET added_tokens = ?, carrying_dollars = ? "
            "WHERE host = ? AND provider = ? AND transcript = ? AND call = ?",
            (round(tokens), tokens * reread, *key, call),
        )
    return requests, figures


def account(
    db: sqlite3.Connection, host: str, provider: str, top: str, prices: Prices
) -> int:
    """Accounts a top-level transcript and its subagents, and writes `window_dollars` for
    its windows: a subagent's requests go to the window that holds its first record, as its
    rows' tickets do. Returns how many rows it wrote."""
    windows = [
        str(r[0])
        for r in db.execute(
            "SELECT start FROM windows WHERE host = ? AND provider = ? AND transcript = ? "
            "ORDER BY start",
            (host, provider, top),
        )
    ]
    db.execute(
        "DELETE FROM window_dollars WHERE host = ? AND provider = ? AND transcript = ?",
        (host, provider, top),
    )
    subagents = [
        str(r[0])
        for r in db.execute(
            "SELECT transcript FROM transcripts WHERE host = ? AND provider = ? "
            "AND parent = ?",
            (host, provider, top),
        )
    ]
    by_window: dict[str, Totals] = defaultdict(dict)
    for transcript in [top, *subagents]:
        requests, figures = account_transcript(db, (host, provider, transcript), prices)
        if not windows or not requests:
            continue
        anchor = requests[0].time if transcript != top else None
        for r in requests:
            moment = anchor or r.time
            window = windows[max(bisect.bisect_right(windows, moment) - 1, 0)]
            target = by_window[window]
            for (split, name), (tokens, dollars) in figures[r.message].items():
                add(target, split, name, tokens, dollars)
    written = 0
    for window, totals in by_window.items():
        for (split, name), (tokens, dollars) in totals.items():
            db.execute(
                "INSERT INTO window_dollars (host, provider, transcript, start, split, "
                "name, tokens, dollars) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (host, provider, top, window, split, name, tokens, dollars),
            )
            written += 1
    return written


def refresh(db: sqlite3.Connection, host: str, provider: str, tops: list[str]) -> int:
    """Accounts `tops` again, or every top-level transcript when the price table or this
    file's `VERSION` changed since the store's figures were written."""
    prices = load()
    digest = f"{VERSION}:{prices.digest}"
    setting = f"prices:{host}:{provider}"
    found = db.execute(
        "SELECT value FROM settings WHERE key = ?", (setting,)
    ).fetchone()
    if found is None or str(found[0]) != digest:
        tops = [
            str(r[0])
            for r in db.execute(
                "SELECT transcript FROM transcripts WHERE host = ? AND provider = ? "
                "AND parent IS NULL",
                (host, provider),
            )
        ]
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
            (setting, digest),
        )
    return sum(account(db, host, provider, top, prices) for top in tops)
