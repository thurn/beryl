"""data/catalogs.json: Scryfall's catalogs of types and keywords, and the counter kinds the
pool's Oracle text names, which `by cards` writes beside the card store. The generated subtype
and counter lists are built from it, not written by hand (docs/project-50k.md, "Card pool
pipeline").

Scryfall serves each catalog at https://api.scryfall.com/catalog/<name>
(https://scryfall.com/docs/api/catalogs), cached here like the bulk files (scryfall.py). It has
no catalog of counter kinds, so `counter_kinds` reads them from the pool's Oracle text: the word
before "counter" or "counters" after a quantity ("a", "three", "another", "that many"), or the
two words of a keyword ability ("a first strike counter"), and "+1/+1"-style kinds; never a word
that is itself a quantity or `NOT_KINDS`. Every list is sorted, so a refresh diffs small.
"""

from __future__ import annotations

import json
import re
import time
from collections.abc import Iterable
from pathlib import Path
from typing import cast

from . import paths, scryfall

CATALOGS: Path = paths.REPO / "data" / "catalogs.json"
CATALOG = "https://api.scryfall.com/catalog/"
# The catalogs kept: the types a type line holds, and the keywords rules text holds.
NAMES: tuple[str, ...] = (
    "supertypes",
    "card-types",
    "artifact-types",
    "battle-types",
    "creature-types",
    "enchantment-types",
    "land-types",
    "planeswalker-types",
    "spell-types",
    "keyword-abilities",
    "keyword-actions",
    "ability-words",
)
# The catalogs of subtypes: what follows the dash of a type line.
SUBTYPES: tuple[str, ...] = tuple(n for n in NAMES if n.endswith("-types"))[1:]
# Scryfall asks clients to wait 50 to 100 ms between requests.
PAUSE = 0.1
QUANTITIES = (
    "a an one two three four five six seven eight nine ten x that many twice another "
    "additional no each all those"
).split()
# Words before "counter" that name no kind: "each kind of counter", "two different counters".
NOT_KINDS: frozenset[str] = frozenset(QUANTITIES) | {"of", "had", "more", "different"}
COUNTER = re.compile(
    rf"\b(?:(?:{'|'.join(QUANTITIES)}|\d+) )+([+-]\d+/[+-]\d+|[a-z]+(?: [a-z]+)?) counters?\b"
)
COUNTER_RULE = (
    "counter_kinds: from the pool's Oracle text, the word before 'counter' or 'counters' after "
    "a quantity ('a', 'three', 'that many'), two words for a keyword ability ('first strike'), "
    "or a '+1/+1'-style kind (tools/src/beryl_tools/catalogs.py)."
)

Json = dict[str, object]


def fetch(refresh: bool = False) -> dict[str, list[str]]:
    """Each catalog of `NAMES`, sorted, from the cache while it is under a week old."""
    out: dict[str, list[str]] = {}
    for name in NAMES:
        path = scryfall.cache_dir() / f"catalog-{name}.json"
        age = time.time() - path.stat().st_mtime if path.exists() else None
        if refresh or age is None or age >= scryfall.MAX_AGE:
            data = scryfall.http_get(CATALOG + name)["data"]
            path.write_text(json.dumps(data, ensure_ascii=False))
            time.sleep(PAUSE)
        out[name] = sorted(cast(list[str], json.loads(path.read_text())))
    return out


def counter_kinds(texts: Iterable[str], keywords: Iterable[str]) -> list[str]:
    """The counter kinds these Oracle texts name, sorted."""
    abilities = {k.lower() for k in keywords}
    kinds: set[str] = set()
    for text in texts:
        for match in COUNTER.finditer(text.lower()):
            kind = match.group(1)
            if " " in kind and kind not in abilities:
                kind = kind.split()[-1]
            if kind not in NOT_KINDS:
                kinds.add(kind)
    return sorted(kinds)


def write(path: Path, catalogs: dict[str, list[str]], texts: Iterable[str]) -> None:
    data = {
        "rule": f"Scryfall's catalogs ({CATALOG}<name>), sorted. {COUNTER_RULE}",
        "catalogs": catalogs,
        "counter_kinds": counter_kinds(texts, catalogs["keyword-abilities"]),
    }
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    if not path.exists() or path.read_text() != text:
        path.write_text(text)


def load(path: Path = CATALOGS) -> Json:
    return cast(Json, json.loads(path.read_text()))


def subtypes(type_line: str, known: set[str]) -> tuple[list[str], list[str]]:
    """A type line's subtypes, read by the longest catalog entry at each word ("Time Lord"),
    and the words no entry matched."""
    words = type_line.partition(" — ")[2].split()
    found: list[str] = []
    unknown: list[str] = []
    at = 0
    while at < len(words):
        for size in range(min(3, len(words) - at), 0, -1):
            if " ".join(words[at : at + size]) in known:
                found.append(" ".join(words[at : at + size]))
                at += size
                break
        else:
            unknown.append(words[at])
            at += 1
    return found, unknown
