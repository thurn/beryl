"""The card store, data/card-store.tsv: a record for every card in the pool and every token or
emblem a pool card makes, which `by cards` writes from Scryfall's bulk data and this module
reads. Python here, Rust (the app) and Node read it, so this header is its specification.

Format. UTF-8 text, one entry per line, `<key>\\t<value>\\n`, lines sorted by the bytes of the
key and then of the value, so a reader finds a key by binary search over byte offsets (seek,
skip to the next newline, compare) without reading the whole file or keeping an index. Keys
hold no tab or newline. Two kinds of key keep a card and a token of the same name apart (a
card and its offspring token can share one):

- `card:<name>`: a card, by its name as the engine knows it, the faces joined by " // "
  ("Fire // Ice"). Card keys are unique.
- `token:<name>`: a token or emblem, by its name as the engine and the rules have it (by CR 111.4,
  "Samurai Token"). Several tokens can share a name (two printings of Treasure Token with their
  own art), so a key may span consecutive lines, each a record; a reader takes them all.

A value is a JSON object on one line: a record, or an alias, `{"alias": <name>,
"scryfall_id": <id>}`, which says that the record under the same kind's key `<name>` with that
Scryfall id answers this key too. Aliases key each face's name of a card with several faces,
and a card's oracle name where its printing has a name of its own. An alias never shadows a
card's own key.

A record:

- `name`, `oracle_id`; the printing shown: `scryfall_id`, `set`, `collector_number`, `layout`,
  `artist`, `card_back_id` (absent for a card whose back is its other face); Scryfall's
  `keywords`;
- a card's first printing, the earliest real one (`cards.first_of`), which places its file
  (set_groups.py): `first_set`, its set code, and `first_released`, its release date
  (`YYYY-MM-DD`); a token has neither;
- a card's `tokens`: each token or emblem it makes as `{"name", "scryfall_id"}`, the key and id
  of its record;
- `faces`, each with the characteristics CR 109.3 lists that a printed face carries: `name`,
  `mana_cost`, `type_line` (card types, subtypes and supertypes), `oracle_text` (rules text),
  `colors`, `color_indicator`, `power`, `toughness`, `loyalty`, `defense`, each present when
  Scryfall gives it; and `side`, which with the Scryfall id derives its images:
  `https://cards.scryfall.io/<version>/<side>/<id[0]>/<id[1]>/<id>.jpg`, version `art_crop`,
  `normal` or `large`, and the card back's from `card_back_id` likewise on
  `https://backs.scryfall.io/<version>/`.

The store keeps nothing that changes without the card changing (prices, legalities, rankings,
image timestamps), so a refresh rewrites only the lines of cards Scryfall changed.
https://yawgatog.com/resources/magic-rules/#R1093 (CR 109.3): "An object's characteristics are
name, mana cost, color, color indicator, card type, subtype, supertype, rules text, abilities,
power, toughness, loyalty, defense, hand modifier, and life modifier." Hand and life modifiers
are Vanguard's, which is not in the pool.
"""

from __future__ import annotations

import json
from collections.abc import Iterable, Iterator
from pathlib import Path
from types import TracebackType
from typing import BinaryIO, Literal, cast

from . import paths

STORE: Path = paths.REPO / "data" / "card-store.tsv"

Json = dict[str, object]
Kind = Literal["card", "token"]
KINDS: tuple[Kind, ...] = ("card", "token")


def key(kind: Kind, name: str) -> str:
    return f"{kind}:{name}"


def line(kind: Kind, name: str, value: Json) -> bytes:
    text = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    return f"{key(kind, name)}\t{text}\n".encode()


def write(path: Path, lines: Iterable[bytes]) -> None:
    """Writes entries made by `line`, sorted as readers expect."""
    with path.open("wb") as out:
        out.writelines(sorted(set(lines)))


class Store:
    """An open card store. Each lookup seeks: it costs a few reads however large the store."""

    def __init__(self, path: Path = STORE) -> None:
        self.file: BinaryIO = path.open("rb")
        self.size: int = path.stat().st_size

    def __enter__(self) -> Store:
        return self

    def __exit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        trace: TracebackType | None,
    ) -> None:
        self.file.close()

    def close(self) -> None:
        self.file.close()

    def line_from(self, offset: int) -> int:
        """The offset of the first line starting at or after `offset`."""
        if offset == 0:
            return 0
        self.file.seek(offset - 1)
        self.file.readline()
        return self.file.tell()

    def entries(self, wanted: str) -> list[Json]:
        """The values under key `wanted`, aliases unresolved."""
        target = wanted.encode()
        low, high = 0, self.size
        # The least offset whose next line's key is at or past the target.
        while low < high:
            middle = (low + high) // 2
            self.file.seek(self.line_from(middle))
            found = self.file.readline().split(b"\t", 1)[0]
            if found and found < target:
                low = middle + 1
            else:
                high = middle
        self.file.seek(self.line_from(low))
        out: list[Json] = []
        for raw in self.file:
            name, _, value = raw.partition(b"\t")
            if name != target:
                break
            out.append(cast(Json, json.loads(value)))
        return out

    def records(self, kind: Kind, name: str) -> list[Json]:
        """The records answering `name`: its own, or through aliases."""
        out: list[Json] = []
        for value in self.entries(key(kind, name)):
            if "alias" in value:
                target = self.entries(key(kind, str(value["alias"])))
                out += [r for r in target if r["scryfall_id"] == value["scryfall_id"]]
            else:
                out.append(value)
        return out

    def card(self, name: str) -> Json | None:
        """The card named `name`, or with a face of that name."""
        found = self.records("card", name)
        return found[0] if found else None

    def tokens(self, name: str) -> list[Json]:
        """The tokens and emblems named `name`."""
        return self.records("token", name)

    def made_by(self, record: Json) -> list[Json]:
        """The token and emblem records a card's record says it makes."""
        out: list[Json] = []
        for part in cast(list[Json], record.get("tokens", [])):
            out += [
                t
                for t in self.tokens(str(part["name"]))
                if t["scryfall_id"] == part["scryfall_id"]
            ]
        return out

    def cards(self, names: Iterable[str]) -> list[Json]:
        """The cards of `names`, in order; a LookupError names those the store lacks."""
        found = {n: self.card(n) for n in dict.fromkeys(names)}
        missing = [n for n, r in found.items() if r is None]
        if missing:
            raise LookupError(f"no card in the store named {missing}")
        return [cast(Json, found[n]) for n in names]

    def names(self, kind: Kind) -> list[str]:
        """Every name a lookup of `kind` answers: each record's and alias's key, once."""
        prefix = f"{kind}:".encode()
        self.file.seek(0)
        found = (raw.split(b"\t", 1)[0] for raw in self.file)
        return list(
            dict.fromkeys(
                k[len(prefix) :].decode() for k in found if k.startswith(prefix)
            )
        )

    def __iter__(self) -> Iterator[tuple[Kind, Json]]:
        """Every record, cards before tokens, each once (aliases skipped)."""
        self.file.seek(0)
        for raw in self.file:
            name, _, value = raw.partition(b"\t")
            record = cast(Json, json.loads(value))
            if "alias" not in record:
                yield cast(Kind, name.split(b":", 1)[0].decode()), record
