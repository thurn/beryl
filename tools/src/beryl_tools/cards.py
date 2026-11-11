"""`by cards`: the card pool from Scryfall's bulk data. Reads `default_cards`, every printing
of every card (scryfall.py downloads it into the cache and reuses it for a week, or until
`--refresh`), in one pass with no per-card request, and writes:

- data/card-store.tsv, the card store (card_store.py specifies it): a record for every card in
  the pool and every token or emblem a pool card makes (its `all_parts`).
- data/cards.json, which the UI still bundles: for the main decks of the mirror and of the
  Vintage deck before it, and the basic lands, each card's printing, artist and per face the
  display text and image URLs, the tokens they make, and the image of Magic's card back. The
  app downloads the images themselves on demand (rust/crates/beryl_app/src/art.rs).

The pool is every card in Scryfall's Oracle data except un-set, dexterity and Alchemy cards: a
card whose Vintage legality is anything but not_legal (which drops un-cards, Alchemy and other
digital-only cards, tokens, art cards and oversized cards in one test), less the cards
data/card-pool-exclusions.json names with its reason. A run prints how many it kept and how
many it dropped for each reason.

Each card shows one printing. data/card-printings.csv pins some (name, set, collector_number).
Any other card shows its original art, the illustration of its first printing, in the newest
printing of that art that looks like an ordinary card of it: a reprint's scan or render is
cleaner than the early scans, and its text closer to the oracle's (Alpha's Counterspell, an
"Interrupt" dark with age, takes Dominaria Remastered's retro-frame reprint). The basic lands
are John Avon's full-art lands from Unstable. An English printing with a name of its own, such
as a Through the Omenpaths card that Arena names differently from Scryfall's oracle, takes its
printed name and text, so it is keyed by the name the engine knows.

`--sample FILE NAME...` writes the printings data/cards.json reads, and every printing of each
NAME, in the bulk file's shape: the tests' fixture.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from collections import Counter, defaultdict
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

from . import card_store, paths, scryfall

# Scryfall's card back images: /<size>/<id[0]>/<id[1]>/<card_back_id>.jpg.
BACKS = "https://backs.scryfall.io/large"
DATA: Path = paths.REPO / "data"
# The Milestone 1 mirror deck.
DECK: Path = (
    paths.REPO / "docs" / "sample-decks" / "2025-11-18-17lands-cb8d5d5f-wu-control.txt"
)
# The mirror before it, whose implemented cards the UI fixtures still play.
VINTAGE_DECK: Path = (
    paths.REPO / "docs" / "sample-decks" / "2026-10-17-PrinceJools-wu-control.txt"
)
# The basic lands, which any deck may play.
BASICS = ("Plains", "Island", "Swamp", "Mountain", "Forest")
# The basics' printings: John Avon's full-art lands from Unstable.
BASIC_PRINTINGS: dict[str, tuple[str, str]] = {
    "Plains": ("ust", "212"),
    "Island": ("ust", "213"),
    "Swamp": ("ust", "214"),
    "Mountain": ("ust", "215"),
    "Forest": ("ust", "216"),
}
# Kinds of sets whose printings are not a card's first real one: collectors' and joke sets,
# tokens, and Arena's rebalanced cards.
NOT_FIRST: frozenset[str] = frozenset(
    {"memorabilia", "funny", "token", "minigame", "alchemy", "treasure_chest"}
)
# Nor do a card's printings in promo sets show its art as an ordinary card would: they carry
# foil glare, watermarks and stamps (a judge gift, a Friday Night Magic promo); nor do the
# all-foil sets', whose scans catch the glare.
NOT_REPRINT: frozenset[str] = NOT_FIRST | {"promo", "from_the_vault", "premium_deck"}
# The List's reprints, each stamped with a planeswalker symbol.
STAMPED_SETS: frozenset[str] = frozenset({"plst", "ulst"})
# Promos stamped or marked as other than the card.
STAMPED_PROMOS: frozenset[str] = frozenset(
    {
        "prerelease",
        "datestamped",
        "stamped",
        "promopack",
        "playtest",
        "thick",
        "serialized",
    }
)
# Frame treatments that change a card's look from its ordinary frame.
TREATMENTS: frozenset[str] = frozenset(
    {"showcase", "extendedart", "etched", "inverted", "shatteredglass", "fullart"}
)
PRINTINGS: Path = DATA / "card-printings.csv"
EXCLUSIONS: Path = DATA / "card-pool-exclusions.json"
OUTPUT: Path = DATA / "cards.json"
IMAGE_VERSIONS = ("art_crop", "normal", "large")
FACE_FIELDS = ("mana_cost", "type_line", "oracle_text", "power", "toughness", "loyalty")
# The characteristics a store record's face keeps (CR 109.3), less name, colors and color
# indicator, which follow.
STORE_FACE_FIELDS: tuple[str, ...] = (*FACE_FIELDS, "defense")
NOT_LEGAL = "not legal in Vintage"

# Prefer the Double Masters Copy illustration over the generic token Scryfall links.
TOKEN_PRINTINGS = {
    "2115c141-f2fb-4bfd-a8b5-2943bad9f418": "6870a2af-80e1-4813-a332-2b1bd789f1c1",
}

Json = dict[str, object]
# A pinned printing: set code and collector number.
Printing = tuple[str, str]


def deck_names(path: Path) -> list[str]:
    """The main deck of a decklist: `<count> <name>` lines up to the first blank line (the
    sideboard follows it)."""
    names: list[str] = []
    for line in path.read_text().splitlines():
        if not line.strip():
            break
        names.append(line.split(" ", 1)[1].strip())
    return names


def m1_names() -> list[str]:
    """The cards of the Vintage and mirror decks and the basic lands, each once."""
    return list(
        dict.fromkeys(deck_names(VINTAGE_DECK) + deck_names(DECK) + list(BASICS))
    )


def listed(path: Path = card_store.STORE) -> tuple[list[Json], list[Json]]:
    """The store's records for `m1_names`' cards, each once, and for the tokens and emblems
    they make: what data/cards.json holds, which the tools read from the store instead.
    """
    with card_store.Store(path) as store:
        found = {str(r["scryfall_id"]): r for r in store.cards(m1_names())}
        tokens = {
            str(t["scryfall_id"]): t for r in found.values() for t in store.made_by(r)
        }
    return list(found.values()), list(tokens.values())


def printings(path: Path) -> dict[str, Printing]:
    """Pinned printings by card name, the basics' among them."""
    pins = dict(BASIC_PRINTINGS)
    if path.exists():
        with path.open(newline="") as f:
            for row in csv.DictReader(f):
                pins[row["name"]] = (row["set"], row["collector_number"])
    return pins


def exclusions(path: Path) -> dict[str, str]:
    """Cards out of the pool though legal in Vintage, by name, with the reason."""
    return cast(dict[str, str], json.loads(path.read_text()))


def first_of(found: list[Json]) -> Json:
    """A card's first printing, which has its original art, of its printings oldest first: the
    earliest English one sold in paper that is no promo, oversized card or printing of the
    kinds `NOT_FIRST` names; failing that, the earliest in paper (a card printed only in joke
    sets), else the earliest at all (one printed only on Arena)."""
    paper = [
        p
        for p in found
        if not p.get("digital")
        and not p.get("promo")
        and not p.get("oversized")
        and p.get("lang", "en") == "en"
    ]
    real = [p for p in paper if p.get("set_type") not in NOT_FIRST]
    return (real or paper or found)[0]


def shows_original_art(p: Json, first: Json) -> bool:
    """Whether printing `p` shows `first`'s illustration as an ordinary card of it would: in
    English, with the same border and a frame no older (never a modern card in a retro frame,
    nor the Future Sight frame), at full size, and with no treatment, stamp or alternate
    look. It is sold in paper, or else only on Magic Online, whose renders are clean: Scryfall
    renders a digital card Arena has as Arena draws it, dropping the flavor text and changing
    the type. It has a high-resolution image."""
    illustration = first.get("illustration_id")
    games = cast(list[str], p.get("games", []))
    frame = str(p.get("frame", ""))
    return (
        illustration is not None
        and p.get("illustration_id") == illustration
        and p.get("lang") == "en"
        and p.get("border_color") == first.get("border_color")
        # The frames are named by the year they began: "1993", "1997", "2003", "2015".
        and frame != "future"
        and frame >= str(first.get("frame", ""))
        and not p.get("oversized")
        and not p.get("full_art")
        and not p.get("textless")
        and not TREATMENTS & set(cast(list[str], p.get("frame_effects", [])))
        and not STAMPED_PROMOS & set(cast(list[str], p.get("promo_types", [])))
        and p.get("set_type") not in NOT_REPRINT
        and p.get("set") not in STAMPED_SETS
        and ("paper" in games or games == ["mtgo"])
        and p.get("image_status") == "highres_scan"
    )


def released(p: Json) -> tuple[str, str, int, str]:
    """Orders printings oldest first, as Scryfall's searches by release date do."""
    number = str(p["collector_number"])
    digits = "".join(c for c in number if c.isdigit())
    return (str(p.get("released_at", "")), str(p["set"]), int(digits or 0), number)


def choose(found: list[Json]) -> Json:
    """The printing a card shows, of its printings oldest first: the newest that
    `shows_original_art`, else its first (`first_of`)."""
    first = first_of(found)
    matching = [p for p in found if shows_original_art(p, first)]
    return matching[-1] if matching else first


def oracle_id(card: Json) -> str:
    """A card's oracle id; a reversible card's faces each carry their own, and the first
    names the card."""
    faces = cast(list[Json], card.get("card_faces", [{}]))
    return str(card.get("oracle_id") or faces[0].get("oracle_id"))


def drop_reason(card: Json, excluded: dict[str, str]) -> str | None:
    """Why a card is out of the pool, or None for a card in it."""
    legalities = cast(Json, card.get("legalities", {}))
    if legalities.get("vintage", "not_legal") == "not_legal":
        return NOT_LEGAL
    return excluded.get(str(card["name"]))


# The fields the pool, the printing rule, the records and data/cards.json read, so a run keeps
# only these of each printing, and so does the tests' sample.
BULK_FIELDS: tuple[str, ...] = (
    "id",
    "oracle_id",
    "name",
    "lang",
    "released_at",
    "set",
    "set_type",
    "collector_number",
    "layout",
    "artist",
    "digital",
    "promo",
    "promo_types",
    "oversized",
    "full_art",
    "textless",
    "frame",
    "frame_effects",
    "border_color",
    "illustration_id",
    "games",
    "image_status",
    "image_uris",
    "card_back_id",
    "colors",
    "color_indicator",
    "keywords",
    "legalities",
    "all_parts",
    "card_faces",
    "printed_name",
    "printed_text",
    *STORE_FACE_FIELDS,
)
FACE_KEYS: tuple[str, ...] = (
    "name",
    "oracle_id",
    "printed_name",
    "printed_text",
    "colors",
    "color_indicator",
    "image_uris",
    *STORE_FACE_FIELDS,
)
PART_FIELDS = ("id", "component", "type_line")


def pruned(card: Json) -> Json:
    out = {k: card[k] for k in BULK_FIELDS if k in card}
    if "legalities" in out:
        out["legalities"] = {"vintage": cast(Json, card["legalities"]).get("vintage")}
    if "image_uris" in out:
        images = cast(Json, card["image_uris"])
        out["image_uris"] = {v: images[v] for v in IMAGE_VERSIONS}
    if "card_faces" in out:
        out["card_faces"] = [
            pruned_face(f) for f in cast(list[Json], card["card_faces"])
        ]
    if "all_parts" in out:
        # A token lists every card making it; only the parts naming tokens are read.
        parts = [p for p in cast(list[Json], card["all_parts"]) if makes(p)]
        out["all_parts"] = [{k: p[k] for k in PART_FIELDS if k in p} for p in parts]
    return out


def pruned_face(face: Json) -> Json:
    out = {k: face[k] for k in FACE_KEYS if k in face}
    if "image_uris" in out:
        images = cast(Json, face["image_uris"])
        out["image_uris"] = {v: images[v] for v in IMAGE_VERSIONS}
    return out


@dataclass
class Pool:
    """The pool read from a bulk file: each card's printing, by oracle id, and every printing
    by Scryfall id (tokens are found there)."""

    shown: dict[str, Json] = field(default_factory=dict)
    by_id: dict[str, Json] = field(default_factory=dict)
    # Cards (oracle ids) out of the pool, per reason.
    dropped: Counter[str] = field(default_factory=Counter)
    # Cards out of the pool for not being legal in Vintage, per set type.
    not_legal: Counter[str] = field(default_factory=Counter)


def read_pool(
    printings: Iterable[Json], pins: dict[str, Printing], excluded: dict[str, str]
) -> Pool:
    pool = Pool()
    groups: defaultdict[str, list[Json]] = defaultdict(list)
    out: dict[str, tuple[str, str]] = {}
    pinned = {printing: name for name, printing in pins.items()}
    chosen: dict[str, Json] = {}
    for raw in printings:
        card = pruned(raw)
        pool.by_id[str(card["id"])] = card
        oracle = oracle_id(card)
        reason = drop_reason(card, excluded)
        if reason is not None:
            out.setdefault(oracle, (reason, str(card.get("set_type"))))
            continue
        groups[oracle].append(card)
        if (str(card["set"]), str(card["collector_number"])) in pinned:
            if oracle in chosen:
                raise LookupError(f"two pinned printings of {card['name']}")
            chosen[oracle] = card
    for oracle, (reason, set_type) in out.items():
        if oracle not in groups:
            pool.dropped[reason] += 1
            if reason == NOT_LEGAL:
                pool.not_legal[set_type] += 1
    for oracle, found in sorted(groups.items()):
        pool.shown[oracle] = chosen.get(oracle) or choose(sorted(found, key=released))
    missing = set(pinned) - {
        (str(c["set"]), str(c["collector_number"])) for c in chosen.values()
    }
    if missing:
        raise LookupError(
            f"no pool card has the pinned printings of {[pinned[m] for m in missing]}"
        )
    return pool


def is_printed(card: Json) -> bool:
    """Whether a printing's own name and text, where Scryfall gives them, replace the
    oracle's: an English printing named otherwise (a non-English printing's are a
    translation)."""
    return card.get("lang", "en") == "en"


def raw_faces(card: Json) -> list[Json]:
    return cast(list[Json], card.get("card_faces") or [card])


def faces(card: Json) -> list[Json]:
    """Per face its display text and images. Single-faced cards keep these on the card;
    split and adventure faces share the card's single image and, sometimes, its colors.
    A printing's own name and text, where Scryfall gives them, replace the oracle's.
    """
    printed = is_printed(card)
    out: list[Json] = []
    for face in raw_faces(card):
        images = cast(Json, face.get("image_uris") or card["image_uris"])
        entry: Json = {"name": face_name(face, printed)}
        for name in FACE_FIELDS:
            if name in face:
                entry[name] = face[name]
        if printed and "printed_text" in face:
            entry["oracle_text"] = face["printed_text"]
        entry["colors"] = face.get("colors", card.get("colors", []))
        entry["images"] = {v: images[v] for v in IMAGE_VERSIONS}
        out.append(entry)
    return out


def face_name(face: Json, printed: bool) -> str:
    return str(face.get("printed_name", face["name"]) if printed else face["name"])


def has_own_name(card: Json) -> bool:
    return is_printed(card) and any("printed_name" in f for f in raw_faces(card))


def display_name(card: Json) -> str:
    """The card's name as the engine knows it: its printed name where it has one, else
    Scryfall's."""
    if has_own_name(card):
        return " // ".join(face_name(f, True) for f in raw_faces(card))
    return str(card["name"])


def entry(card: Json, **extra: object) -> Json:
    return {
        "name": display_name(card),
        "scryfall_id": card["id"],
        "set": card["set"],
        "collector_number": card["collector_number"],
        "layout": card["layout"],
        "artist": card.get("artist", ""),
        **extra,
        "faces": faces(card),
    }


def makes(part: Json) -> bool:
    """Whether a related card is a token or emblem. Scryfall lists a token as a "token" part,
    and an emblem as a "combo_piece" whose type is Emblem."""
    return part["component"] == "token" or str(part["type_line"]).startswith("Emblem")


def made(card: Json) -> list[str]:
    """The Scryfall ids of the tokens and emblems a card makes."""
    out: list[str] = []
    for part in cast(list[Json], card.get("all_parts", [])):
        if makes(part) and part["id"] != card["id"]:
            linked = str(part["id"])
            out.append(TOKEN_PRINTINGS.get(linked, linked))
    return list(dict.fromkeys(out))


def token_parts(cards: Iterable[Json]) -> dict[str, list[str]]:
    """The tokens and emblems these cards make, by Scryfall id, with the cards making each."""
    makers: dict[str, list[str]] = {}
    for card in cards:
        for token in made(card):
            names = makers.setdefault(token, [])
            if card["name"] not in names:
                names.append(str(card["name"]))
    return makers


def engine_name(name: str, type_line: str) -> str:
    """A token's name as the engine and the rules have it: a token whose name is its subtypes
    is named for them "plus the word 'Token'" (CR 111.4), which Scryfall leaves off ("Samurai"
    for a Samurai Token). A token with a name of its own, or none, keeps it."""
    _, dash, subtypes = type_line.partition(" — ")
    if type_line.startswith("Token") and dash and subtypes == name:
        return f"{name} Token"
    return name


def token_entry(token: Json, made_by: list[str]) -> Json:
    out = entry(token, made_by=made_by)
    for face in cast(list[Json], out["faces"]):
        face["name"] = engine_name(str(face["name"]), str(face["type_line"]))
    if len(cast(list[Json], out["faces"])) == 1:
        out["name"] = cast(list[Json], out["faces"])[0]["name"]
    return out


def card_back(cards: Iterable[Json]) -> str:
    """The image of the back most of these cards have: Magic's own, which a few cards replace
    (a double-faced card's back is its other face)."""
    backs = Counter(str(c["card_back_id"]) for c in cards if "card_back_id" in c)
    if not backs:
        raise LookupError("Scryfall gave no card back")
    back = backs.most_common(1)[0][0]
    return f"{BACKS}/{back[0]}/{back[1]}/{back}.jpg"


def store_faces(card: Json, token: bool) -> list[Json]:
    """A record's faces: each one's characteristics (CR 109.3) and the side its image is on."""
    printed = is_printed(card)
    out: list[Json] = []
    for index, face in enumerate(raw_faces(card)):
        name = face_name(face, printed)
        record: Json = {
            "name": engine_name(name, str(face.get("type_line", ""))) if token else name
        }
        for key in STORE_FACE_FIELDS:
            if key in face:
                record[key] = face[key]
        if printed and "printed_text" in face:
            record["oracle_text"] = face["printed_text"]
        record["colors"] = face.get("colors", card.get("colors", []))
        if "color_indicator" in face:
            record["color_indicator"] = face["color_indicator"]
        # A double-faced card's back face has an image of its own; other faces share the
        # card's.
        own = index > 0 and "image_uris" in face
        record["side"] = "back" if own else "front"
        out.append(record)
    return out


def store_record(card: Json, token: bool, tokens: list[Json] | None = None) -> Json:
    faces = store_faces(card, token)
    name = display_name(card)
    if token and len(faces) == 1:
        name = str(faces[0]["name"])
    record: Json = {
        "name": name,
        "oracle_id": oracle_id(card),
        "scryfall_id": card["id"],
        "set": card["set"],
        "collector_number": card["collector_number"],
        "layout": card["layout"],
        "artist": card.get("artist", ""),
    }
    if "card_back_id" in card:
        record["card_back_id"] = card["card_back_id"]
    record["keywords"] = card.get("keywords", [])
    if tokens:
        record["tokens"] = tokens
    record["faces"] = faces
    return record


def aliases(card: Json, record: Json) -> list[str]:
    """The other names a record answers to: each face's, and the oracle's where the printing
    has a name of its own."""
    names = [str(f["name"]) for f in cast(list[Json], record["faces"])]
    if has_own_name(card):
        names += [str(card["name"]), *(str(f["name"]) for f in raw_faces(card))]
    return [n for n in dict.fromkeys(names) if n != record["name"]]


@dataclass
class Built:
    """The store's lines, each card's printing by every name it answers to, and counts."""

    lines: list[bytes] = field(default_factory=list)
    by_name: dict[str, Json] = field(default_factory=dict)
    counts: Counter[str] = field(default_factory=Counter)


def build_store(pool: Pool) -> Built:
    built = Built()
    tokens: dict[str, Json] = {}
    named: dict[str, list[tuple[Json, Json]]] = {}
    for card in pool.shown.values():
        links: list[Json] = []
        for linked in made(card):
            if linked not in pool.by_id:
                built.counts["token links to no printing"] += 1
                continue
            token = tokens.setdefault(
                linked, store_record(pool.by_id[linked], token=True)
            )
            links.append({"name": token["name"], "scryfall_id": linked})
        record = store_record(card, token=False, tokens=links)
        named.setdefault(str(record["name"]), []).append((card, record))
    for name, found in named.items():
        if len(found) > 1:
            raise LookupError(f"two cards named {name}")
        card, record = found[0]
        built.lines.append(card_store.line("card", name, record))
        built.by_name[name] = card
    for name, found in named.items():
        card, record = found[0]
        for alias in aliases(card, record):
            if alias in named:
                built.counts["aliases shadowing a card"] += 1
                continue
            value: Json = {"alias": name, "scryfall_id": record["scryfall_id"]}
            built.lines.append(card_store.line("card", alias, value))
            built.by_name.setdefault(alias, card)
    for linked, record in tokens.items():
        name = str(record["name"])
        built.lines.append(card_store.line("token", name, record))
        for alias in aliases(pool.by_id[linked], record):
            token_alias: Json = {"alias": name, "scryfall_id": linked}
            built.lines.append(card_store.line("token", alias, token_alias))
    built.counts["cards"] = len(named)
    built.counts["tokens"] = len(tokens)
    return built


def build_cards_json(names: list[str], built: Built, pool: Pool) -> Json:
    """data/cards.json for `names`: each card's printing, as a deck names it (its own name
    or its front face's), and the tokens and emblems they make."""
    missing = [n for n in names if n not in built.by_name]
    if missing:
        raise LookupError(f"no card in the pool named {missing}")
    cards = list({id(c): c for c in (built.by_name[n] for n in names)}.values())
    makers = token_parts(cards)
    return {
        "card_back": card_back(cards),
        "cards": sorted((entry(c) for c in cards), key=by_name),
        "tokens": sorted(
            (token_entry(pool.by_id[t], sorted(makers[t])) for t in makers),
            key=by_name,
        ),
    }


def by_name(card: Json) -> tuple[str, str]:
    return (str(card["name"]), str(card["scryfall_id"]))


def render(data: Json) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False) + "\n"


def sample(path: Path, bulk: Path, built: Built, pool: Pool, names: list[str]) -> None:
    """Writes the tests' fixture: the printings data/cards.json reads, the pinned ones, and
    every printing of each of `names`, pruned, one per line."""
    pinned = [n for n in printings(PRINTINGS) if n in built.by_name]
    shown = [built.by_name[n] for n in dict.fromkeys(m1_names() + pinned)]
    ids = {str(c["id"]) for c in shown} | set(token_parts(shown))
    wanted = set(names)
    with path.open("w", encoding="utf-8", newline="\n") as out:
        for raw in scryfall.objects(bulk):
            if raw["id"] in ids or raw["name"] in wanted:
                out.write(json.dumps(pruned(raw), ensure_ascii=False) + "\n")


def report(pool: Pool, built: Built) -> None:
    def say(text: str) -> None:
        print(f"cards: {text}", file=sys.stderr)

    say(f"kept {built.counts['cards']} cards and {built.counts['tokens']} tokens")
    for reason, count in sorted(pool.dropped.items()):
        say(f"dropped {count} cards: {reason}")
    detail = ", ".join(f"{t} {n}" for t, n in pool.not_legal.most_common())
    say(f"  {NOT_LEGAL}, by set type: {detail}")
    for what in ("aliases shadowing a card", "token links to no printing"):
        if built.counts[what]:
            say(f"{what}: {built.counts[what]}")


def cards(args: argparse.Namespace) -> int:
    bulk = (
        Path(args.bulk)
        if args.bulk
        else scryfall.bulk_file("default_cards", args.refresh)
    )
    pool = read_pool(
        scryfall.objects(bulk), printings(PRINTINGS), exclusions(EXCLUSIONS)
    )
    built = build_store(pool)
    report(pool, built)
    if args.sample:
        sample(Path(args.sample), bulk, built, pool, args.names)
        print(f"cards: wrote {args.sample}", file=sys.stderr)
        return 0
    card_store.write(Path(args.store), built.lines)
    output = Path(args.output)
    output.write_text(render(build_cards_json(m1_names(), built, pool)), newline="\n")
    print(f"cards: wrote {args.store} and {output}", file=sys.stderr)
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "cards",
        help="build the card store and data/cards.json from Scryfall's bulk data",
    )
    p.add_argument("--output", default=str(OUTPUT))
    p.add_argument("--store", default=str(card_store.STORE))
    source = p.add_mutually_exclusive_group()
    source.add_argument(
        "--refresh", action="store_true", help="download a new bulk file"
    )
    source.add_argument("--bulk", metavar="FILE", help="read this bulk file instead")
    p.add_argument(
        "--sample",
        metavar="FILE",
        help="write the tests' fixture instead: data/cards.json's printings and NAMES'",
    )
    p.add_argument("names", nargs="*", metavar="NAME")
    p.set_defaults(func=cards)
