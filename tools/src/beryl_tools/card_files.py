"""The card files under `rust/cards` and the cards they define, read as text: nothing is built.

A card's file is `rust/cards/<crate>/<set module>/<card module>.rs` (set_groups.py), and no list
of them is kept: this finds each file's card in the card store by the set and module of its
path, as the set crates' build scripts do (`beryl_card_build`), and gives the constants of
`beryl_cards::names`, which that crate's build script writes for the cards of the decklists in
`docs/sample-decks`, the basic lands and every card with a file.
"""

from __future__ import annotations

import functools
import re
import subprocess
from collections.abc import Iterable
from pathlib import Path

from . import card_store, paths, set_groups

CARDS: Path = set_groups.CARDS
DECKS: Path = paths.REPO / "docs" / "sample-decks"
BASIC_LANDS = ("Plains", "Island", "Swamp", "Mountain", "Forest")
# A card's file by its path from the repository's root: its crate, set and card modules. A
# crate's own `lib.rs` and `build.rs` sit a directory higher.
FILE: re.Pattern[str] = re.compile(r"^rust/cards/(\w+)/(\w+)/(\w+)\.rs$")
# A decklist's line: `<count> <name>`.
DECK_LINE: re.Pattern[str] = re.compile(r"^\d+ (.+?)\s*$", re.M)


def files(root: Path = CARDS) -> list[Path]:
    """Every card file under `root`, in the registry's order: crate, set, card."""
    return sorted(root.glob("*/*/*.rs"))


@functools.cache
def spelled(store: Path = card_store.STORE) -> dict[str, tuple[str, ...]]:
    """The names the store answers to, cards' and their faces', by `set_groups.card_module`."""
    found: dict[str, list[str]] = {}
    with card_store.Store(store) as opened:
        for name in opened.names("card"):
            found.setdefault(set_groups.card_module(name), []).append(name)
    return {module: tuple(names) for module, names in found.items()}


def name_of(path: Path, store: card_store.Store) -> str | None:
    """The card whose file `path` is: the one spelled as its module that was first printed in
    its directory's set; None if the store has none."""
    for name in spelled(store.path).get(path.stem, ()):
        record = store.card(name)
        if (
            record
            and set_groups.set_module(str(record["first_set"])) == path.parent.name
        ):
            return name
    return None


def implemented(root: Path = CARDS, store: Path = card_store.STORE) -> dict[str, Path]:
    """Each implemented card's file, by the card's name."""
    with card_store.Store(store) as opened:
        named = ((name_of(path, opened), path) for path in files(root))
        return {name: path for name, path in named if name is not None}


def cards_in(changed: Iterable[str], root: Path = paths.REPO) -> list[str]:
    """The cards whose files are among `changed` (paths from `root`), in order and once each.
    A deleted file and a crate's own files name no card."""
    found: list[str] = []
    with card_store.Store() as store:
        for rel in changed:
            if FILE.match(rel) and (root / rel).is_file():
                name = name_of(root / rel, store)
                if name is not None:
                    found.append(name)
    return list(dict.fromkeys(found))


def deck_names(decks: Path = DECKS) -> list[str]:
    """The cards of every decklist, main deck and sideboard, each once."""
    lists = sorted(decks.glob("*.txt"))
    return list(
        dict.fromkeys(n for p in lists for n in DECK_LINE.findall(p.read_text()))
    )


def rustfmt_step(*args: str) -> int:
    """Formats the workspace in the working directory, `rust/`, or with `--check` says what is
    not formatted: `cargo fmt --all`, then the card files, which rustfmt does not reach from a
    crate's root because a build script writes their `mod` lines."""
    cards = [str(path) for path in files(Path("cards"))]
    for command in (
        ["cargo", "fmt", "--all", *args],
        ["rustfmt", "--edition", "2024", *args, *cards],
    ):
        code = subprocess.run(command, check=False).returncode
        if code != 0:
            return code
    return 0


def constants(root: Path = CARDS, decks: Path = DECKS) -> dict[str, str]:
    """The constants of `beryl_cards::names`, each with its card's name."""
    names = [*deck_names(decks), *BASIC_LANDS, *implemented(root)]
    return {set_groups.constant(name): name for name in names}
