"""The parts every findings rule shares (docs/efficiency-dashboard.md, "Findings"): what a
rule is given, what it returns, and the helpers that choose rows in a query's scope.

A rule measures one kind of waste as `Item`s, each with its cost, its reference (the level
the cost would fall to if the waste were removed) and the saving between them, all in the
rule's one unit over the scope's range. `findings.py` scales the items to a week, ranks the
rules and renders them.
"""

from __future__ import annotations

import sqlite3
import statistics
from collections import defaultdict
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field

from .facts import NO_BEAD, ORCHESTRATION, Facts, Window
from .scope import Scope

# The three units, as the keys of `saving_per_week`. No unit is converted into another.
DOLLARS = "dollars"
AGENT_HOURS = "agent_hours"
CORE_HOURS = "core_hours"
UNITS: tuple[str, ...] = (DOLLARS, AGENT_HOURS, CORE_HOURS)
# The role of a ticket that has no `role:` label.
NO_ROLE = "(no role)"
# A cohort needs this many closed tickets before its tickets are compared with each other.
MIN_COHORT = 5


@dataclass
class Item:
    """One line of a rule's detail, such as one role or one kind of command. `saving` is
    None when the rule cannot estimate it. `beads` is each ticket's share of the saving (or
    of the cost, when the saving is unknown), used only to pick examples."""

    name: str
    count: int
    cost: float
    reference: float
    saving: float | None
    beads: dict[str, float] = field(default_factory=dict)

    def scaled(self, factor: float) -> Item:
        return Item(
            self.name,
            self.count,
            self.cost * factor,
            self.reference * factor,
            None if self.saving is None else self.saving * factor,
            self.beads,
        )


@dataclass(frozen=True)
class Context:
    db: sqlite3.Connection
    facts: Facts
    scope: Scope


@dataclass(frozen=True)
class Rule:
    """A named rule. `measure` returns its items over the scope's range, empty when there
    is nothing to report. `summary` writes one sentence from the items, already scaled to a
    week. `see` is the query that shows the wider breakdown the rule draws on."""

    name: str
    unit: str
    reference: str
    measure: Callable[[Context], list[Item]]
    summary: Callable[[list[Item]], str]
    see: str


def examples(beads: Iterable[dict[str, float]], count: int = 3) -> list[str]:
    """The `count` tickets with the largest shares across `beads`."""
    merged: dict[str, float] = defaultdict(float)
    for shares in beads:
        for bead, value in shares.items():
            merged[bead] += value
    ordered = sorted(merged.items(), key=lambda kv: (-kv[1], kv[0]))
    return [bead for bead, value in ordered[:count] if value > 0]


def percentile(values: Sequence[float], share: float) -> float:
    """The `share` percentile of `values` (0.1 for the 10th), inclusive of both ends, as
    `facts.spread` takes its 90th; the value itself when there is one."""
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    cuts = statistics.quantiles(ordered, n=100, method="inclusive")
    return cuts[round(share * 100) - 1]


def median(values: Sequence[float]) -> float:
    return statistics.median(values)


def role_of(facts: Facts, bead: str | None) -> str:
    """A ticket's role for grouping: its `role:` label, `orchestration` for an epic (the
    ticket an orchestrator session's work is charged to), `(no bead)` for none."""
    if bead is None:
        return NO_BEAD
    found = facts.beads.get(bead)
    if found is None:
        return NO_ROLE
    role = found.label("role")
    if role is not None:
        return role
    return ORCHESTRATION if found.issue_type == "epic" else NO_ROLE


def window_role(facts: Facts, w: Window) -> str:
    if w.kind == "orchestration":
        return ORCHESTRATION
    if w.kind == "none" or w.bead is None:
        return NO_BEAD
    return role_of(facts, w.bead)


def cohort_of(facts: Facts, bead: str) -> str:
    """A ticket's cohort for comparing tickets with each other: its role and tier."""
    found = facts.beads.get(bead)
    tier = found.label("tier") if found is not None else None
    return f"{role_of(facts, bead)} {'tier:' + tier if tier else '(no tier)'}"


def bounds(
    column: str, scope: Scope, provider: bool = True
) -> tuple[str, list[object]]:
    """A `WHERE` clause (starting with `AND`) keeping `column` in the scope's range and
    rows on its host and, for a table of sessions' rows (`provider`), its provider. Ticket
    filters are applied by `wanted`."""
    clause = ""
    given: list[object] = []
    if scope.range.since:
        clause += f" AND {column} >= ?"
        given.append(scope.range.since)
    if scope.range.until:
        clause += f" AND {column} < ?"
        given.append(scope.range.until)
    if scope.host:
        clause += " AND host = ?"
        given.append(scope.host)
    if provider and scope.provider:
        clause += " AND provider = ?"
        given.append(scope.provider)
    return clause, given


def wanted(facts: Facts, scope: Scope) -> Callable[[str | None], bool]:
    """Whether a row on a ticket belongs to the scope's cohort."""
    cohort = facts.cohort(scope)
    return lambda bead: cohort is None or bead in cohort


def grouped(
    rows: Iterable[tuple[str, str | None, float, float, float | None]],
) -> list[Item]:
    """Items from (item name, ticket, cost, reference, saving) rows, one per name, costliest
    saving first."""
    items: dict[str, Item] = {}
    for name, bead, cost, reference, saving in rows:
        item = items.setdefault(name, Item(name, 0, 0.0, 0.0, 0.0))
        item.count += 1
        item.cost += cost
        item.reference += reference
        if saving is None or item.saving is None:
            item.saving = None
        else:
            item.saving += saving
        if bead is not None:
            share = saving if saving is not None else cost
            item.beads[bead] = item.beads.get(bead, 0.0) + share
    return sorted(items.values(), key=lambda i: (-(i.saving or 0.0), -i.cost, i.name))


def total_saving(items: Sequence[Item]) -> float | None:
    if any(i.saving is None for i in items):
        return None
    return sum(i.saving or 0.0 for i in items)


def total_cost(items: Sequence[Item]) -> float:
    return sum(i.cost for i in items)


def top(items: Sequence[Item]) -> str:
    """The name of the item with the largest saving, for summaries."""
    return items[0].name if items else ""
