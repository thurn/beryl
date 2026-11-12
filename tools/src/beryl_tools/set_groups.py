"""data/set-groups.json: the crates card files go in, each a run of first-printing sets
(docs/project-50k.md, K8: "Card files are grouped by first-printing set, in many crates"), which
`by cards` writes beside the card store, and the paths a card's file belongs at.

A group is a crate name and its set codes. The first run partitions every first-printing set,
oldest first (by its earliest first printing's release date, then code), into runs of 500 to
1,500 cards (`MINIMUM`, `MAXIMUM`): an era a crate. A set of more than 1,500 cards is a group
by itself. Later runs keep every set where the file has it, so a refresh or a new set never
moves a card file to another crate: each new set, oldest first, joins the newest group while
that holds at most 1,500 cards, and opens a group of its own otherwise. A set that has no cards
any more keeps its place.

A crate is named for the years of its sets when it is made, `cards_1993_1994`, or `cards_2024`
for one year, with `_part_2`, `_part_3` and so on for a name already taken, and keeps its name
as later sets join it.

A card's file is `rust/cards/<crate>/<set module>/<card module>.rs` (`SetGroups.card_path`): a
set's module is its code (`m10`), and a card's is its name in lowercase ASCII words joined by
underscores, accents dropped (`lim_dul_s_vault`, `fire_ice`). A module name that would start
with a digit or be a Rust keyword takes a prefix spelled out: `set_10e`, `set_mod`,
`card_2_mace`.
"""

from __future__ import annotations

import json
import unicodedata
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

from . import paths

GROUPS: Path = paths.REPO / "data" / "set-groups.json"
CARDS: Path = paths.REPO / "rust" / "cards"
MINIMUM = 500
MAXIMUM = 1500
RULE = (
    "Crates of card files (`by cards`; tools/src/beryl_tools/set_groups.py specifies them): "
    "first-printing sets oldest first in runs of 500 to 1,500 cards, a larger set alone. A set "
    "never moves: a new one joins the newest crate while it holds at most 1,500 cards, else "
    "opens one. A crate is named for its sets' years when made and keeps its name."
)
# Rust's strict and reserved keywords, which name no module unescaped.
KEYWORDS: frozenset[str] = frozenset(
    "abstract as async await become box break const continue crate do dyn else enum extern "
    "false final fn for gen if impl in let loop macro match mod move mut override priv pub ref "
    "return self static struct super trait true try type typeof union unsafe unsized use "
    "virtual where while yield".split()
)

Json = dict[str, object]


@dataclass(frozen=True)
class SetInfo:
    """A first-printing set: its earliest first printing's release date and its card count."""

    released: str
    cards: int


@dataclass
class Group:
    crate: str
    sets: list[str] = field(default_factory=list)


def module(name: str, prefix: str) -> str:
    """`name` as a Rust module: itself where it is an identifier and no keyword, else
    `<prefix>_<name>`."""
    valid = name and not name[0].isdigit() and name.replace("_", "a").isalnum()
    return name if valid and name not in KEYWORDS else f"{prefix}_{name}"


def set_module(code: str) -> str:
    """A set's module: its code, lowercase letters and digits, prefixed `set_` as needed."""
    if not (code.isascii() and code.isalnum() and code == code.lower()):
        raise ValueError(f"set code {code!r} is not lowercase letters and digits")
    return module(code, "set")


def card_module(name: str) -> str:
    """A card's module, from its name: ASCII letters and digits in lowercase words joined by
    underscores, accents dropped and other characters breaking words."""
    folded = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode()
    words = "".join(c.lower() if c.isalnum() else " " for c in folded).split()
    if not words:
        raise ValueError(f"card name {name!r} has no ASCII letters or digits")
    return module("_".join(words), "card")


def first_sets(records: Iterable[Json]) -> dict[str, SetInfo]:
    """Each first-printing set of these card records, with its date and card count."""
    dates: dict[str, str] = {}
    counts: dict[str, int] = {}
    for record in records:
        code, date = str(record["first_set"]), str(record["first_released"])
        dates[code] = min(dates.get(code, date), date)
        counts[code] = counts.get(code, 0) + 1
    return {c: SetInfo(dates[c], counts[c]) for c in dates}


def penalty(total: int, single: bool) -> float:
    """How far a run of sets holding `total` cards is from a group's bounds."""
    if single and total > MAXIMUM:
        return 0.0
    if total > MAXIMUM:
        return float("inf")
    return float(max(0, MINIMUM - total))


def partition(sizes: list[int]) -> list[int]:
    """The ends of the runs splitting `sizes` in order with the least `penalty`, then the
    fewest runs."""
    best: list[tuple[float, int, int]] = [(0.0, 0, 0)]  # (penalty, runs, start)
    for end in range(1, len(sizes) + 1):
        options: list[tuple[float, int, int]] = []
        total = 0
        for start in range(end - 1, -1, -1):
            total += sizes[start]
            if total > MAXIMUM and start < end - 1:
                break
            cost, runs, _ = best[start]
            options.append((cost + penalty(total, start == end - 1), runs + 1, start))
        best.append(min(options))
    ends: list[int] = []
    end = len(sizes)
    while end:
        ends.append(end)
        end = best[end][2]
    return ends[::-1]


def crate_name(years: list[str], taken: set[str]) -> str:
    low, high = min(years), max(years)
    base = f"cards_{low}" if low == high else f"cards_{low}_{high}"
    name, part = base, 2
    while name in taken:
        name, part = f"{base}_part_{part}", part + 1
    return name


def extend(existing: list[Group], sets: Mapping[str, SetInfo]) -> list[Group]:
    """`existing` with every set of `sets` it lacks placed: partitioned when there is no group
    yet, else each joining the newest group or opening one. No placed set moves."""
    groups = [Group(g.crate, list(g.sets)) for g in existing]
    placed = {code for g in groups for code in g.sets}
    new = sorted(
        (c for c in sets if c not in placed), key=lambda c: (sets[c].released, c)
    )
    taken = {g.crate for g in groups}

    def opened(codes: list[str]) -> Group:
        group = Group(crate_name([sets[c].released[:4] for c in codes], taken), codes)
        taken.add(group.crate)
        return group

    if not groups:
        start = 0
        for end in partition([sets[c].cards for c in new]):
            groups.append(opened(new[start:end]))
            start = end
        return groups
    for code in new:
        last = groups[-1]
        held = sum(sets[c].cards for c in last.sets if c in sets)
        if held + sets[code].cards <= MAXIMUM:
            last.sets.append(code)
        else:
            groups.append(opened([code]))
    return groups


def load(path: Path = GROUPS) -> list[Group]:
    if not path.exists():
        return []
    data = cast(Json, json.loads(path.read_text()))
    return [
        Group(str(g["crate"]), [str(s) for s in cast(list[object], g["sets"])])
        for g in cast(list[Json], data["groups"])
    ]


def write(path: Path, groups: list[Group]) -> None:
    data = {
        "rule": RULE,
        "groups": [{"crate": g.crate, "sets": g.sets} for g in groups],
    }
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    if not path.exists() or path.read_text() != text:
        path.write_text(text)


def update(path: Path, records: Iterable[Json]) -> list[Group]:
    """Places the first-printing sets of these card records in the groups at `path`."""
    groups = extend(load(path), first_sets(records))
    write(path, groups)
    return groups


class SetGroups:
    """The groups of data/set-groups.json, looked up by set: what the set crates are built
    from."""

    def __init__(self, groups: list[Group] | None = None) -> None:
        self.groups: list[Group] = load() if groups is None else groups
        self.crates: dict[str, str] = {
            code: g.crate for g in self.groups for code in g.sets
        }

    def group_of(self, record: Json) -> str:
        """The crate of a card record's file: its first printing's set's group."""
        code = str(record["first_set"])
        if code not in self.crates:
            raise LookupError(f"set {code} is in no group: run `just cards`")
        return self.crates[code]

    def card_path(self, record: Json, root: Path = CARDS) -> Path:
        """Where a card record's file belongs:
        `rust/cards/<crate>/<set module>/<card module>.rs`."""
        return (
            root
            / self.group_of(record)
            / set_module(str(record["first_set"]))
            / f"{card_module(str(record['name']))}.rs"
        )
