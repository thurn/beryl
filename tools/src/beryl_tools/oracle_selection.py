"""Which cards slow CI's oracle lane plays, and which of a change's cards the fast check's oracle
lane has time for (`fitted`).

Slow CI tests staging's commits since master, so the lane plays the cards whose files changed
since `origin/master`, plus a sample fixed by the commit, so that every run checks something
and a failure in the sample reproduces from the SHA. Changes to exporters or harnesses
([`HARNESS`]) select scenarios through their exported input keys, rather than forcing every
card to play. A rules change does not select every card; the nightly's full sweep on staging
(nightly.py, `oracle_step`) does that. `by oracle` with no names stays the manual full sweep.
Of the chosen cards, the sample plays whole and the others only their scenarios that have not
passed; every other scenario that has not passed joins them (oracle_memory.py).
"""

from __future__ import annotations

import hashlib
import re
from collections.abc import Mapping
from typing import TYPE_CHECKING, cast

from . import card_files, deadline, oracle_memory, runlog
from .changes import changed_paths, git, master_base

if TYPE_CHECKING:
    from .selection import Record

# The files the exporters and the harnesses are built from: the exporters and the rules they
# share, the scenario dump they read (`beryl_scenario dump`: its JSON, its card facts and the
# scenario types), the oracle runner with its pins, the Java harnesses, and the oracles'
# allowlists.
HARNESS: re.Pattern[str] = re.compile(
    r"^tools/src/beryl_tools/(xmage\w*|forge|card_facts|oracles|oracle_exports|oracle_memory|oracle_selection)\.py$"
    r"|^rust/crates/beryl_scenario/src/(json|facts)\.rs$"
    r"|^rust/crates/beryl_scenario_types/"
    r"|^rust/crates/beryl_cards/src/bin/beryl_scenario\.rs$"
    r"|^tools/oracles/"
    r"|^tools/src/beryl_tools/BerylScenarioTest\.java$"
    r"|^data/oracle-[\w-]+\.json$"
    r"|^data/card-store\.tsv$"
)

# How many cards the sample adds.
SAMPLE = 3

# Why a card played, as the run log's `chosen` says (runlog.py).
CHANGED = "card file changed"
SAMPLED = "sample of the commit"


def sample(cards: list[str], commit: str, count: int = SAMPLE) -> list[str]:
    """`count` of `cards` chosen by a hash of `commit` and each card's name: the same commit
    picks the same cards, and another commit others."""

    def rank(card: str) -> str:
        return hashlib.sha256(f"{commit}\0{card}".encode()).hexdigest()

    return sorted(cards, key=rank)[:count]


def choose(
    changed: list[str] | None, commit: str, every: list[str]
) -> dict[str, str] | None:
    """The cards to play, each with why: those whose files are among `changed` and the sample
    of `commit` among `every` card; None, for every card, when nothing says what changed.
    Exported input keys add every scenario that has not passed after an exporter change.
    """
    if changed is None:
        return None
    chosen = {card: CHANGED for card in card_files.cards_in(changed)}
    for card in sample(sorted(every), commit):
        chosen.setdefault(card, SAMPLED)
    return chosen


def slow_ci(every: list[str]) -> dict[str, str] | None:
    """`choose` among `every` card with scenarios for the commit checked out, against what
    changed since `origin/master`."""
    return choose(changed_paths(master_base()), git("rev-parse", "HEAD"), every)


# The seconds of a scenario in one oracle with no record in the run log: the mean of the
# fast check's runs, startup shared out among the scenarios a run played.
SCENARIO_SECONDS = 8.0


def fitted(
    entries: list[dict[str, object]], known: Mapping[str, Record]
) -> list[dict[str, object]]:
    """The scenarios of the cards that fit what is left of the fast check's target
    (deadline.py), the cheapest cards first by their run-log `known` records. The rest are
    logged as skipped, for slow CI's oracle lane, which plays every card changed since
    `master`."""
    cost: dict[str, float] = {}
    for e in entries:
        for engine in ("xmage", "forge"):
            r = known.get(f"oracle:{e['card']}/{e['name']}/{engine}")
            seconds = r.seconds if r and r.seconds > 0 else SCENARIO_SECONDS
            card = unit(e)
            cost[card] = cost.get(card, 0.0) + seconds
    left, spent, kept = deadline.left() or 0.0, 0.0, set()
    for card in sorted(cost, key=lambda c: (cost[c], c)):
        if spent + cost[card] > left:
            break
        spent += cost[card]
        kept.add(card)
    cut = [e for e in entries if unit(e) not in kept]
    if cut:
        skip_all(
            cut,
            f"deferred to slow CI: {len(cost) - len(kept)} of {len(cost)} cards do not fit"
            f" the {max(left, 0):.0f} s left of the fast check's target",
        )
    return [e for e in entries if unit(e) in kept]


def unit(entry: Mapping[str, object]) -> str:
    """What `fitted` keeps or defers together: a card's scenarios, or one generated scenario,
    since the generated ones are a hundred unrelated positions under one card."""
    card = cast(str, entry["card"])
    return f"{card}/{entry['name']}" if card == oracle_memory.GENERATED else card


def skip_all(entries: list[dict[str, object]], why: str) -> None:
    """Reports every scenario of `entries` as skipped in each oracle, saying why."""
    print(f"oracle: skipped: {why}")
    for e in entries:
        for engine in ("xmage", "forge"):
            runlog.result(f"oracle:{e['card']}/{e['name']}/{engine}", "skip", 0.0)


# How each scenario status counts in the run log: an allowlisted divergence fails no run.
RUN_LOG_OUTCOMES = {
    "pass": "pass",
    "allowlisted": "pass",
    "fail": "fail",
    "unsupported": "skip",
}


def log_results(
    results: dict[str, dict[str, object]],
    seconds: dict[str, float],
    chosen: dict[str, str] | None = None,
) -> None:
    """Reports each scenario of each oracle to the run log as `oracle:<card>/<scenario>/<engine>`,
    with why its card was chosen when cards were (`oracle_selection`). Neither oracle times a
    scenario, so each engine's seconds are shared evenly among those it played."""
    for engine, result in results.items():
        scenarios = cast(list[dict[str, object]], result["scenarios"])
        played = sum(1 for s in scenarios if s["status"] != "unsupported") or 1
        for s in scenarios:
            outcome = RUN_LOG_OUTCOMES.get(cast(str, s["status"]), "fail")
            runlog.result(
                f"oracle:{s['card']}/{s['name']}/{engine}",
                outcome,
                0.0 if outcome == "skip" else seconds[engine] / played,
                chosen.get(cast(str, s["card"])) if chosen is not None else None,
            )
