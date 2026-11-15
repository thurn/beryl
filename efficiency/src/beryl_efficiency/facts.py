"""The rows every query adds up: beads with their labels, and bead windows with their seconds
by category (`window_time`) and their dollars by split (`window_dollars`).

A window is a stretch of a session spent on one bead (kind `bead`), a whole orchestrator
session charged to its epic (kind `orchestration`) or a stretch on no bead (kind `none`).
The windows of a top-level transcript cover it with no gap and no overlap, and a subagent's
seconds and dollars are already inside its parent's window, so summing windows counts
everything once. Queries choose windows with a `Scope` and add up these rows, so that every
roll-up's parts sum to its total.
"""

from __future__ import annotations

import json
import sqlite3
import statistics
from collections import defaultdict
from collections.abc import Iterable, Sequence
from dataclasses import dataclass, field

from .scope import Range, Scope
from .timing import AGENT_CATEGORIES

# The names of roll-up lines that are not a bead or an epic.
NO_BEAD = "(no bead)"
ORCHESTRATION = "orchestration"
NO_EPIC = "(no epic)"


@dataclass(frozen=True)
class Bead:
    id: str
    title: str | None
    parent: str | None
    issue_type: str | None
    status: str | None
    created: str | None
    started: str | None
    closed: str | None
    labels: frozenset[str]
    enact: dict[str, object]

    def label(self, key: str) -> str | None:
        """The value of the bead's first `key:value` label, if any."""
        found = sorted(l for l in self.labels if l.startswith(f"{key}:"))
        return found[0].split(":", 1)[1] if found else None

    def ran_on(self) -> str | None:
        host = self.enact.get("enact_spawn_host") or self.enact.get(
            "enact_worktree_host"
        )
        return str(host) if host else None


@dataclass
class Window:
    host: str
    provider: str
    session: str
    transcript: str
    start: str
    end: str | None
    kind: str
    bead: str | None
    label: str | None
    title: str | None
    model: str | None
    # Seconds by (category, kind of command, part).
    time: dict[tuple[str, str, str], float] = field(default_factory=dict)
    # (tokens, dollars) by (split, name); split `unpriced` names models with no price.
    dollars: dict[tuple[str, str], tuple[float, float]] = field(default_factory=dict)

    def seconds(self, categories: Iterable[str]) -> float:
        wanted = set(categories)
        return sum(s for (c, _, _), s in self.time.items() if c in wanted)

    def total_dollars(self) -> float:
        return sum(d for (split, _), (_, d) in self.dollars.items() if split == "class")

    def unpriced(self) -> set[str]:
        return {name for (split, name) in self.dollars if split == "unpriced"}


@dataclass
class Totals:
    """What a set of windows adds up to: seconds beside and inside agent minutes, dollars,
    and the models with no price that make the dollars a lower bound."""

    agent_seconds: float = 0.0
    person_seconds: float = 0.0
    idle_seconds: float = 0.0
    dollars: float = 0.0
    unpriced: set[str] = field(default_factory=set)
    windows: int = 0

    def add(self, w: Window) -> None:
        self.agent_seconds += w.seconds(AGENT_CATEGORIES)
        self.person_seconds += w.seconds(["person"])
        self.idle_seconds += w.seconds(["idle"])
        self.dollars += w.total_dollars()
        self.unpriced |= w.unpriced()
        self.windows += 1

    def json(self) -> dict[str, object]:
        return {
            "agent_minutes": minutes(self.agent_seconds),
            "person_minutes": minutes(self.person_seconds),
            "idle_minutes": minutes(self.idle_seconds),
            "dollars": money(self.dollars),
            "lower_bound": bool(self.unpriced),
            "windows": self.windows,
        }


def total(windows: Iterable[Window]) -> Totals:
    found = Totals()
    for w in windows:
        found.add(w)
    return found


def minutes(seconds: float) -> float:
    return round(seconds / 60, 1)


def money(dollars: float) -> float:
    return round(dollars, 2)


def spread(values: Sequence[float], digits: int) -> dict[str, object]:
    """The median and 90th percentile of `values`, both null when there are none."""
    if not values:
        return {"median": None, "p90": None, "count": 0}
    ordered = sorted(values)
    p90 = (
        statistics.quantiles(ordered, n=10, method="inclusive")[-1]
        if len(ordered) > 1
        else ordered[0]
    )
    return {
        "median": round(statistics.median(ordered), digits),
        "p90": round(p90, digits),
        "count": len(ordered),
    }


class Facts:
    """Every bead and every window in the store, loaded once per query."""

    def __init__(self, beads: dict[str, Bead], windows: list[Window]) -> None:
        self.beads = beads
        self.windows = windows
        self.children: dict[str, list[str]] = defaultdict(list)
        for b in beads.values():
            if b.parent:
                self.children[b.parent].append(b.id)

    def top_epic(self, bead: str | None) -> str | None:
        """The bead's topmost ancestor, or the bead itself when it is an epic with no
        parent; `None` for a bead under no epic."""
        if bead is None:
            return None
        seen: set[str] = set()
        current = bead
        while current not in seen:
            seen.add(current)
            found = self.beads.get(current)
            if found is None or not found.parent:
                break
            current = found.parent
        found = self.beads.get(current)
        if current == bead and (found is None or found.issue_type != "epic"):
            return None
        return current

    def under(self, epic: str) -> set[str]:
        """The epic and every bead under it."""
        found: set[str] = set()
        stack = [epic]
        while stack:
            current = stack.pop()
            if current not in found:
                found.add(current)
                stack.extend(self.children.get(current, []))
        return found

    def cohort(self, scope: Scope) -> set[str] | None:
        """The beads `scope`'s filters choose, or `None` when no filter chooses beads."""
        if not scope.chooses_beads():
            return None
        ids = set(self.under(scope.epic)) if scope.epic else set(self.beads)
        wanted = set(scope.labels)
        return {i for i in ids if i in self.beads and wanted <= self.beads[i].labels}

    def chosen(self, scope: Scope) -> list[Window]:
        """The windows that start in `scope`'s range, ran on its host and provider, and
        belong to beads its filters choose."""
        when = scope.range
        beads = self.cohort(scope)
        return [
            w
            for w in self.windows
            if when.holds(w.start)
            and (scope.host is None or w.host == scope.host)
            and (scope.provider is None or w.provider == scope.provider)
            and (beads is None or w.bead in beads)
        ]

    def closed(self, scope: Scope, within: Range | None = None) -> list[Bead]:
        """The beads in `scope`'s cohort, not epics, closed inside `within` (the scope's
        range by default)."""
        when = scope.range if within is None else within
        beads = self.cohort(scope)
        return [
            b
            for b in self.beads.values()
            if b.status == "closed"
            and b.issue_type != "epic"
            and when.holds(b.closed)
            and (beads is None or b.id in beads)
        ]

    def bead_windows(self, scope: Scope) -> dict[str, list[Window]]:
        """Every window of kind `bead` that `scope`'s host and provider allow, by bead, at
        any time: a bead's cost counts wherever its windows fall."""
        found: dict[str, list[Window]] = defaultdict(list)
        for w in self.windows:
            if (
                w.kind == "bead"
                and w.bead is not None
                and (scope.host is None or w.host == scope.host)
                and (scope.provider is None or w.provider == scope.provider)
            ):
                found[w.bead].append(w)
        return found


def line_of(facts: Facts, w: Window) -> str:
    """The roll-up line a window belongs to in a roll-up by epic."""
    if w.kind == "none" or w.bead is None:
        return NO_BEAD
    if w.kind == "orchestration":
        return ORCHESTRATION
    return facts.top_epic(w.bead) or NO_EPIC


def load(db: sqlite3.Connection) -> Facts:
    labels: dict[str, set[str]] = defaultdict(set)
    for bead, label in db.execute("SELECT bead, label FROM labels"):
        labels[str(bead)].add(str(label))
    beads = {
        str(r[0]): Bead(
            str(r[0]),
            r[1],
            r[2] or None,
            r[3],
            r[4],
            r[5],
            r[6],
            r[7],
            frozenset(labels.get(str(r[0]), set())),
            json.loads(r[8]) if r[8] else {},
        )
        for r in db.execute(
            "SELECT id, title, parent, issue_type, status, created_at, started_at, "
            "closed_at, enact FROM beads"
        )
    }
    windows: dict[tuple[str, str, str, str], Window] = {}
    for r in db.execute(
        """SELECT w.host, w.provider, w.session, w.transcript, w.start, w.end,
            w.kind, w.bead, w.label, s.title, s.model
        FROM windows AS w LEFT JOIN sessions AS s ON s.host = w.host
            AND s.provider = w.provider AND s.transcript = w.transcript
        ORDER BY w.start"""
    ):
        key = (str(r[0]), str(r[1]), str(r[3]), str(r[4]))
        windows[key] = Window(
            str(r[0]),
            str(r[1]),
            str(r[2]),
            str(r[3]),
            str(r[4]),
            r[5],
            str(r[6]),
            r[7],
            r[8],
            r[9],
            r[10],
        )
    for host, provider, transcript, start, category, kind, part, seconds in db.execute(
        "SELECT host, provider, transcript, start, category, kind, part, seconds "
        "FROM window_time"
    ):
        w = windows.get((str(host), str(provider), str(transcript), str(start)))
        if w is not None:
            w.time[(str(category), str(kind), str(part))] = float(seconds)
    for host, provider, transcript, start, split, name, tokens, dollars in db.execute(
        "SELECT host, provider, transcript, start, split, name, tokens, dollars "
        "FROM window_dollars"
    ):
        w = windows.get((str(host), str(provider), str(transcript), str(start)))
        if w is not None:
            w.dollars[(str(split), str(name))] = (float(tokens), float(dollars))
    return Facts(beads, list(windows.values()))
