"""The Forge census: which mechanics block the most cards from Beryl's vocabulary.

Forge's card scripts are read only to count. For each script of a card in the pool this reads
the kind names that say what the card does and nothing else: the API name of each ability
(`AB$`, `SP$` and `DB$`, on `A:` lines and at the start of an `SVar:` value), the `Mode$` of a
trigger (`T:`) and of a static ability (`S:`), the `Event$` of a replacement effect (`R:`) and
the name of a keyword (`K:`). No parameter, description or logic is kept, and nothing is emitted
that could be a definition: the output is a list of kind names with counts of cards.

`data/census-coverage.json` says which kinds the vocabulary covers, by the `beryl_card` entry
that does (checked against vocabulary.py by the tests). A card is blocked when any of its kinds
is uncovered. The report lists the uncovered kinds by the cards that only that kind blocks, then
by the cards it blocks at all, and is written to `$BERYL_CACHE/census.json`.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card_store, forge, paths, vocabulary

COVERAGE: Path = paths.REPO / "data" / "census-coverage.json"

# The categories of kind, in the order the report lists them.
CATEGORIES: tuple[str, ...] = ("effect", "trigger", "static", "replacement", "keyword")

# A `$`-field of a script line, `Name$ value`; only the first of an ability line is read.
API: re.Pattern[str] = re.compile(r"(?:^|\|)\s*(?:AB|SP|DB)\$\s*(\w+)")
MODE: re.Pattern[str] = re.compile(r"(?:^|\|)\s*Mode\$\s*(\w+)")
EVENT: re.Pattern[str] = re.compile(r"(?:^|\|)\s*Event\$\s*(\w+)")


def kind(category: str, name: str) -> str:
    return f"{category}:{name}"


def kinds_of(script: str) -> tuple[list[str], set[str]]:
    """The card names a script gives (its faces') and the set of kinds it uses."""
    names: list[str] = []
    found: set[str] = set()
    for raw in script.splitlines():
        head, _, rest = raw.partition(":")
        if head == "Name":
            names.append(rest.strip())
        elif head == "A":
            if m := API.search(rest):
                found.add(kind("effect", m.group(1)))
        elif head == "SVar":
            # `SVar:<name>:<value>`; only a value that is itself an ability is an effect.
            _, _, value = rest.partition(":")
            if m := API.match(value):
                found.add(kind("effect", m.group(1)))
        elif head == "T":
            if m := MODE.search(rest):
                found.add(kind("trigger", m.group(1)))
        elif head == "S":
            if m := MODE.search(rest):
                found.add(kind("static", m.group(1)))
        elif head == "R":
            if m := EVENT.search(rest):
                found.add(kind("replacement", m.group(1)))
        elif head == "K":
            if word := rest.split(":", 1)[0].strip():
                found.add(kind("keyword", word))
    return names, found


def coverage(path: Path = COVERAGE) -> dict[str, str]:
    """The covered kinds: Forge's kind name to the `beryl_card` entry that covers it."""
    return cast(dict[str, str], json.loads(path.read_text())["covered"])


def stale(covered: dict[str, str], entries: Iterable[str]) -> list[str]:
    """The kinds whose entry the vocabulary no longer has."""
    known = set(entries)
    return sorted(k for k, entry in covered.items() if entry not in known)


@dataclass(frozen=True)
class Report:
    cards: int
    covered_cards: int
    counts: dict[str, int]
    blocked_by: list[
        tuple[str, int, int]
    ]  # kind, cards only it blocks, cards it blocks


def report(cards: list[set[str]], covered: dict[str, str]) -> Report:
    """Counts and blockers over the kind sets of the pool's cards."""
    counts: Counter[str] = Counter()
    blocks: Counter[str] = Counter()
    alone: Counter[str] = Counter()
    full = 0
    for used in cards:
        counts.update(used)
        missing = {k for k in used if k not in covered}
        if not missing:
            full += 1
        blocks.update(missing)
        if len(missing) == 1:
            alone.update(missing)
    order = sorted(blocks, key=lambda k: (-alone[k], -blocks[k], k))
    return Report(
        len(cards),
        full,
        dict(sorted(counts.items(), key=lambda p: (-p[1], p[0]))),
        [(k, alone[k], blocks[k]) for k in order],
    )


def pool_kinds(scripts: Path, pool: set[str]) -> list[set[str]]:
    """The kind set of each script whose card is in the pool."""
    out: list[set[str]] = []
    for path in sorted(scripts.rglob("*.txt")):
        names, found = kinds_of(path.read_text(errors="replace"))
        both = " // ".join(names)
        if both in pool or any(n in pool for n in names):
            out.append(found)
    return out


def top(r: Report, count: int) -> list[str]:
    """The `count` leading blockers as report lines."""
    return [
        f"{i}. {k}: {alone} only, {total} cards"
        for i, (k, alone, total) in enumerate(r.blocked_by[:count], start=1)
    ]


def run(args: argparse.Namespace) -> int:
    scripts = forge.scripts()
    if not scripts.is_dir():
        print(f"no Forge scripts at {scripts}; run `just oracle-setup`")
        return 1
    with card_store.Store() as store:
        pool = set(store.names("card"))
    covered = coverage()
    gone = stale(covered, (e.name for e in vocabulary.entries()))
    if gone:
        print(f"census-coverage.json names entries the vocabulary lacks: {gone}")
        return 1
    r = report(pool_kinds(scripts, pool), covered)
    out = paths.cache_root() / "census.json"
    out.write_text(
        json.dumps(
            {
                "cards": r.cards,
                "fully_covered": r.covered_cards,
                "counts": r.counts,
                "uncovered": [
                    {"kind": k, "only": alone, "cards": total}
                    for k, alone, total in r.blocked_by
                ],
            },
            indent=1,
        )
    )
    share = 100 * r.covered_cards / max(r.cards, 1)
    print(f"{r.cards} pool cards with scripts; {share:.1f}% fully covered; wrote {out}")
    print("\n".join(top(r, args.top)))
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "census",
        help="count the mechanics in Forge's scripts that the vocabulary lacks",
    )
    p.add_argument("--top", type=int, default=30, help="how many blockers to print")
    p.set_defaults(func=run)
