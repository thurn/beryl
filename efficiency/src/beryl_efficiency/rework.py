"""Rework: the cost of later fixes that a ticket's commits caused (docs/efficiency-dashboard.md,
"Rework").

Rework is a second view of costs already counted once as the fixing ticket's direct cost, so
it is reported beside direct cost and never added into a total. Only two traceable cases
count:

- Slow-CI breakages. A Steward ticket (one with `enact_ci_range`) names the commit range that
  broke slow CI. The tickets with commits in the range are the candidates, never the Steward
  itself. Those whose commits in the range changed a file that the Steward's own commits
  also changed are at fault; when none did, all are. The Steward's whole cost is shared
  equally among the tickets at fault.
- Fixes to cards. A `fix` commit that changes a card file is rework for the ticket that first
  added the file. The part of the fixing ticket's cost that is rework is its cost times the
  card files its `fix` commits changed, over every file its commits changed; that part is
  shared equally among those card files. Steward tickets are counted only by the first rule.

A ticket is never charged for its own commits: a card file a ticket fixes after adding it
itself is not rework, and does not count in the fixing ticket's rework part.
"""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Mapping
from dataclasses import dataclass

from .commits import History, is_card

SLOW_CI = "slow CI"
CARD_FIX = "card fix"


@dataclass(frozen=True)
class Charge:
    """Rework charged to `bead`: part of `fixer`'s direct cost, by `rule`."""

    bead: str
    fixer: str
    rule: str
    dollars: float


class Rework:
    """Every charge, and the card files that a later `fix` commit not landed by their adder
    changed."""

    def __init__(self, charges: list[Charge], fixed_later: set[str]) -> None:
        self.charges = charges
        self.fixed_later = fixed_later
        self._by_bead: dict[str, float] = defaultdict(float)
        for c in charges:
            self._by_bead[c.bead] += c.dollars

    def dollars(self, bead: str) -> float:
        return self._by_bead.get(bead, 0.0)

    def of(self, bead: str) -> list[Charge]:
        return [c for c in self.charges if c.bead == bead]


def slow_ci(history: History, steward: str, spec: str, cost: float) -> list[Charge]:
    """The Steward ticket's cost shared among the tickets at fault for the range `spec`."""
    fixed = {p for c in history.by_bead.get(steward, []) for p in c.paths}
    candidates: dict[str, set[str]] = defaultdict(set)
    for c in history.range(spec):
        bead = history.bead_of.get(c.sha)
        if bead is not None and bead != steward:
            candidates[bead] |= c.paths
    if not candidates:
        return []
    at_fault = sorted(b for b, paths in candidates.items() if paths & fixed) or sorted(
        candidates
    )
    return [Charge(b, steward, SLOW_CI, cost / len(at_fault)) for b in at_fault]


def card_fixes(history: History, fixer: str, cost: float) -> list[Charge]:
    """The rework part of `fixer`'s cost, shared among the card files its `fix` commits
    changed that another ticket added."""
    commits = history.by_bead.get(fixer, [])
    changed = {p for c in commits for p in c.paths}
    fixed: dict[str, str] = {}
    for c in commits:
        if not c.is_fix():
            continue
        for path in c.paths:
            adder = history.adder(path)
            first = history.card_added.get(path)
            if (
                is_card(path)
                and adder is not None
                and adder != fixer
                and first is not None
                and first.sha != c.sha
            ):
                fixed[path] = adder
    if not fixed or not changed:
        return []
    part = cost * len(fixed) / len(changed)
    return [
        Charge(adder, fixer, CARD_FIX, part / len(fixed))
        for _, adder in sorted(fixed.items())
    ]


def fixed_later(history: History) -> set[str]:
    """Card files that a later `fix` commit changed, other than one landed by the ticket that
    added the file."""
    found: set[str] = set()
    for c in history.commits.values():
        if not c.is_fix():
            continue
        lander = history.bead_of.get(c.sha)
        for path in c.paths:
            first = history.card_added.get(path)
            if first is None or first.sha == c.sha or first.time > c.time:
                continue
            adder = history.bead_of.get(first.sha)
            if lander is None or lander != adder:
                found.add(path)
    return found


def compute(
    history: History, stewards: Mapping[str, str], cost: Mapping[str, float]
) -> Rework:
    """Every ticket's rework. `stewards` gives each Steward ticket's `enact_ci_range`, and
    `cost` each ticket's direct dollars."""
    charges: list[Charge] = []
    for steward, spec in sorted(stewards.items()):
        charges += slow_ci(history, steward, spec, cost.get(steward, 0.0))
    for fixer in sorted(history.by_bead):
        if fixer not in stewards:
            charges += card_fixes(history, fixer, cost.get(fixer, 0.0))
    return Rework(charges, fixed_later(history))
