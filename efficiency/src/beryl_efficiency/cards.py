"""`just efficiency cards --by <key>`: the card cohort's cost per card (docs/efficiency-
dashboard.md, "Units" and "Rework"). The card cohort is the closed beads whose commits first
added a card file under `rust/cards/`. Per card is over the card files those beads added:
direct dollars and agent minutes, rework dollars beside them, and the share of the cards a
later `fix` commit by another bead changed. The metrics of cards, rework and lead time are
registered here too."""

from __future__ import annotations

import sqlite3
from collections import defaultdict
from dataclasses import asdict, dataclass

from .facts import NO_EPIC, Bead, Facts, total
from .machine import Capacity, capacity, machine_cores
from .metrics import Metric, Points, register
from .render import Result, describe, mark, table
from .scope import Scope

NO_LABEL = "-"


@dataclass
class CardGroup:
    """One line of the cards query. `cards` counts every card file the line's beads added;
    the per-card figures are over the cards of beads with windows in the store, and
    `no_data` counts the beads without."""

    name: str
    beads: int
    no_data: int
    cards: int
    direct: float | None
    rework: float | None
    agent_minutes: float | None
    fixed_later: float | None
    lower_bound: bool


@dataclass
class Cards:
    by: str
    since: str | None
    range: str
    filters: str
    groups: list[CardGroup]
    total: CardGroup
    capacity: Capacity


class Tally:
    def __init__(self) -> None:
        self.beads = 0
        self.no_data = 0
        self.cards = 0
        self.measured = 0
        self.fixed = 0
        self.direct = 0.0
        self.rework = 0.0
        self.agent_seconds = 0.0
        self.lower = False

    def group(self, name: str) -> CardGroup:
        def per(value: float, digits: int) -> float | None:
            return round(value / self.measured, digits) if self.measured else None

        return CardGroup(
            name,
            self.beads,
            self.no_data,
            self.cards,
            per(self.direct, 2),
            per(self.rework, 2),
            per(self.agent_seconds / 60, 1),
            float(round(100 * self.fixed / self.cards)) if self.cards else None,
            self.lower,
        )


def key_of(facts: Facts, bead: Bead, by: str) -> str:
    if by == "epic":
        return facts.top_epic(bead.id) or NO_EPIC
    return bead.label(by) or NO_LABEL


def card_beads(facts: Facts, scope: Scope) -> list[tuple[Bead, list[str]]]:
    """The closed beads in the scope that added card files, with the files."""
    history = facts.history()
    found = [(b, history.cards_added(b.id)) for b in facts.closed(scope)]
    return [(b, cards) for b, cards in found if cards]


def report(db: sqlite3.Connection, facts: Facts, scope: Scope, by: str) -> Result:
    windows = facts.bead_windows(scope)
    added = card_beads(facts, scope)
    rework = facts.rework()
    tallies: dict[str, Tally] = defaultdict(Tally)
    whole = Tally()
    for b, cards in added:
        fixed = sum(1 for c in cards if c in rework.fixed_later)
        mine = windows.get(b.id, [])
        spent = total(mine)
        for t in (tallies[key_of(facts, b, by)], whole):
            t.beads += 1
            t.cards += len(cards)
            t.fixed += fixed
            if not mine:
                t.no_data += 1
                continue
            t.measured += len(cards)
            t.direct += spent.dollars
            t.rework += rework.dollars(b.id)
            t.agent_seconds += spent.agent_seconds
            t.lower = t.lower or bool(spent.unpriced)
    groups = sorted(
        (t.group(name) for name, t in tallies.items()), key=lambda g: (-g.cards, g.name)
    )
    result = Cards(
        by,
        scope.range.since,
        scope.range.name,
        scope.describe(),
        groups,
        whole.group("total"),
        capacity(facts, scope, machine_cores(db), [b for b, _ in added]),
    )
    flags = scope.flags()
    return Result(
        "cards",
        asdict(result),
        lines(result),
        [
            f"efficiency metrics card.dollars {flags}".rstrip(),
            f"efficiency metrics bead.rework-share {flags}".rstrip(),
        ],
        describe(
            "since",
            "cards",
            "rework",
            "fixed_later",
            "per_card",
            "lower_bound",
            "capacity",
        ),
    )


def number(value: float | None, digits: int, lower: bool = False) -> str:
    return "-" if value is None else f"{value:.{digits}f}{mark(lower)}"


def lines(c: Cards) -> list[str]:
    k = c.capacity
    head = f"cards by {c.by}  since {c.since or 'the start'} ({c.range})"
    rows = [
        [
            g.name,
            g.beads,
            g.no_data,
            g.cards,
            number(g.direct, 2, g.lower_bound),
            number(g.rework, 2),
            number(g.agent_minutes, 1),
            "-" if g.fixed_later is None else f"{g.fixed_later:.0f}%",
        ]
        for g in [*c.groups, c.total]
    ]
    return [
        head + (f"  {c.filters}" if c.filters else ""),
        "per card: direct dollars, rework dollars and agent minutes",
        "",
        *table(
            [
                c.by,
                "beads",
                "no data",
                "cards",
                "direct",
                "rework",
                "agent min",
                "fixed later",
            ],
            rows,
        ),
        "",
        f"capacity  cores per session {number(k.mean_cores, 1)} mean, "
        f"{number(k.p90_cores, 1)} p90 over {k.beads} beads; parallel capacity "
        f"{number(k.sessions, 0)} sessions",
    ]


def per_card(dollars: bool) -> Metric:
    def points(_: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
        windows = facts.bead_windows(scope)
        found: Points = []
        for b, cards in card_beads(facts, scope):
            began = b.began()
            if began and windows.get(b.id):
                t = total(windows[b.id])
                value = t.dollars if dollars else t.agent_seconds / 60
                found.append((began, value / len(cards)))
        return found

    if dollars:
        return Metric(
            "card.dollars", "direct dollars per card file added", "dollars", 2, points
        )
    return Metric(
        "card.agent-minutes",
        "agent minutes per card file added",
        "minutes",
        1,
        points,
    )


def rework_share(_: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    """Each closed bead's rework as a percent of its direct dollars, for beads with
    dollars in the store."""
    windows = facts.bead_windows(scope)
    rework = facts.rework()
    found: Points = []
    for b in facts.closed(scope):
        direct = total(windows.get(b.id, [])).dollars
        began = b.began()
        if began and direct > 0:
            found.append((began, 100 * rework.dollars(b.id) / direct))
    return found


def lead_minutes(_: sqlite3.Connection, facts: Facts, scope: Scope) -> Points:
    """Each closed bead's minutes from creation to its last commit reaching master."""
    found: Points = []
    for b in facts.closed(scope):
        whole = facts.lead(b.id).whole
        began = b.began()
        if began and whole is not None:
            found.append((began, whole))
    return found


register(per_card(True))
register(per_card(False))
register(
    Metric(
        "bead.rework-share",
        "rework as a percent of direct dollars per closed bead",
        "percent",
        0,
        rework_share,
    )
)
register(
    Metric(
        "bead.lead-minutes",
        "minutes from a closed bead's creation to its last commit reaching master",
        "minutes",
        0,
        lead_minutes,
    )
)
