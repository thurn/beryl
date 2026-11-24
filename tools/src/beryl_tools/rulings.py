"""Match cached Scryfall rulings to the ledger by publication date and comment hash."""

from __future__ import annotations

import hashlib
import json
import re
from collections.abc import Iterable
from pathlib import Path
from typing import cast

from . import card_names, card_store, paths, scryfall, set_groups, system, vocabulary

Json = card_store.Json
Key = tuple[str, str]

LEDGER: Path = paths.REPO / "data" / "rulings-ledger.json"
LIBRARY: Path = paths.REPO / "engine" / "crates" / "beryl_cards" / "src" / "library"
DISPOSITIONS = {"scenario", "covered", "untestable"}
IDENTITY = {"published_at", "hash"}
SCENARIO_BUILDER: re.Pattern[str] = re.compile(
    r"\bfn\s+(\w+)\s*\(\s*name:\s*&'static str\b[^)]*\)\s*->\s*Scenario\b"
)
RUST_STRING: re.Pattern[str] = re.compile(
    r'(?:"(?:[^"\\]|\\.)*"|r(?P<hashes>#*)"(?P<raw>.*?)"(?P=hashes))', re.S
)


def key(ruling: Json) -> Key:
    comment = str(ruling["comment"]).encode("utf-8")
    return str(ruling["published_at"]), hashlib.sha256(comment).hexdigest()[:12]


def from_bulk(path: Path, oracle_ids: set[str]) -> dict[str, list[Json]]:
    """Read the bulk file once for all cards under audit, merging identical rulings."""
    found: dict[str, dict[Key, Json]] = {oracle_id: {} for oracle_id in oracle_ids}
    for ruling in scryfall.objects(path):
        oracle_id = str(ruling["oracle_id"])
        if oracle_id in found:
            found[oracle_id][key(ruling)] = ruling
    return {
        oracle_id: sorted(
            entries.values(), key=lambda ruling: str(ruling["published_at"])
        )
        for oracle_id, entries in found.items()
    }


def load(path: Path) -> dict[str, object]:
    ledger: object = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(ledger, dict):
        raise ValueError("the ledger must be an object keyed by card name")
    return cast(dict[str, object], ledger)


def indexed(entries: object) -> dict[Key, Json]:
    if not isinstance(entries, list):
        raise ValueError("expected a list of ruling entries")
    found: dict[Key, Json] = {}
    for index, entry in enumerate(entries, 1):
        if not isinstance(entry, dict):
            raise ValueError(f"entry {index} must be an object")
        fields = cast(Json, entry)
        date, digest = fields.get("published_at"), fields.get("hash")
        if (
            not isinstance(date, str)
            or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date)
            or not isinstance(digest, str)
            or not re.fullmatch(r"[0-9a-f]{12}", digest)
        ):
            raise ValueError(
                f"entry {index} needs a publication date and a 12-digit hex hash"
            )
        if (date, digest) in found:
            raise ValueError(f"{date} {digest} is listed twice")
        states = fields.keys() - IDENTITY
        if len(states) != 1 or not states <= DISPOSITIONS:
            raise ValueError(
                f"{date} {digest} needs exactly one of scenario, covered, or untestable"
            )
        found[date, digest] = fields
    return found


def describe(ruling: Json, entry: Json | None, details: bool) -> str:
    """Show the state in a brief, with the identity and evidence in the dedicated command."""
    date, digest = key(ruling)
    identity = f"{date} {digest}" if details else date
    if entry is None:
        state = "unrecorded"
    else:
        disposition = next(iter(entry.keys() - IDENTITY))
        value = entry[disposition]
        if not value:
            state = f"{disposition} (unfinished)"
        elif details:
            evidence = (
                ", ".join(str(item) for item in value)
                if isinstance(value, list)
                else value
            )
            state = f"{disposition}: {evidence}"
        else:
            state = disposition
    return f"- {identity}: {ruling['comment']} [{state}]"


def append(path: Path, name: str, found: list[Json]) -> dict[Key, Json]:
    """Record only missing rulings and keep any evidence already entered for them."""
    with system.file_lock(paths.cache_root() / "rulings-ledger.lock"):
        ledger = load(path) if path.is_file() else {}
        entries = indexed(ledger.get(name, []))
        missing = [key(ruling) for ruling in found if key(ruling) not in entries]
        if missing:
            for date, digest in missing:
                entries[date, digest] = {
                    "published_at": date,
                    "hash": digest,
                    "untestable": "",
                }
            ledger[name] = list(entries.values())
            partial = path.with_suffix(".partial")
            partial.write_text(
                json.dumps(dict(sorted(ledger.items())), indent=2, ensure_ascii=False)
                + "\n",
                encoding="utf-8",
            )
            partial.replace(path)
    return entries


def scenarios(files: Iterable[Path]) -> set[str]:
    """Find literal scenario names without building Rust, including renames and local builders."""
    found: set[str] = set()
    for path in files:
        code, skeleton = card_names.lex_c_like(
            path.read_text(encoding="utf-8"), rust=True
        )
        builders = [
            r"\bScenario::new",
            r"\.titled",
            *(r"\b" + name for name in SCENARIO_BUILDER.findall(skeleton)),
        ]
        calls = re.compile(r"(?:" + "|".join(builders) + r")\s*\(")
        for call in calls.finditer(skeleton):
            literal = RUST_STRING.match(code[call.end() :].lstrip())
            if literal is None:
                continue
            raw = literal["raw"]
            found.add(raw if raw is not None else cast(str, json.loads(literal[0])))
    return found


def problems(
    entries: dict[Key, Json],
    rulings: list[Json],
    scenario_names: set[str],
    shared_coverage: set[str],
) -> list[str]:
    """Report missing and stale entries, unfinished evidence, and invalid scenario references."""
    found: list[str] = []
    expected = {key(ruling) for ruling in rulings}
    for date, digest in sorted(expected - entries.keys()):
        found.append(f"{date} {digest} has no entry")
    for (date, digest), entry in entries.items():
        where = f"{date} {digest}"
        if (date, digest) not in expected:
            found.append(f"{where} matches no ruling")
        if "scenario" in entry:
            names = entry["scenario"]
            if (
                not isinstance(names, list)
                or not names
                or not all(isinstance(name, str) and name.strip() for name in names)
            ):
                found.append(f"{where} needs a nonempty list of scenario names")
            else:
                for name in cast(list[str], names):
                    if name not in scenario_names:
                        found.append(f"{where} names a missing scenario {name!r}")
        elif "covered" in entry:
            evidence = entry["covered"]
            if not isinstance(evidence, str) or evidence not in shared_coverage:
                found.append(
                    f"{where} names an unknown vocabulary entry or library scenario {evidence!r}"
                )
        else:
            reason = entry.get("untestable")
            if not isinstance(reason, str) or not reason.strip():
                found.append(f"{where} needs an untestable reason")
    return found


def audit(
    path: Path,
    bulk: Path,
    store_path: Path = card_store.STORE,
    cards: Path = set_groups.CARDS,
    library: Path = LIBRARY,
) -> list[str]:
    ledger = load(path)
    found: list[str] = []
    records: dict[str, Json] = {}
    with card_store.Store(store_path) as store:
        for name in ledger:
            record = store.card(name)
            if record is None or record["name"] != name:
                found.append(f"{name}: use the card's full name from the card store")
            else:
                records[name] = record
    from_scryfall = from_bulk(bulk, {str(r["oracle_id"]) for r in records.values()})
    library_names = scenarios(library.rglob("*.rs"))
    shared_coverage = library_names | {entry.name for entry in vocabulary.entries()}
    groups = set_groups.SetGroups()
    for name, record in records.items():
        try:
            entries = indexed(ledger[name])
            card_file = groups.card_path(record, cards)
            card_names = scenarios([card_file]) if card_file.is_file() else set()
            errors = problems(
                entries,
                from_scryfall[str(record["oracle_id"])],
                library_names | card_names,
                shared_coverage,
            )
        except (ValueError, LookupError) as error:
            errors = [str(error)]
        found.extend(f"{name}: {error}" for error in errors)
    return found


def step() -> int:
    bulk = scryfall.cache_dir() / "rulings.jsonl.gz"
    if not bulk.is_file():
        print(f"rulings: skipped: {bulk} is not cached")
        return 0
    try:
        found = audit(LEDGER, bulk)
    except (ValueError, OSError) as error:
        found = [str(error)]
    for error in found:
        print(f"rulings: {error}")
    return 1 if found else 0
