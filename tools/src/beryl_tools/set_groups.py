"""data/set-groups.json: the crates card files go in, each the first-printing sets of a
calendar year or part of one, which `by cards` writes beside the card store and `card crates` updates,
and the paths a card's file belongs at.

A group is a crate name and its set codes. Each first-printing set goes in the crate of the
year of its earliest first printing's release date, `cards_2010`. A year whose sets hold more
than `CAP` cards is split by release date into halves, `cards_2024_h1` (January to June) and
`cards_2024_h2` (July to December), and a half over `CAP` into quarters, `cards_2026_q1` to
`cards_2026_q4`; quarters are not split further. The current year and later ones are split into
quarters from the start, since recent years always grow past `CAP`, so a set does not move when
its year fills and a year's crates do not change shape while it is not over. A split is kept
once made: a crate named in the file keeps its year or half split even when its sets shrink or
the year is past. The rest is a function of the store's
sets alone, so a set moves only when its year or half first grows past `CAP`, and `card crates`
then moves its card files. The cap keeps the one-edit rebuild in budget: it is mostly the
recheck of the edited crate, which grows with the crate's cards (K8, Measurements).

A card's file is `rust/cards/<crate>/<set module>/<card module>.rs` (`SetGroups.card_path`): a
set's module is its code (`m10`), and a card's is its name in lowercase ASCII words joined by
underscores, accents and apostrophes dropped (`lim_duls_vault`, `fire_ice`). A module name that
would start with a digit or be a Rust keyword takes a prefix spelled out: `set_10e`, `set_mod`,
`card_2_mace`. The name is the one the engine knows the card by (`file_name`): a card of two
faces is its front face (`jace_vryns_prodigy`), and a split card both halves. A card's constant
in a `names` module is its module in capitals (`constant`). The set crates' build scripts spell
the same rules in Rust (`beryl_card_build`'s names.rs) and find each file's card in the store
by them, so a file out of place fails the build.
"""

from __future__ import annotations

import json
import re
import unicodedata
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from datetime import date
from pathlib import Path
from typing import cast

from . import paths

GROUPS: Path = paths.REPO / "data" / "set-groups.json"
CARDS: Path = paths.REPO / "rust" / "cards"
CAP = 1200
RULE = (
    "Crates of card files (`by cards`, `card crates`; tools/src/beryl_tools/set_groups.py "
    "specifies them): each first-printing set in the crate of its first release's year, "
    "cards_<year>. A year of more than 1,200 cards is split into halves by release date "
    "(cards_<year>_h1, _h2), and a half of more than 1,200 into quarters (_q1 to _q4). The "
    "current year and later ones are split straight into quarters. A split, once made, is "
    "kept."
)
# A crate name the rule makes: a year, then a half or quarter of it.
CRATE: re.Pattern[str] = re.compile(r"^cards_(\d{4})(?:_([hq])([1-4]))?$")
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
    underscores, accents and apostrophes dropped and other characters breaking words."""
    folded = unicodedata.normalize("NFKD", name)
    kept = "".join(c for c in folded if c not in "'’" and not unicodedata.combining(c))
    spaced = "".join(c.lower() if c.isascii() and c.isalnum() else " " for c in kept)
    words = spaced.split()
    if not words:
        raise ValueError(f"card name {name!r} has no ASCII letters or digits")
    return module("_".join(words), "card")


def constant(name: str) -> str:
    """A card's constant in a `names` module: `JACE_VRYNS_PRODIGY`."""
    return card_module(name).upper()


def camel(name: str) -> str:
    """A card's name as an associated constant of a set crate's generated `Card` and `Printed`
    types: its module's words, each capitalized, as `JaceVrynsProdigy`."""
    return "".join(word[:1].upper() + word[1:] for word in card_module(name).split("_"))


def file_name(record: Json) -> str:
    """The name a card record's file is spelled from, the one the engine knows the card by:
    both halves of a split card ("Fire // Ice"), else its front face's."""
    if record.get("layout") == "split":
        return str(record["name"])
    return str(cast(list[Json], record["faces"])[0]["name"])


def first_sets(records: Iterable[Json]) -> dict[str, SetInfo]:
    """Each first-printing set of these card records, with its date and card count."""
    dates: dict[str, str] = {}
    counts: dict[str, int] = {}
    for record in records:
        code, date = str(record["first_set"]), str(record["first_released"])
        dates[code] = min(dates.get(code, date), date)
        counts[code] = counts.get(code, 0) + 1
    return {c: SetInfo(dates[c], counts[c]) for c in dates}


def parents(crate: str) -> list[str]:
    """The crates `crate` was split from: a half's year, a quarter's half and year. None for a
    year, or for a name the rule does not make."""
    found = CRATE.match(crate)
    if not found or not found.group(2):
        return []
    year, part, number = found.group(1), found.group(2), int(found.group(3))
    if part == "h":
        return [f"cards_{year}"]
    return [f"cards_{year}_h{(number + 1) // 2}", f"cards_{year}"]


def part_of(crate: str, released: str) -> str:
    """The part of the split crate `crate`, a year or a half, that a set released on
    `released` goes in."""
    month = int(released[5:7])
    found = CRATE.match(crate)
    if found and not found.group(2):
        return f"{crate}_h{1 if month <= 6 else 2}"
    return f"{crate.rsplit('_', 1)[0]}_q{(month - 1) // 3 + 1}"


def extend(
    existing: list[Group], sets: Mapping[str, SetInfo], today: date | None = None
) -> list[Group]:
    """The groups of `sets` by the rule, keeping each split `existing` has made. Groups are in
    date order, and each group's sets oldest first."""
    current: int = (today or date.today()).year
    split: set[str] = {parent for g in existing for parent in parents(g.crate)}
    groups: list[Group] = []

    def place(crate: str, codes: list[str], depth: int) -> None:
        # `crate` is a year at depth 0, a half at 1 and a quarter at 2.
        total = sum(sets[c].cards for c in codes)
        recent = int(crate.removeprefix("cards_")[:4]) >= current
        must = crate in split or total > CAP or recent
        if depth == 2 or not must:
            groups.append(Group(crate, codes))
            return
        parts: dict[str, list[str]] = {}
        for code in codes:
            parts.setdefault(part_of(crate, sets[code].released), []).append(code)
        for part in sorted(parts):
            place(part, parts[part], depth + 1)

    years: dict[str, list[str]] = {}
    for code in sorted(sets, key=lambda c: (sets[c].released, c)):
        years.setdefault(f"cards_{sets[code].released[:4]}", []).append(code)
    for crate in sorted(years):
        place(crate, years[crate], 0)
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
    """Groups the first-printing sets of these card records by the rule, keeping the splits
    the file at `path` has made, and writes them there."""
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
            / f"{card_module(file_name(record))}.rs"
        )
