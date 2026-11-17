"""`just efficiency bead <id>`: one ticket's agent minutes by what the sessions were doing, its
dollars split each way, its requests and tokens, and the sessions that worked it. An epic's
figures include the orchestration charged to it. A ticket that ran on a host the store has no
data from says so instead of showing zeros."""

from __future__ import annotations

import sqlite3
import statistics
from collections import defaultdict
from dataclasses import asdict, dataclass, field

from . import ticket_ids, timing
from .facts import Facts, Window, load, minutes, money, total
from .pushes import Push, push_line, pushes
from .render import Result, describe, table, wrap
from .rollups import Part, agent_parts, bar, kind_name, number, split_lines
from .scope import Scope

TOKENS: tuple[str, ...] = (
    "input_tokens",
    "cache_write_tokens",
    "cache_read_tokens",
    "output_tokens",
)
SPLITS: tuple[str, ...] = ("class", "source", "purpose", "model")
# The most stretches a timeline strip holds; a longer one is cut into this many equal
# stretches, each named by what took most of it.
STRIP_STRETCHES = 400


@dataclass
class Worked:
    host: str
    provider: str
    session: str
    transcript: str
    title: str | None
    kind: str
    start: str
    end: str | None
    minutes: float


@dataclass
class Detail:
    category: str
    kind: str
    part: str
    minutes: float


@dataclass
class Stretch:
    """A stretch of the ticket's timeline in one category or kind, `at` minutes after the
    start of its first window. The ticket's windows are laid end to end, so the strip is as
    long as its wall minutes."""

    at: float
    minutes: float
    name: str


@dataclass
class Context:
    """The context a request read, `at` minutes along the ticket's timeline."""

    at: float
    tokens: int


@dataclass
class Caused:
    """Rework charged to the ticket: part of `fixer`'s direct dollars, by `rule`."""

    fixer: str
    rule: str
    dollars: float


@dataclass
class BeadReport:
    bead: str
    title: str | None
    status: str | None
    closed: str | None
    labels: list[str]
    ran_on: str | None
    no_data: bool
    requests: int | None
    tokens: dict[str, int] | None
    median_context: int | None
    fixed_prefix: int | None
    wall_minutes: float | None
    agent_minutes: float | None
    person_minutes: float | None
    idle_minutes: float | None
    core_minutes: float | None
    dollars: float | None
    lower_bound: bool
    rework: float | None
    rework_percent: float | None
    rework_from: list[Caused]
    commits: int
    cards: int
    queue_minutes: float | None
    work_minutes: float | None
    to_master_minutes: float | None
    time: list[Part]
    time_detail: list[Detail]
    dollars_by: dict[str, list[Part]]
    unpriced: list[str]
    sessions: list[Worked]
    strip: list[Stretch]
    contexts: list[Context]
    pushes: list[Push] = field(default_factory=list)


def core_minutes(facts: Facts, bead: str) -> float | None:
    found = facts.core_minutes(bead)
    return None if found is None else round(found, 1)


def rounded(value: float | None) -> float | None:
    return None if value is None else round(value, 1)


def wall(w: Window) -> float:
    return minutes(sum(w.time.values()))


def stretch_name(category: str, running: list[timing.Span]) -> str:
    """What a stretch of the strip is called: the kind of the first span running in it, as
    the time split names it."""
    return kind_name(category, running[0].kind if running else "")


def even(found: list[Stretch], length: float) -> list[Stretch]:
    """`found` cut into `STRIP_STRETCHES` equal stretches, each named by what took most of
    it, with neighbours of one name joined."""
    width = length / STRIP_STRETCHES
    seconds: list[dict[str, float]] = [
        defaultdict(float) for _ in range(STRIP_STRETCHES)
    ]
    for s in found:
        low, high = s.at, s.at + s.minutes
        first = min(int(low / width), STRIP_STRETCHES - 1)
        for index in range(first, STRIP_STRETCHES):
            left, right = index * width, (index + 1) * width
            if left >= high:
                break
            seconds[index][s.name] += min(high, right) - max(low, left)
    evened = [
        Stretch(i * width, width, max(names.items(), key=lambda kv: kv[1])[0])
        for i, names in enumerate(seconds)
        if names
    ]
    return joined(evened)


def joined(found: list[Stretch]) -> list[Stretch]:
    """`found` with touching neighbours of one name made one stretch."""
    out: list[Stretch] = []
    for s in found:
        last = out[-1] if out else None
        if last and last.name == s.name and abs(last.at + last.minutes - s.at) < 1e-9:
            last.minutes += s.minutes
        else:
            out.append(Stretch(s.at, s.minutes, s.name))
    return out


def timeline(
    db: sqlite3.Connection, bead: str, windows: list[Window]
) -> tuple[list[Stretch], list[Context]]:
    """The ticket's windows laid end to end in order of start: what each stretch was spent on,
    as time accounting splits it, and the context of each of the windows' requests."""
    stretches: list[Stretch] = []
    contexts: list[Context] = []
    spans: dict[tuple[str, str, str], list[timing.Span]] = {}
    offset = 0.0
    for w in sorted(windows, key=lambda w: w.start):
        closed = w.end
        if closed is None:
            continue
        key = (w.host, w.provider, w.transcript)
        if key not in spans:
            spans[key] = timing.spans_of(db, *key)
        start, end = timing.seconds(w.start), timing.seconds(closed)
        for low, high, category, running in timing.sweep(spans[key], start, end):
            stretches.append(
                Stretch(
                    offset + (low - start) / 60,
                    (high - low) / 60,
                    stretch_name(category, running),
                )
            )
        for time, tokens in db.execute(
            """SELECT time, context_tokens FROM requests WHERE host = ? AND provider = ?
                AND transcript = ? AND bead = ? AND time >= ? AND time <= ?
                AND context_tokens IS NOT NULL ORDER BY time""",
            (*key, bead, w.start, closed),
        ):
            at = offset + (timing.seconds(str(time)) - start) / 60
            contexts.append(Context(round(at, 2), int(tokens)))
        offset += (end - start) / 60
    found = joined(stretches)
    if len(found) > STRIP_STRETCHES:
        found = even(found, offset)
    return [
        Stretch(round(s.at, 2), round(s.minutes, 2), s.name) for s in found
    ], contexts


def report(db: sqlite3.Connection, bead: str, scope: Scope | None = None) -> Result:
    return report_from(db, load(db), scope or Scope(), bead)


def report_from(
    db: sqlite3.Connection, facts: Facts, scope: Scope, bead: str
) -> Result:
    bead = ticket_ids.stored(db, bead)
    found = facts.beads.get(bead)
    windows = [w for w in facts.chosen(scope) if w.bead == bead]
    sums = ", ".join(f"coalesce(sum({c}), 0)" for c in TOKENS)
    clause = " AND time >= ?" if scope.range.since else ""
    clause += " AND time < ?" if scope.range.until else ""
    bounds = [b for b in (scope.range.since, scope.range.until) if b]
    count_row = db.execute(
        f"SELECT count(*), {sums} FROM requests WHERE bead = ?{clause}",
        (bead, *bounds),
    ).fetchone()
    contexts = db.execute(
        f"SELECT context_tokens, prefix_tokens FROM requests WHERE bead = ?{clause}",
        (bead, *bounds),
    ).fetchall()
    context = [int(r[0]) for r in contexts if r[0] is not None]
    prefix = [int(r[1]) for r in contexts if r[1] is not None]
    have = {w.host for w in facts.windows}
    ran = found.ran_on() if found else None
    no_data = not windows and ran is not None and ran not in have
    whole = total(windows)
    detail: dict[tuple[str, str, str], float] = defaultdict(float)
    for w in windows:
        for key, seconds in w.time.items():
            detail[key] += seconds
    splits: defaultdict[str, defaultdict[str, float]] = defaultdict(
        lambda: defaultdict(float)
    )
    for w in windows:
        for (split, name), (_, value) in w.dollars.items():
            splits[split][name] += value
    charges = facts.rework().of(bead)
    reworked = money(sum(c.dollars for c in charges))
    lead = facts.lead(bead)
    direct = None if no_data else money(whole.dollars)
    strip, marks = timeline(db, bead, windows)
    result = BeadReport(
        bead,
        found.title if found else None,
        found.status if found else None,
        found.closed if found else None,
        sorted(found.labels) if found else [],
        ran,
        no_data,
        None if no_data else int(count_row[0]),
        None if no_data else {n: int(v) for n, v in zip(TOKENS, count_row[1:])},
        round(statistics.median(context)) if context else None,
        round(statistics.median(prefix)) if prefix else None,
        None if no_data else round(sum(wall(w) for w in windows), 1),
        None if no_data else minutes(whole.agent_seconds),
        None if no_data else minutes(whole.person_seconds),
        None if no_data else minutes(whole.idle_seconds),
        core_minutes(facts, bead),
        direct,
        bool(whole.unpriced),
        reworked,
        float(round(100 * reworked / direct)) if direct else None,
        [Caused(c.fixer, c.rule, money(c.dollars)) for c in charges],
        len(facts.history().by_bead.get(bead, [])),
        len(facts.history().cards_added(bead)),
        rounded(lead.queue),
        rounded(lead.work),
        rounded(lead.to_master),
        bar(agent_parts(windows), whole.agent_seconds / 60, 1),
        [
            Detail(c, k, p, minutes(s))
            for (c, k, p), s in sorted(detail.items(), key=lambda kv: -kv[1])
        ],
        {s: bar(dict(splits[s]), whole.dollars, 2) for s in SPLITS if s in splits},
        sorted(whole.unpriced),
        [
            Worked(
                w.host,
                w.provider,
                w.session,
                w.transcript,
                w.title,
                w.kind,
                w.start,
                w.end,
                wall(w),
            )
            for w in windows
        ],
        strip,
        marks,
        pushes(db, facts, Scope(range=scope.range), bead),
    )
    sessions = sorted({w.session for w in windows})
    return Result(
        "bead",
        asdict(result),
        lines(result),
        [f"efficiency session {s} --ticket {bead}" for s in sessions[:3]]
        or [f"efficiency sql \"select * from windows where bead = '{bead}'\""],
        {"pushes": "the ticket's pushes, each split into its fast check and the rest"}
        | describe(
            "agent_minutes",
            "person_minutes",
            "idle_minutes",
            "core_minutes",
            "dollars",
            "rework",
            "rework_percent",
            "rework_from",
            "commits",
            "cards",
            "queue_minutes",
            "work_minutes",
            "to_master_minutes",
            "lower_bound",
            "requests",
            "context_tokens",
            "prefix_tokens",
            "time",
            "strip",
        ),
    )


def lines(b: BeadReport) -> list[str]:
    head = f"{b.bead}" + (f"  {b.title}" if b.title else "")
    if b.no_data:
        return [
            head,
            f"no data: it ran on {b.ran_on}, and the store has no sessions from there",
        ]
    if not b.sessions and not b.requests:
        return [head, "no session in the store worked it"]
    providers = sorted({f"{s.provider} on {s.host}" for s in b.sessions})
    tiers = [l for l in b.labels if l.startswith(("tier:", "role:"))]
    closed = b.closed.replace("T", " ")[:16] if b.closed else None
    return [
        head,
        "   ".join(
            [f"closed {closed}" if closed else (b.status or "not in the store")]
            + tiers
            + providers
        ),
        f"agent min {number(b.agent_minutes, 1)}   core min {number(b.core_minutes, 1)}"
        + f"   dollars {number(b.dollars, 2, b.lower_bound)}"
        + f"   rework {number(b.rework, 2)}"
        + (f" ({b.rework_percent:.0f}%)" if b.rework_percent else ""),
        f"requests {b.requests}   median context {number(b.median_context, 0)}"
        + f"   fixed prefix {number(b.fixed_prefix, 0)}   wall min "
        + number(b.wall_minutes, 1),
        f"beside    waiting on a person {number(b.person_minutes, 1)}"
        + f" | idle {number(b.idle_minutes, 1)}",
        f"lead min  queued {number(b.queue_minutes, 1)} | worked "
        + f"{number(b.work_minutes, 1)} | to master {number(b.to_master_minutes, 1)}"
        + f"   commits {b.commits}   cards {b.cards}",
        *(
            wrap(
                "rework    from: ",
                [f"{c.fixer} {c.rule} {c.dollars:.2f}" for c in b.rework_from],
            )
            if b.rework_from
            else []
        ),
        "",
        *split_lines("time      ", b.time, 1),
        *(
            line
            for i, (s, parts) in enumerate(b.dollars_by.items())
            for line in split_lines(
                f"{'dollars' if i == 0 else '':<10}by {s}: ", parts, 2
            )
        ),
        *(
            [f"+ a lower bound: no price for {', '.join(b.unpriced)}"]
            if b.unpriced
            else []
        ),
        *(["", *(push_line(p) for p in b.pushes)] if b.pushes else []),
        "",
        *table(
            ["session", "title", "kind", "start", "minutes"],
            [[s.session, s.title, s.kind, s.start, s.minutes] for s in b.sessions],
        ),
    ]
