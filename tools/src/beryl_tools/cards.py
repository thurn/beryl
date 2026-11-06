"""`by cards`: card data from Scryfall. Looks up a card list on Scryfall's /cards/collection
endpoint, plus the tokens and emblems the cards make (each card's `all_parts`), and writes
data/cards.json: per card its printing, artist and per face the display text and image URLs.
The app downloads the images themselves on demand (rust/crates/beryl_app/src/art.rs).

The default list is the Milestone 1 main deck; `--cube` takes the whole cube CSV. Scryfall
picks each card's default printing; data/card-printings.csv pins another (name, set,
collector_number). A printing with a name of its own, such as a Through the Omenpaths card
that Arena names differently from Scryfall's oracle, takes its printed name and text, so it
is keyed by the name the engine knows. `--record FILE` saves the requests and responses,
which the tests replay.

Scryfall's API guidelines, followed here: https://scryfall.com/docs/api
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
import time
import urllib.request
from collections.abc import Callable, Iterable
from importlib import metadata
from pathlib import Path
from typing import cast

from . import paths

API = "https://api.scryfall.com/cards/collection"
# /cards/collection takes at most 75 identifiers per request.
BATCH = 75
# Scryfall asks for 50 to 100 ms between requests; take the top of that range.
INTERVAL = 0.1
DATA: Path = paths.REPO / "data"
DECK: Path = (
    paths.REPO / "docs" / "sample-decks" / "2026-10-17-PrinceJools-wu-control.txt"
)
CUBE: Path = DATA / "arena-powered-cube-2025-10-30.csv"
PRINTINGS: Path = DATA / "card-printings.csv"
OUTPUT: Path = DATA / "cards.json"
IMAGE_VERSIONS = ("art_crop", "normal", "large")
FACE_FIELDS = ("mana_cost", "type_line", "oracle_text", "power", "toughness", "loyalty")

Json = dict[str, object]
# Posts a JSON body to a URL and returns the decoded JSON response.
Post = Callable[[str, Json], Json]
# A /cards/collection card identifier: a name, an id, or a set and collector number.
Identifier = dict[str, str]


def user_agent() -> str:
    return f"ProjectBeryl/{metadata.version('beryl-tools')}"


def deck_names(path: Path) -> list[str]:
    """The main deck of a decklist: `<count> <name>` lines up to the first blank line (the
    sideboard follows it)."""
    names: list[str] = []
    for line in path.read_text().splitlines():
        if not line.strip():
            break
        names.append(line.split(" ", 1)[1].strip())
    return names


def cube_names(path: Path) -> list[str]:
    with path.open(newline="") as f:
        return [row["name"] for row in csv.DictReader(f)]


def printings(path: Path) -> dict[str, Identifier]:
    """Pinned printings, as collection identifiers keyed by card name."""
    if not path.exists():
        return {}
    with path.open(newline="") as f:
        return {
            row["name"]: {
                "set": row["set"],
                "collector_number": row["collector_number"],
            }
            for row in csv.DictReader(f)
        }


def http_post(url: str, body: Json) -> Json:
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={
            "User-Agent": user_agent(),
            "Accept": "application/json",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return cast(Json, json.load(response))


def throttled(post: Post) -> Post:
    last = 0.0

    def wrapped(url: str, body: Json) -> Json:
        nonlocal last
        wait = last + INTERVAL - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        try:
            return post(url, body)
        finally:
            last = time.monotonic()

    return wrapped


# The fields `build` reads, so recordings keep only these.
CARD_FIELDS: tuple[str, ...] = (
    "id",
    "name",
    "set",
    "collector_number",
    "layout",
    "artist",
    "colors",
    "image_uris",
    "card_faces",
    "all_parts",
    "printed_name",
    "printed_text",
    *FACE_FIELDS,
)
PART_FIELDS = ("id", "component", "type_line")


def pruned(response: Json) -> Json:
    def card(c: Json) -> Json:
        out = {k: c[k] for k in CARD_FIELDS if k in c}
        if "card_faces" in out:
            out["card_faces"] = [card(f) for f in cast(list[Json], c["card_faces"])]
        if "all_parts" in out:
            out["all_parts"] = [
                {k: p[k] for k in PART_FIELDS} for p in cast(list[Json], c["all_parts"])
            ]
        if "image_uris" in out:
            images = cast(Json, c["image_uris"])
            out["image_uris"] = {v: images[v] for v in IMAGE_VERSIONS}
        return out

    return {
        "not_found": response.get("not_found", []),
        "data": [card(c) for c in cast(list[Json], response["data"])],
    }


def recording(post: Post, log: list[Json]) -> Post:
    def wrapped(url: str, body: Json) -> Json:
        response = post(url, body)
        log.append({"url": url, "body": body, "response": pruned(response)})
        return response

    return wrapped


def replaying(path: Path) -> Post:
    """Answers each request from a recording made with `--record`."""
    log = cast(list[Json], json.loads(path.read_text()))
    answers: dict[tuple[str, str], Json] = {
        (str(e["url"]), json.dumps(e["body"], sort_keys=True)): cast(
            Json, e["response"]
        )
        for e in log
    }

    def post(url: str, body: Json) -> Json:
        key = (url, json.dumps(body, sort_keys=True))
        if key not in answers:
            raise LookupError(f"no recorded response for {url} {key[1]}")
        return answers[key]

    return post


def collection(post: Post, identifiers: list[Identifier]) -> list[Json]:
    cards: list[Json] = []
    for start in range(0, len(identifiers), BATCH):
        response = post(API, {"identifiers": identifiers[start : start + BATCH]})
        missing = cast(list[Json], response.get("not_found", []))
        if missing:
            raise LookupError(f"Scryfall found no card for {missing}")
        cards.extend(cast(list[Json], response["data"]))
    return cards


def faces(card: Json) -> list[Json]:
    """Per face its display text and images. Single-faced cards keep these on the card;
    split and adventure faces share the card's single image and, sometimes, its colors.
    A printing's own name and text, where Scryfall gives them, replace the oracle's.
    """
    raw = cast(list[Json], card.get("card_faces") or [card])
    out: list[Json] = []
    for face in raw:
        images = cast(Json, face.get("image_uris") or card["image_uris"])
        entry: Json = {"name": face.get("printed_name", face["name"])}
        for name in FACE_FIELDS:
            if name in face:
                entry[name] = face[name]
        if "printed_text" in face:
            entry["oracle_text"] = face["printed_text"]
        entry["colors"] = face.get("colors", card.get("colors", []))
        entry["images"] = {v: images[v] for v in IMAGE_VERSIONS}
        out.append(entry)
    return out


def entry(card: Json, **extra: object) -> Json:
    out = faces(card)
    printed = "printed_name" in card or any(
        "printed_name" in f for f in cast(list[Json], card.get("card_faces", []))
    )
    name = " // ".join(str(f["name"]) for f in out) if printed else card["name"]
    return {
        "name": name,
        "scryfall_id": card["id"],
        "set": card["set"],
        "collector_number": card["collector_number"],
        "layout": card["layout"],
        "artist": card.get("artist", ""),
        **extra,
        "faces": out,
    }


def token_parts(cards: Iterable[Json]) -> dict[str, list[str]]:
    """The tokens and emblems these cards make, by Scryfall id, with the cards making each.
    Scryfall lists a token as a "token" part, and an emblem as a "combo_piece" whose type is
    Emblem."""
    made: dict[str, list[str]] = {}
    for card in cards:
        for part in cast(list[Json], card.get("all_parts", [])):
            makes = part["component"] == "token" or str(part["type_line"]).startswith(
                "Emblem"
            )
            if makes and part["id"] != card["id"]:
                makers = made.setdefault(str(part["id"]), [])
                if card["name"] not in makers:
                    makers.append(str(card["name"]))
    return made


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


def build(names: list[str], pinned: dict[str, Identifier], post: Post) -> Json:
    unique = list(dict.fromkeys(names))
    # The collection endpoint finds a split card ("Fire // Ice") by its first half only.
    cards = collection(
        post, [pinned.get(n, {"name": n.split(" // ")[0]}) for n in unique]
    )
    made = token_parts(cards)
    tokens = collection(post, [{"id": i} for i in sorted(made)])
    return {
        "cards": sorted((entry(c) for c in cards), key=by_name),
        "tokens": sorted(
            (token_entry(t, sorted(made[str(t["id"])])) for t in tokens),
            key=by_name,
        ),
    }


def by_name(card: Json) -> tuple[str, str]:
    return (str(card["name"]), str(card["scryfall_id"]))


def render(data: Json) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False) + "\n"


def cards(args: argparse.Namespace) -> int:
    names = cube_names(CUBE) if args.cube else deck_names(DECK)
    log: list[Json] = []
    if args.replay:
        post = replaying(Path(args.replay))
    else:
        post = recording(throttled(http_post), log)
    output = Path(args.output)
    output.write_text(render(build(names, printings(PRINTINGS), post)))
    if args.record:
        Path(args.record).write_text(
            json.dumps(log, indent=1, ensure_ascii=False) + "\n"
        )
    print(f"cards: wrote {output}", file=sys.stderr)
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "cards", help="look up card data on Scryfall into data/cards.json"
    )
    p.add_argument(
        "--cube", action="store_true", help="the whole cube, not the M1 deck"
    )
    p.add_argument("--output", default=str(OUTPUT))
    # A replay records nothing, so --record with it would overwrite FILE with an empty log.
    source = p.add_mutually_exclusive_group()
    source.add_argument(
        "--record", metavar="FILE", help="save requests and responses here"
    )
    source.add_argument(
        "--replay", metavar="FILE", help="answer requests from a recording"
    )
    p.set_defaults(func=cards)
