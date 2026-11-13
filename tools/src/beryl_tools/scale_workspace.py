"""The scale rehearsal's scratch workspace (scale.py): the tree's own card structure, with N
clones of the real card files in place of the real cards.

The workspace at `root` is laid out as the repository is, so the build helper finds what it
reads where it reads it in the tree:

- `rust/cards/<crate>/<set>/<card>.rs`: the clones, in set crates whose `Cargo.toml`,
  `build.rs` and `lib.rs` are those `just card crates` writes (card_crates.py), with only the
  paths to the checkout's crates made absolute. Each crate's build script is the tree's,
  `beryl_card_build::set_crate`, so it writes the crate's modules, tables, `names` and
  `printed` faces from the scratch store, and checks each clone as it checks a real file.
- `rust/crates/registry`: the registry over the set crates, whose build script is
  `beryl_card_build::registry`, as `beryl_cards`' is, and whose `lib.rs` gathers the set
  crates' tables as `beryl_cards` does. Its binary, `all`, reads every card through
  `std::hint::black_box`, so a release build links every definition.
- `data/card-store.tsv`: a scratch store with a record for each clone (its original's, renamed
  and placed in its scratch set), the real records of the cards the clones' files name, and
  every token. `data/set-groups.json` gives each scratch crate its sets; `data/catalogs.json`
  and `beryl_card`'s `characteristics.rs` are copied from the tree.
- `rust/Cargo.toml`: the tree's workspace manifest, with the scratch crates as its members, so
  the lints, edition and release profile are the tree's.

A clone of card file `f` numbered `i` is `f` with its card renamed "<name> i" (each face
"<face> i" for a card with several faces): every `names::` and `printed::` constant of the
card and its faces becomes the clone's, so it keeps its abilities, faces and scenarios.

The crates follow the shipped grouping (data/set-groups.json: first-printing sets in runs of
500 to 1,500 cards): with no `--crates`, there are as many as N cards make at the tree's mean
group size, sized and split into sets as the tree's groups are, in order and cycled.
"""

from __future__ import annotations

import json
import math
import re
import shutil
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card_crates, card_files, card_store, paths, set_groups

CRATES: Path = paths.RUST / "crates"
# The registry crate's name in the workspace, and its binary's.
REGISTRY = "registry"
BINARY = "all"
# A crate of nothing that depends on the checkout's crates the cards use, which the rehearsal
# builds before the clock starts: `beryl_scenario_types` is an optional dependency, which
# Cargo will not build by name.
WARM = "warm"
# What the build helper reads under the repository's root besides the store and the groups.
COPIED = ("data/catalogs.json", "rust/crates/beryl_card/src/characteristics.rs")
# A constant a card file reads from the crate's `names` or `printed` module.
CONSTANT: re.Pattern[str] = re.compile(r"\b(names|printed)::([A-Z0-9_]+)\b")
FIRST_SET: re.Pattern[str] = re.compile(r'"first_set":"([^"]+)"')


@dataclass
class Source:
    """A real card file to clone: its path and text, its card's name and its record."""

    path: Path
    text: str
    name: str
    record: card_store.Json

    def faces(self) -> list[str]:
        return [
            str(f["name"]) for f in cast(list[card_store.Json], self.record["faces"])
        ]


def sources(
    root: Path = card_files.CARDS, store: Path = card_store.STORE
) -> tuple[list[Source], dict[Path, str]]:
    """Every card file under `root` that can be cloned, and each one left out with the reason.
    No card file reaches another's, so the only reason is a file the store has no card for.
    """
    pool: list[Source] = []
    left: dict[Path, str] = {}
    with card_store.Store(store) as opened:
        for path in card_files.files(root):
            name = card_files.name_of(path, opened)
            record = opened.card(name) if name is not None else None
            if name is None or record is None:
                left[path] = "no card of its set in the card store is spelled so"
                continue
            pool.append(Source(path, path.read_text(), name, record))
    return pool, left


def clone_name(source: Source, index: int) -> str:
    """The clone's name: each part of its original's suffixed with `index`, as the store keys
    it ("Fire 7 // Ice 7", or "Jace, Vryn's Prodigy 7" for a card keyed by its front face).
    """
    return " // ".join(f"{part} {index}" for part in source.name.split(" // "))


def renames(source: Source, index: int) -> dict[str, str]:
    """Each constant of the card and its faces, with the clone's."""
    out = {
        set_groups.constant(source.name): set_groups.constant(clone_name(source, index))
    }
    for face in source.faces():
        out[set_groups.constant(face)] = set_groups.constant(f"{face} {index}")
    return out


def clone(source: Source, index: int) -> str:
    """`source`'s file as clone number `index`: its own constants are the clone's."""
    table: dict[str, str] = renames(source, index)

    def rename(found: re.Match[str]) -> str:
        module, constant = found.group(1), found.group(2)
        return f"{module}::{table.get(constant, constant)}"

    return CONSTANT.sub(rename, source.text)


def clone_record(source: Source, index: int, set_code: str) -> card_store.Json:
    """The clone's store record: its original's, renamed and first printed in `set_code`."""
    record = json.loads(json.dumps(source.record))
    record["name"] = clone_name(source, index)
    record["set"] = record["first_set"] = set_code
    for face in cast(list[card_store.Json], record["faces"]):
        face["name"] = f"{face['name']} {index}"
    return record


@dataclass
class Crate:
    """A scratch set crate: its name, and the count of clones in each of its sets."""

    name: str
    sets: list[tuple[str, int]]

    @property
    def cards(self) -> int:
        return sum(count for _, count in self.sets)


def tree_groups(
    groups: Path = set_groups.GROUPS, store: Path = card_store.STORE
) -> list[tuple[int, int]]:
    """The tree's groups in order: how many sets and how many pool cards each holds."""
    loaded = set_groups.load(groups)
    by_set = {code: g.crate for g in loaded for code in g.sets}
    counts: Counter[str] = Counter()
    with store.open(encoding="utf-8") as lines:
        for line in lines:
            if line.startswith("card:"):
                found = FIRST_SET.search(line)
                if found and found.group(1) in by_set:
                    counts[by_set[found.group(1)]] += 1
    return [(len(g.sets), counts[g.crate]) for g in loaded if counts[g.crate]]


def shares(total: int, weights: list[int]) -> list[int]:
    """`total` split in proportion to `weights` by largest remainder: exact, none negative."""
    whole = sum(weights)
    exact = [total * w / whole for w in weights]
    out = [math.floor(e) for e in exact]
    order = sorted(range(len(weights)), key=lambda i: out[i] - exact[i])
    for i in order[: total - sum(out)]:
        out[i] += 1
    return out


def plan(cards: int, crates: int | None, groups: list[tuple[int, int]]) -> list[Crate]:
    """The scratch crates for `cards` clones: as many as the tree's mean group size makes, or
    `crates`; sized as the tree's groups are, in order and cycled, each split over as many sets
    as its group has. No crate or set is empty."""
    mean = sum(size for _, size in groups) / len(groups)
    count = crates if crates is not None else round(cards / mean)
    count = max(1, min(count, cards))
    picked = [groups[i % len(groups)] for i in range(count)]
    sizes = shares(cards, [size for _, size in picked])
    out = []
    for number, ((set_count, _), size) in enumerate(zip(picked, sizes, strict=True)):
        if size == 0:
            continue
        per_set = shares(size, [1] * max(1, min(set_count, size)))
        sets = [(f"z{number:03}{s:02}", n) for s, n in enumerate(per_set)]
        out.append(Crate(f"cards_s{number:03}", sets))
    return out


def set_crate_files(name: str) -> dict[str, str]:
    """A scratch set crate's files: the tree's, with its paths to the checkout's crates
    absolute."""
    files = card_crates.files_of(name)
    files["Cargo.toml"] = files["Cargo.toml"].replace(
        '"../../crates/', f'"{CRATES.as_posix()}/'
    )
    return files


REGISTRY_LIB = """//! The rehearsal's registry: the set crates' tables gathered as `beryl_cards` gathers them.

use beryl_card::CardDef;

pub mod names {
    include!(concat!(env!("OUT_DIR"), "/names.rs"));
}

include!(concat!(env!("OUT_DIR"), "/tables.rs"));

const fn total<T>(parts: &[&[T]]) -> usize {
    let (mut sum, mut part) = (0, 0);
    while part < parts.len() {
        sum += parts[part].len();
        part += 1;
    }
    sum
}

const fn gathered<T: Copy, const N: usize>(parts: &[&[T]], fill: T) -> [T; N] {
    let mut all = [fill; N];
    let (mut at, mut part) = (0, 0);
    while part < parts.len() {
        let mut entry = 0;
        while entry < parts[part].len() {
            all[at] = parts[part][entry];
            at += 1;
            entry += 1;
        }
        part += 1;
    }
    all
}

static ALL_CARDS: [&CardDef; total(SET_CARDS)] = gathered(SET_CARDS, SET_CARDS[0][0]);

static ALL_STORE_CARDS: [&CardDef; total(SET_STORE_CARDS)] =
    gathered(SET_STORE_CARDS, SET_CARDS[0][0]);

/// Every clone.
pub static CARDS: &[&CardDef] = &ALL_CARDS;

/// The cards complete from the scratch store: none, since every clone has a file.
pub static STORE_CARDS: &[&CardDef] = &ALL_STORE_CARDS;

/// Every clone's scenarios, by card name, gathered when first asked for.
#[cfg(feature = "scenarios")]
pub fn scenarios() -> &'static [(&'static str, &'static [beryl_scenario_types::Scenario])] {
    static GATHERED: std::sync::OnceLock<Vec<(&str, &[beryl_scenario_types::Scenario])>> =
        std::sync::OnceLock::new();
    GATHERED.get_or_init(|| SET_SCENARIOS.iter().flat_map(|part| part.iter().copied()).collect())
}
"""

REGISTRY_BUILD = """fn main() {
    beryl_card_build::registry();
}
"""

REGISTRY_MAIN = """use std::hint::black_box;

fn main() {
    let (mut cards, mut scenarios) = (0, 0);
    for card in black_box(registry::CARDS) {
        cards += black_box(card).faces.len();
    }
    #[cfg(feature = "scenarios")]
    for (_, card) in black_box(registry::scenarios()) {
        for scenario in black_box(*card) {
            scenarios += black_box(scenario).name.len();
        }
    }
    println!("{cards} {scenarios} {}", registry::STORE_CARDS.len());
}
"""


def registry_manifest(crates: list[str]) -> str:
    """The registry's manifest. `beryl_card_build::registry` requires each set crate among its
    dependencies and in its `scenarios` feature, as it does of `beryl_cards`."""
    depends = "".join(f'{c} = {{ path = "../../cards/{c}" }}\n' for c in crates)
    features = "".join(f'    "{c}/scenarios",\n' for c in crates)
    return f"""[package]
name = "{REGISTRY}"
version = "0.0.0"
edition.workspace = true
license.workspace = true
publish = false

[[bin]]
name = "{BINARY}"
path = "src/main.rs"

[dependencies]
beryl_card = {{ path = "{(CRATES / 'beryl_card').as_posix()}" }}
beryl_scenario_types = {{ path = "{(CRATES / 'beryl_scenario_types').as_posix()}", optional = true }}
{depends}
[build-dependencies]
beryl_card_build = {{ path = "{(CRATES / 'beryl_card_build').as_posix()}" }}

[features]
scenarios = [
    "dep:beryl_scenario_types",
{features}]
"""


def warm_manifest() -> str:
    depends = "".join(
        f'{c} = {{ path = "{(CRATES / c).as_posix()}" }}\n'
        for c in ("beryl_card", "beryl_card_shared", "beryl_scenario_types")
    )
    return f"""[package]
name = "{WARM}"
version = "0.0.0"
edition.workspace = true
publish = false

[dependencies]
{depends}"""


def workspace_manifest(tree: str) -> str:
    """The tree's workspace manifest with the scratch crates as its members."""
    members = f'members = ["cards/*", "crates/{REGISTRY}", "crates/{WARM}"]'
    return re.sub(r"^members = \[.*\]$", members, tree, count=1, flags=re.M)


def store_lines(pool: list[Source], store: Path = card_store.STORE) -> list[bytes]:
    """The real lines the scratch store keeps: every token, and the cards the clones' files
    name that are not the clones themselves, with the record each such alias names."""
    named = {c for s in pool for m, c in CONSTANT.findall(s.text) if m == "names"}
    spelled = card_files.spelled(store)
    wanted = {name for c in named for name in spelled.get(c.lower(), ())}
    kept: list[bytes] = []
    aliased: set[str] = set()
    with store.open("rb") as lines:
        entries = lines.readlines()
    for raw in entries:
        key, _, value = raw.partition(b"\t")
        if key.startswith(b"token:"):
            kept.append(raw)
        elif key.startswith(b"card:") and key[5:].decode() in wanted:
            kept.append(raw)
            if value.startswith(b'{"alias":'):
                aliased.add(str(json.loads(value)["alias"]))
    for raw in entries:
        key = raw.partition(b"\t")[0]
        name = key[5:].decode() if key.startswith(b"card:") else None
        if name in aliased and name not in wanted:
            kept.append(raw)
    return kept


def clear(root: Path) -> None:
    """Empties `root` but for its `target` directory."""
    root.mkdir(parents=True, exist_ok=True)
    for old in root.iterdir():
        if old.name != "target":
            shutil.rmtree(old) if old.is_dir() else old.unlink()


def generate(root: Path, plan_: list[Crate], pool: list[Source]) -> list[str]:
    """Writes the workspace at `root`, keeping its `target` directory; returns the set crates'
    names. The tree's `Cargo.lock` pins the dependencies as they are there."""
    clear(root)
    lines = store_lines(pool)
    index = 0
    for crate in plan_:
        directory = root / "rust" / "cards" / crate.name
        for file, text in set_crate_files(crate.name).items():
            directory.mkdir(parents=True, exist_ok=True)
            (directory / file).write_text(text)
        for code, count in crate.sets:
            folder = directory / set_groups.set_module(code)
            folder.mkdir()
            for _ in range(count):
                source = pool[index % len(pool)]
                name = clone_name(source, index)
                card = folder / f"{set_groups.card_module(name)}.rs"
                card.write_text(clone(source, index))
                record = clone_record(source, index, code)
                lines.append(card_store.line("card", name, record))
                # Each face's name answers too, as in the tree's store, unless it is the
                # card's own key, which an alias never shadows.
                for face in cast(list[card_store.Json], record["faces"]):
                    if face["name"] != name:
                        alias = {"alias": name, "scryfall_id": record["scryfall_id"]}
                        lines.append(card_store.line("card", str(face["name"]), alias))
                index += 1
    data = root / "data"
    data.mkdir()
    card_store.write(data / "card-store.tsv", lines)
    groups = [{"crate": c.name, "sets": [code for code, _ in c.sets]} for c in plan_]
    (data / "set-groups.json").write_text(json.dumps({"groups": groups}, indent=2))
    for copied in COPIED:
        (root / copied).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(paths.REPO / copied, root / copied)
    names = [c.name for c in plan_]
    registry = root / "rust" / "crates" / REGISTRY
    (registry / "src").mkdir(parents=True)
    (registry / "Cargo.toml").write_text(registry_manifest(names))
    (registry / "build.rs").write_text(REGISTRY_BUILD)
    (registry / "src" / "lib.rs").write_text(REGISTRY_LIB)
    (registry / "src" / "main.rs").write_text(REGISTRY_MAIN)
    warm = root / "rust" / "crates" / WARM
    (warm / "src").mkdir(parents=True)
    (warm / "Cargo.toml").write_text(warm_manifest())
    (warm / "src" / "lib.rs").write_text("")
    (root / "docs" / "sample-decks").mkdir(parents=True)
    tree = (paths.RUST / "Cargo.toml").read_text()
    (root / "rust" / "Cargo.toml").write_text(workspace_manifest(tree))
    shutil.copy(paths.RUST / "Cargo.lock", root / "rust" / "Cargo.lock")
    return names


def edit_card(root: Path, crates: list[str]) -> Path:
    """Changes the code of the first clone's file of the first crate, as an edit would: a new
    item, so the compiler has more than a comment to read again. Returns the file."""
    path = sorted((root / "rust" / "cards" / crates[0]).glob("*/*.rs"))[0]
    path.write_text(path.read_text() + f"\nconst _: u64 = {time.time_ns()};\n")
    return path
