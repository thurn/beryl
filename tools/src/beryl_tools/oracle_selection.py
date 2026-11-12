"""Which cards slow CI's oracle lane plays (docs/project-50k.md, "Verification": "Oracles run per
changed card in CI. The full sweep runs nightly").

Slow CI tests staging's commits since master, so the lane plays the cards whose files changed
since `origin/master`, plus a sample fixed by the commit, so that every run checks something
and a failure in the sample reproduces from the SHA. A change to what the exporters or the
oracle harnesses are built from ([`HARNESS`]) can break any card's translation: then every card
plays. A rules change does not select every card; the nightly's full sweep on staging
(nightly.py, `oracle_step`) does that. `by oracle` with no names stays the manual full sweep.
"""

from __future__ import annotations

import hashlib
import re

from . import card_names
from .changes import changed_paths, git

# The files the exporters and the harnesses are built from: the exporters and the rules they
# share, the oracle runner with its pins, the Java harnesses, and the oracles' allowlists.
HARNESS: re.Pattern[str] = re.compile(
    r"^tools/src/beryl_tools/(xmage\w*|forge|card_facts|oracles)\.py$"
    r"|^tools/oracles/"
    r"|^data/oracle-[\w-]+\.json$"
)

# How many cards the sample adds.
SAMPLE = 3

# Why a card played, as the run log's `chosen` says (runlog.py).
CHANGED = "card file changed"
SAMPLED = "sample of the commit"
EVERY = "exporters or harness changed"


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
    of `commit` among `every` card; None, for every card, when the harness changed or nothing
    says what changed."""
    if changed is None:
        return None
    if any(HARNESS.match(p) for p in changed):
        return None
    chosen = {card: CHANGED for card in card_names.cards_in(changed)}
    for card in sample(sorted(every), commit):
        chosen.setdefault(card, SAMPLED)
    return chosen


def slow_ci(every: list[str]) -> dict[str, str] | None:
    """`choose` among `every` card with scenarios for the commit checked out, against what
    changed since `origin/master`."""
    base = git("merge-base", "origin/master", "HEAD")
    return choose(changed_paths(base), git("rev-parse", "HEAD"), every)
