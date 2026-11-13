"""`card crates`: writes the set crates under `rust/cards` from `data/set-groups.json`, and the
list of them in `beryl_cards`' manifest, the one list Cargo makes someone keep.

A group of set-groups.json gets a crate when it has a card file or a card complete from the
store (store_cards.py), whose definition the crate's build script writes with no file. A crate
is three files that differ only in its name: `Cargo.toml`, a `build.rs` that calls
`beryl_card_build::set_crate` and a `lib.rs` that includes what it writes. Nobody writes one by
hand. The `card-crates` check fails when a crate is missing or differs from what this writes,
or `beryl_cards`' manifest does not list it.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from . import card_files, card_store, paths, set_groups, store_cards

REGISTRY: Path = paths.RUST / "crates" / "beryl_cards" / "Cargo.toml"
# A set crate's line among `beryl_cards`' dependencies, and among its `scenarios` feature's.
DEPENDENCY: re.Pattern[str] = re.compile(
    r'^cards_\w+ = \{ path = "\.\./\.\./cards/cards_\w+" \}\n', re.M
)
FEATURE: re.Pattern[str] = re.compile(r'^    "cards_\w+/scenarios",\n', re.M)

MANIFEST = """[package]
name = "{name}"
version = "0.1.0"
description = "The cards first printed in the sets of the group {name}"
edition.workspace = true
license.workspace = true
publish = false

[lib]
path = "lib.rs"

[lints]
workspace = true

[dependencies]
# The vocabulary alone, so that no rules edit rebuilds a card (plan, Crates), and what card
# files share, which depends on nothing else either.
beryl_card = {{ path = "../../crates/beryl_card" }}
beryl_card_shared = {{ path = "../../crates/beryl_card_shared" }}
beryl_scenario_types = {{ path = "../../crates/beryl_scenario_types", optional = true }}

[build-dependencies]
beryl_card_build = {{ path = "../../crates/beryl_card_build" }}

[features]
# The cards' scenarios, which tests and tools compile and the app never does.
scenarios = ["dep:beryl_scenario_types"]
"""

BUILD = """//! Lists the card files: `beryl_card_build` writes this crate's modules, tables and names.

fn main() {
    beryl_card_build::set_crate();
}
"""

LIB = """//! The cards first printed {years}: one file per card, `<set>/<card>.rs`, in the sets
//! `data/set-groups.json` gives this crate. A new card is a new file and nothing else: the
//! build script (`beryl_card_build`) writes the modules, `CARDS`, `SCENARIOS` and the `names`
//! the files use, and `STORE_CARDS`, the cards of these sets complete from the card store, which
//! have no file. `just card crates` writes this crate.

include!(concat!(env!("OUT_DIR"), "/cards.rs"));
"""


def years(name: str) -> str:
    """The years a crate's name gives, as its `lib.rs` says them: `cards_1993_1994` is "from
    1993 to 1994", `cards_2022` is "in 2022"."""
    parts = name.removeprefix("cards_").split("_")
    return f"in {parts[0]}" if len(parts) == 1 else f"from {parts[0]} to {parts[-1]}"


def files_of(name: str) -> dict[str, str]:
    """A crate's files by their names in its directory."""
    return {
        "Cargo.toml": MANIFEST.format(name=name),
        "build.rs": BUILD,
        "lib.rs": LIB.format(years=years(name)),
    }


def wanted(
    groups: list[set_groups.Group] | None = None,
    root: Path = set_groups.CARDS,
    store: Path = card_store.STORE,
) -> list[str]:
    """The crates there should be, in order: each group with a card file under `root` or a
    card complete from the store."""
    groups = set_groups.load() if groups is None else groups
    crates = {path.parent.parent.name for path in card_files.files(root)}
    by_set = {code: g.crate for g in groups for code in g.sets}
    with card_store.Store(store) as opened:
        for name in store_cards.names(store):
            record = opened.card(name)
            if record is not None:
                crates.add(by_set[str(record["first_set"])])
    return [g.crate for g in groups if g.crate in crates]


def registry(manifest: str, crates: list[str]) -> str:
    """`beryl_cards`' manifest with its set crates' dependencies and features as `crates`."""
    dependencies = "".join(f'{c} = {{ path = "../../cards/{c}" }}\n' for c in crates)
    features = "".join(f'    "{c}/scenarios",\n' for c in crates)
    for pattern, lines in ((DEPENDENCY, dependencies), (FEATURE, features)):
        found = list(pattern.finditer(manifest))
        if not found:
            raise ValueError(f"{REGISTRY}: no set crate lines to replace")
        start, end = found[0].start(), found[-1].end()
        manifest = manifest[:start] + lines + manifest[end:]
    return manifest


def changes(
    root: Path = set_groups.CARDS, manifest: Path = REGISTRY
) -> dict[Path, str]:
    """Each file `card crates` would write, with its text, that differs from the tree's."""
    crates = wanted(root=root)
    out: dict[Path, str] = {}
    for name in crates:
        for file, text in files_of(name).items():
            path = root / name / file
            if not path.is_file() or path.read_text() != text:
                out[path] = text
    text = manifest.read_text()
    updated = registry(text, crates)
    if updated != text:
        out[manifest] = updated
    return out


def extra(root: Path = set_groups.CARDS) -> list[Path]:
    """Set crates under `root` that no group wants: neither a file nor a store card."""
    keep = set(wanted(root=root))
    return [
        path
        for path in sorted(root.iterdir())
        if (path / "Cargo.toml").is_file() and path.name not in keep
    ]


def step(*_changed: str) -> int:
    """The `card-crates` check: every set crate written as `card crates` writes it."""
    found = [*changes(), *extra()]
    for path in found:
        rel = path.relative_to(paths.REPO).as_posix()
        print(f"card-crates: {rel} is stale; `just card crates`")
    return 1 if found else 0


def run(_args: argparse.Namespace) -> int:
    """`card crates`: writes every set crate and `beryl_cards`' list of them."""
    for path, text in changes().items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        print(path.relative_to(paths.REPO).as_posix())
    for path in extra():
        print(
            f"card crates: {path.relative_to(paths.REPO).as_posix()} has no card file "
            "and no card complete from the store; remove it",
            file=sys.stderr,
        )
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    c = commands.add_parser(
        "crates",
        help="write the set crates under rust/cards from data/set-groups.json",
    )
    c.set_defaults(func=run)


if __name__ == "__main__":
    sys.exit(step(*sys.argv[1:]))
