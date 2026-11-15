"""The roll-up queries (docs/efficiency-dashboard.md, "The command"): `summary`, `epic <id>`,
`time --by ...` and `dollars --by ...`.

Each query adds up `facts` rows into a typed result whose numbers are already rounded to
what the text shows, and a renderer that only formats them, so that the text, the JSON and
the page cannot disagree. Work on no bead and orchestration are lines of every roll-up, so
the lines sum to the total.
"""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Callable, Iterable
from dataclasses import asdict, dataclass

from .facts import (
    NO_BEAD,
    NO_EPIC,
    ORCHESTRATION,
    Bead,
    Facts,
    Totals,
    Window,
    line_of,
    minutes,
    money,
    spread,
    total,
)
from .render import Result, describe, mark, table, wrap
from .scope import Scope, now, parse_time
from .timing import AGENT_CATEGORIES

# How many parts a split's bar shows before the rest are added up as one.
BAR_PARTS = 8
TIME_BY: tuple[str, ...] = ("kind", "model", "tier", "role", "lane", "epic", "day")
DOLLARS_BY: tuple[str, ...] = (
    "class",
    "source",
    "purpose",
    "model",
    "tier",
    "role",
    "lane",
    "epic",
    "day",
)
# The splits stored in `window_dollars`, as against bys that group whole windows.
DOLLAR_SPLITS: frozenset[str] = frozenset({"class", "source", "purpose", "model"})
REST = "the rest"


@dataclass
class Part:
    """One part of a split: its value (minutes or dollars) and its share of the whole."""

    name: str
    value: float
    percent: float


@dataclass
class Line:
    """One line of a roll-up: a bead, an epic, or work on no bead or orchestration."""

    name: str
    title: str | None
    beads_closed: int
    agent_minutes: float
    person_minutes: float
    idle_minutes: float
    dollars: float
    lower_bound: bool
    rework: float


@dataclass
class Figure:
    """A per-bead figure over the closed beads: its median and 90th percentile, and the
    median over the range of the same length before, for its trend. All are null where
    nothing measures the figure yet."""

    median: float | None
    p90: float | None
    count: int
    before: float | None
    lower_bound: bool


def line(
    name: str, title: str | None, closed: int, found: Totals, rework: float = 0.0
) -> Line:
    return Line(
        name,
        title,
        closed,
        minutes(found.agent_seconds),
        minutes(found.person_seconds),
        minutes(found.idle_seconds),
        money(found.dollars),
        bool(found.unpriced),
        money(rework),
    )


def bar(parts: dict[str, float], whole: float, digits: int) -> list[Part]:
    """The largest parts, with the rest added up as one, each with its share of `whole`."""
    ordered = sorted(parts.items(), key=lambda kv: -kv[1])
    if len(ordered) > BAR_PARTS:
        rest = sum(v for _, v in ordered[BAR_PARTS - 1 :])
        ordered = ordered[: BAR_PARTS - 1] + [(REST, rest)]
    return [
        Part(
            name,
            round(value, digits),
            float(round(100 * value / whole)) if whole else 0.0,
        )
        for name, value in ordered
        if round(value, digits)
    ]


def kind_name(category: str, kind: str) -> str:
    """How a category of time and a kind of command read in a split."""
    if category == "tool":
        return kind or "tool"
    if category == "background":
        return f"background: {kind}" if kind else "background"
    return {
        "queued": "queued for cores",
        "person": "waiting on a person",
        "idle": "idle",
    }.get(category, category)


def agent_parts(windows: Iterable[Window]) -> dict[str, float]:
    """Agent minutes by category and kind."""
    found: dict[str, float] = defaultdict(float)
    for w in windows:
        for (category, kind, _), seconds in w.time.items():
            if category in AGENT_CATEGORIES:
                found[kind_name(category, kind)] += seconds / 60
    return found


def bead_figures(
    facts: Facts, scope: Scope, closed: list[Bead]
) -> tuple[list[float], list[float], bool, int]:
    """Each closed bead's agent minutes and dollars, over its windows at any time; whether a
    model with no price makes any of the dollars a lower bound; and how many beads have no
    window in the store."""
    by_bead = facts.bead_windows(scope)
    agent: list[float] = []
    dollars: list[float] = []
    lower = False
    missing = 0
    for b in closed:
        windows = by_bead.get(b.id, [])
        if not windows:
            missing += 1
            continue
        found = total(windows)
        agent.append(found.agent_seconds / 60)
        dollars.append(found.dollars)
        lower = lower or bool(found.unpriced)
    return agent, dollars, lower, missing


def core_figures(facts: Facts, closed: list[Bead]) -> list[float]:
    """The core minutes of each closed bead the sampler measured."""
    found = (facts.core_minutes(b.id) for b in closed)
    return [m for m in found if m is not None]


def figure(
    values: list[float], before: list[float], digits: int, lower: bool
) -> Figure:
    now_spread = spread(values, digits)
    then = spread(before, digits)
    median, p90, before_median = now_spread["median"], now_spread["p90"], then["median"]
    assert median is None or isinstance(median, float)
    assert p90 is None or isinstance(p90, float)
    assert before_median is None or isinstance(before_median, float)
    return Figure(median, p90, len(values), before_median, lower)


def rework_figures(facts: Facts, scope: Scope, closed: list[Bead]) -> list[float]:
    """The rework of each closed bead with windows in the store, the beads whose direct
    dollars the summary's dollars figure measures."""
    by_bead = facts.bead_windows(scope)
    found = facts.rework()
    return [found.dollars(b.id) for b in closed if by_bead.get(b.id)]


def days_in(facts: Facts, scope: Scope) -> float | None:
    """The range's length in days; for `all`, from the first window."""
    start = scope.range.since or min((w.start for w in facts.windows), default=None)
    if start is None:
        return None
    end = parse_time(scope.range.until) if scope.range.until else now()
    return max((end - parse_time(start)).total_seconds() / 86400, 1 / 24)


@dataclass
class Summary:
    since: str | None
    until: str | None
    range: str
    filters: str
    beads_closed: int
    per_day: float | None
    no_data: int
    agent_minutes: Figure
    core_minutes: Figure
    dollars: Figure
    rework: Figure
    total: Line
    time: list[Part]
    beside: list[Part]
    dollars_by_source: list[Part]
    lines: list[Line]
    unpriced: list[str]


def summary(facts: Facts, scope: Scope) -> Result:
    windows = facts.chosen(scope)
    closed = facts.closed(scope)
    agent, dollars, lower, missing = bead_figures(facts, scope, closed)
    before = scope.range.before()
    then = facts.closed(scope, before) if before else []
    agent_before, dollars_before, _, _ = bead_figures(facts, scope, then)
    groups: dict[str, list[Window]] = defaultdict(list)
    for w in windows:
        groups[line_of(facts, w)].append(w)
    closed_by: dict[str, int] = defaultdict(int)
    rework_by: dict[str, float] = defaultdict(float)
    found_rework = facts.rework()
    for b in closed:
        closed_by[facts.top_epic(b.id) or NO_EPIC] += 1
        rework_by[facts.top_epic(b.id) or NO_EPIC] += found_rework.dollars(b.id)
    epics = sorted(
        (
            line(
                name,
                facts.beads[name].title if name in facts.beads else None,
                closed_by.get(name, 0),
                total(groups.get(name, [])),
                rework_by.get(name, 0.0),
            )
            for name in set(groups) | set(closed_by)
            if name not in (NO_EPIC, ORCHESTRATION, NO_BEAD)
        ),
        key=lambda l: (-l.dollars, l.name),
    )
    tail = [
        line(
            name,
            None,
            closed_by.get(name, 0),
            total(groups.get(name, [])),
            rework_by.get(name, 0.0),
        )
        for name in (NO_EPIC, ORCHESTRATION, NO_BEAD)
    ]
    whole = total(windows)
    sources: dict[str, float] = defaultdict(float)
    for w in windows:
        for (split, name), (_, value) in w.dollars.items():
            if split == "source":
                sources[name] += value
    days = days_in(facts, scope)
    result = Summary(
        scope.range.since,
        scope.range.until,
        scope.range.name,
        scope.describe(),
        len(closed),
        round(len(closed) / days, 1) if days else None,
        missing,
        figure(agent, agent_before, 1, False),
        figure(core_figures(facts, closed), core_figures(facts, then), 1, False),
        figure(dollars, dollars_before, 2, lower),
        figure(
            rework_figures(facts, scope, closed),
            rework_figures(facts, scope, then),
            2,
            False,
        ),
        line("total", None, len(closed), whole, sum(rework_by.values())),
        bar(agent_parts(windows), whole.agent_seconds / 60, 1),
        [
            Part(n, minutes(s), 0.0)
            for n, s in (
                ("waiting on a person", whole.person_seconds),
                ("idle", whole.idle_seconds),
            )
        ],
        bar(sources, whole.dollars, 2),
        epics + tail,
        sorted(whole.unpriced),
    )
    top = next((l.name for l in epics), None)
    flags = scope.flags()
    return Result(
        "summary",
        asdict(result),
        summary_lines(result),
        ([f"efficiency epic {top}"] if top else [])
        + [
            f"efficiency time --by kind {flags}",
            f"efficiency dollars --by source {flags}",
        ],
        describe(
            "since",
            "beads_closed",
            "per_day",
            "no_data",
            "agent_minutes",
            "core_minutes",
            "dollars",
            "rework",
            "time",
            "beside",
            "dollars_by_source",
            "lines",
            "lower_bound",
        ),
    )


def number(value: float | int | None, digits: int, lower: bool = False) -> str:
    """A value with `digits` decimals, marked when it is a lower bound; `-` when
    unknown."""
    if value is None:
        return "-"
    return f"{value:.{digits}f}" + mark(lower)


def split_lines(head: str, parts: list[Part], digits: int) -> list[str]:
    """A split's parts after `head`, wrapped."""
    shown = [f"{p.name} {p.value:.{digits}f}" for p in parts] or ["-"]
    return wrap(head, shown)


def figure_text(name: str, f: Figure, digits: int) -> str:
    shown = f"{name} {number(f.median, digits, f.lower_bound)}"
    return shown + (
        f" (was {number(f.before, digits)})" if f.before is not None else ""
    )


def lines_table(lines: list[Line], first: str) -> list[str]:
    return table(
        [
            first,
            "closed",
            "agent min",
            "person min",
            "idle min",
            "dollars",
            "rework",
            "title",
        ],
        [
            [
                l.name,
                l.beads_closed,
                l.agent_minutes,
                l.person_minutes,
                l.idle_minutes,
                f"{l.dollars:.2f}{mark(l.lower_bound)}",
                f"{l.rework:.2f}",
                l.title,
            ]
            for l in lines
        ],
    )


def footnote(unpriced: list[str]) -> list[str]:
    if not unpriced:
        return []
    return [f"+ a lower bound: no price for {', '.join(unpriced)}"]


def summary_lines(s: Summary) -> list[str]:
    head = f"summary  since {s.since or 'the start'} ({s.range})"
    t = s.total
    return [
        head + (f"  {s.filters}" if s.filters else ""),
        f"beads closed {s.beads_closed}   per day {number(s.per_day, 1)}"
        + f"   no data {s.no_data}",
        "per bead, median   "
        + "   ".join(
            [
                figure_text("agent min", s.agent_minutes, 1),
                figure_text("core min", s.core_minutes, 1),
                figure_text("dollars", s.dollars, 2),
                figure_text("rework", s.rework, 2),
            ]
        ),
        *split_lines(f"time      agent min {t.agent_minutes:.1f}: ", s.time, 1),
        *split_lines("          beside: ", s.beside, 1),
        *split_lines(
            f"dollars   {t.dollars:.2f}{mark(t.lower_bound)}: ", s.dollars_by_source, 2
        ),
        "",
        *lines_table([*s.lines, t], "epic"),
        *footnote(s.unpriced),
    ]


@dataclass
class Epic:
    epic: str
    title: str | None
    status: str | None
    since: str | None
    range: str
    filters: str
    beads_closed: int
    total: Line
    lines: list[Line]
    unpriced: list[str]


def epic(facts: Facts, scope: Scope, epic_id: str) -> Result:
    """The epic's beads, each with its agent minutes and dollars in the range, and the
    orchestration charged to it."""
    within = Scope(scope.range, scope.labels, epic_id, scope.host, scope.provider)
    windows = facts.chosen(within)
    closed = facts.closed(within)
    groups: dict[str, list[Window]] = defaultdict(list)
    for w in windows:
        name = ORCHESTRATION if w.kind == "orchestration" else str(w.bead)
        groups[name].append(w)
    closed_ids = {b.id for b in closed}
    found_rework = facts.rework()
    beads = sorted(
        (
            line(
                name,
                facts.beads[name].title if name in facts.beads else None,
                1 if name in closed_ids else 0,
                total(groups.get(name, [])),
                found_rework.dollars(name),
            )
            for name in (set(groups) | closed_ids) - {ORCHESTRATION}
        ),
        key=lambda l: (-l.dollars, l.name),
    )
    whole = total(windows)
    found = facts.beads.get(epic_id)
    result = Epic(
        epic_id,
        found.title if found else None,
        found.status if found else None,
        scope.range.since,
        scope.range.name,
        within.describe(),
        len(closed),
        line("total", None, len(closed), whole, sum(l.rework for l in beads)),
        beads + [line(ORCHESTRATION, None, 0, total(groups.get(ORCHESTRATION, [])))],
        sorted(whole.unpriced),
    )
    flags = within.flags()
    top = next((l.name for l in beads if l.dollars), None)
    return Result(
        "epic",
        asdict(result),
        epic_lines(result),
        ([f"efficiency bead {top}"] if top else [])
        + [
            f"efficiency time --by kind {flags}",
            f"efficiency dollars --by source {flags}",
        ],
        describe("since", "beads_closed", "lines", "rework", "lower_bound"),
    )


def epic_lines(e: Epic) -> list[str]:
    t = e.total
    return [
        f"epic {e.epic}" + (f"  {e.title}" if e.title else ""),
        f"{e.status or 'not in the store'}   since {e.since or 'the start'} ({e.range})"
        + f"   beads closed {e.beads_closed}",
        f"agent min {t.agent_minutes:.1f}   dollars {t.dollars:.2f}{mark(t.lower_bound)}"
        + f"   rework {t.rework:.2f}",
        "",
        *lines_table([*e.lines, t], "bead"),
        *footnote(e.unpriced),
    ]


def day_of(moment: str) -> str:
    """The local day of a UTC time."""
    return parse_time(moment).astimezone().strftime("%Y-%m-%d")


def window_key(facts: Facts, by: str) -> Callable[[Window], str]:
    """For a by that groups whole windows, the row each window goes to."""

    def key(w: Window) -> str:
        if by == "day":
            return day_of(w.start)
        if by == "model":
            return w.model or "(unknown)"
        if by == "epic":
            return line_of(facts, w)
        bead = w.bead
        if w.kind == "none" or bead is None:
            return NO_BEAD
        if w.kind == "orchestration":
            return ORCHESTRATION
        found = facts.beads.get(bead)
        return (found.label(by) if found else None) or f"(no {by})"

    return key


@dataclass
class Row:
    name: str
    agent_minutes: float
    person_minutes: float
    idle_minutes: float
    percent: float


@dataclass
class TimeBy:
    by: str
    since: str | None
    range: str
    filters: str
    total: Row
    rows: list[Row]


def time_by(facts: Facts, scope: Scope, by: str) -> Result:
    """Agent minutes, and minutes beside them, by `by`. By `model` it is the model the
    session was started on."""
    windows = facts.chosen(scope)
    seconds: dict[str, list[float]] = defaultdict(lambda: [0.0, 0.0, 0.0])
    key = window_key(facts, by)
    for w in windows:
        for (category, kind, _), value in w.time.items():
            name = kind_name(category, kind) if by == "kind" else key(w)
            column = (
                0 if category in AGENT_CATEGORIES else 1 if category == "person" else 2
            )
            seconds[name][column] += value
    whole = total(windows)
    agent: float = whole.agent_seconds

    def row(name: str, values: list[float]) -> Row:
        return Row(
            name,
            minutes(values[0]),
            minutes(values[1]),
            minutes(values[2]),
            float(round(100 * values[0] / agent)) if agent else 0.0,
        )

    rows = sorted(
        (row(n, v) for n, v in seconds.items()),
        key=lambda r: (r.name if by == "day" else "", -r.agent_minutes, r.name),
    )
    result = TimeBy(
        by,
        scope.range.since,
        scope.range.name,
        scope.describe(),
        row("total", [whole.agent_seconds, whole.person_seconds, whole.idle_seconds]),
        rows,
    )
    flags = scope.flags()
    return Result(
        "time",
        asdict(result),
        [
            f"time by {by}  since {result.since or 'the start'} ({result.range})"
            + (f"  {result.filters}" if result.filters else ""),
            "",
            *table(
                [by, "agent min", "%", "person min", "idle min"],
                [
                    [r.name, r.agent_minutes, r.percent, r.person_minutes]
                    + [r.idle_minutes]
                    for r in [*rows, result.total]
                ],
            ),
        ],
        [f"efficiency time --by {b} {flags}" for b in TIME_BY if b != by][:3],
        describe("since", "agent_minutes", "person_minutes", "idle_minutes", "percent"),
    )


@dataclass
class Spent:
    name: str
    tokens: int
    dollars: float
    percent: float


@dataclass
class DollarsBy:
    by: str
    since: str | None
    range: str
    filters: str
    total: Spent
    rows: list[Spent]
    lower_bound: bool
    unpriced: dict[str, int]


def dollars_by(facts: Facts, scope: Scope, by: str) -> Result:
    """Dollars by `by`: one of the splits token accounting stores for each window, or a
    grouping of whole windows. Models with no price are listed with their tokens."""
    windows = facts.chosen(scope)
    found: dict[str, list[float]] = defaultdict(lambda: [0.0, 0.0])
    unpriced: dict[str, float] = defaultdict(float)
    key = window_key(facts, by)
    for w in windows:
        for (split, name), (tokens, value) in w.dollars.items():
            if split == "unpriced":
                unpriced[name] += tokens
            elif by in DOLLAR_SPLITS and split == by:
                found[name][0] += tokens
                found[name][1] += value
            elif by not in DOLLAR_SPLITS and split == "class":
                found[key(w)][0] += tokens
                found[key(w)][1] += value
    whole: float = sum(v[1] for v in found.values())
    tokens: float = sum(v[0] for v in found.values())

    def spent(name: str, values: list[float]) -> Spent:
        return Spent(
            name,
            round(values[0]),
            money(values[1]),
            float(round(100 * values[1] / whole)) if whole else 0.0,
        )

    rows = sorted(
        (spent(n, v) for n, v in found.items()),
        key=lambda r: (r.name if by == "day" else "", -r.dollars, r.name),
    )
    result = DollarsBy(
        by,
        scope.range.since,
        scope.range.name,
        scope.describe(),
        spent("total", [tokens, whole]),
        rows,
        bool(unpriced),
        {k: round(v) for k, v in sorted(unpriced.items())},
    )
    flags = scope.flags()
    return Result(
        "dollars",
        asdict(result),
        [
            f"dollars by {by}  since {result.since or 'the start'} ({result.range})"
            + (f"  {result.filters}" if result.filters else ""),
            "",
            *table(
                [by, "dollars", "%", "tokens"],
                [
                    [r.name, r.dollars, r.percent, r.tokens]
                    for r in [*rows, result.total]
                ],
            ),
            *(
                [
                    "+ the total is a lower bound: no price for "
                    + ", ".join(f"{k} ({v} tokens)" for k, v in result.unpriced.items())
                ]
                if result.unpriced
                else []
            ),
        ],
        [f"efficiency dollars --by {b} {flags}" for b in DOLLARS_BY if b != by][:3],
        describe("since", "dollars", "tokens", "percent", "lower_bound", "unpriced"),
    )
