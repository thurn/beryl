"""Reads Beryl 3D's recordings pack, the file `record` builds and `b3d` plays, without `b3d`.

The pack is `Contents` in rust/beryl3d/crates/beryl_inspect/src/pack/mod.rs, written by
postcard: its format `VERSION`, then the entries sorted by name and seat. Each entry is its name,
its seat's index, its number of views and either its messages as a JSON array compressed with
raw deflate or why it could not be recorded. Postcard writes every unsigned integer wider than a
byte as a LEB128 varint, a byte string as its length and then its bytes, and an enum as its
variant's index.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import zlib
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from . import paths

# The pack format this reader knows, `beryl_inspect::pack::VERSION`.
VERSION = 2
PACK_ENV = "BERYL_B3D_PACK"


def record(binary: Path, *selection: str) -> Path:
    """Build the selected pack and read the path the recorder prints."""
    done = subprocess.run(
        [str(binary), *selection],
        cwd=paths.WORKSPACE,
        stdout=subprocess.PIPE,
        text=True,
        check=False,
    )
    if done.returncode:
        raise subprocess.CalledProcessError(done.returncode, done.args)
    return Path(done.stdout.strip().splitlines()[-1])


@dataclass(frozen=True)
class Entry:
    """One recording: its name, the seat it is seen from, and its messages compressed (`None`
    when it could not be recorded)."""

    name: str
    seat: int
    deflated: bytes | None

    def messages(self) -> str:
        """The recording's messages, a JSON array, or `""` when it could not be recorded."""
        if self.deflated is None:
            return ""
        # Raw deflate, with no zlib header: `miniz_oxide::deflate::compress_to_vec`.
        return zlib.decompress(self.deflated, -15).decode("utf-8", errors="replace")


class Reader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        self.at = 0

    def varint(self) -> int:
        value = 0
        shift = 0
        while True:
            byte = self.data[self.at]
            self.at += 1
            value |= (byte & 0x7F) << shift
            if byte < 0x80:
                return value
            shift += 7

    def byte(self) -> int:
        self.at += 1
        return self.data[self.at - 1]

    def bytes(self) -> bytes:
        length = self.varint()
        if self.at + length > len(self.data):
            raise IndexError("a byte string runs past the end")
        self.at += length
        return self.data[self.at - length : self.at]


def read(data: bytes) -> list[Entry]:
    """The entries of a pack's bytes. Raises `ValueError` when they are not a pack of
    [`VERSION`]."""
    reader = Reader(data)
    try:
        version = reader.varint()
        if version != VERSION:
            raise ValueError(f"a pack of version {version}, not {VERSION}")
        entries: list[Entry] = []
        for _ in range(reader.varint()):
            name = reader.bytes().decode("utf-8")
            seat = reader.byte()
            reader.varint()  # views
            made = reader.varint() == 0
            body = reader.bytes()
            entries.append(Entry(name, seat, body if made else None))
    except IndexError as error:
        raise ValueError("not a pack: it ends inside an entry") from error
    if reader.at != len(data):
        raise ValueError("not a pack: bytes follow its last entry")
    return entries


def names_in(text: str, names: Iterable[str]) -> set[str]:
    """The `names` that a JSON string of `text` spells exactly.

    A card's name holds no quote, so a JSON string that is a name is a piece of the text between
    two quotes, and splitting the text at every quote finds it. At worst the split also finds a
    name that follows an escaped quote, which only adds an input. Over the 260 MB of JSON the
    recordings hold, the split takes 3 s where matching every JSON string with a regular
    expression took 12 s."""
    return set(names) & set(text.split('"'))


# The card names each recording's messages spell, kept across packs in Beryl 3D's cache by a
# digest of the recording's compressed messages and of every card name. An edit to a card file
# names a new pack, but the edit changes only the recordings that play that card, so only those
# are read again.
SEEN_KEPT = "ignite-cards-seen.json"
# The most recordings [`SEEN_KEPT`] holds before it is cut to the present pack's.
SEEN_MOST = 20_000


def names_seen(entries: list[Entry], names: Iterable[str]) -> dict[str, set[str]]:
    """[`names_in`] of each recording's messages from both seats, by recording, reusing what
    [`SEEN_KEPT`] holds for a recording whose messages and card names it has seen before.
    """
    names = sorted(names)
    known = hashlib.blake2b("\n".join(names).encode()).digest()
    kept_file = paths.cache() / SEEN_KEPT
    try:
        kept: dict[str, list[str]] = json.loads(kept_file.read_text())
    except (OSError, ValueError):
        kept = {}
    present: dict[str, list[str]] = {}
    seen: dict[str, set[str]] = {}
    for entry in entries:
        spelled = seen.setdefault(entry.name, set())
        if entry.deflated is None:
            continue
        key = hashlib.blake2b(entry.deflated, key=known[:64]).hexdigest()
        if key not in kept:
            kept[key] = sorted(names_in(entry.messages(), names))
        present[key] = kept[key]
        spelled |= set(kept[key])
    if len(kept) > SEEN_MOST:
        kept = present
    partial = kept_file.with_name(f"{kept_file.name}.{os.getpid()}.tmp")
    partial.write_text(json.dumps(kept))
    os.replace(partial, kept_file)
    return seen


# Spec moments are derived by playing the card scenarios, so the corpus check
# reads the pack index rather than trying to predict their names from source.
CORPUS_INPUTS = [
    "rust/cards/**",
    "rust/crates/**",
    "rust/beryl3d/crates/beryl_record/**",
    "data/beryl3d/corpus.txt",
    "rust/Cargo.lock",
    "data/card-store.tsv",
    "data/set-groups.json",
    "ui/fixtures.json",
    "docs/sample-decks/**",
]


def corpus_errors(text: str, names: set[str]) -> list[str]:
    """Reject any line that does not name an entry in the current recordings pack."""
    return [
        f"data/beryl3d/corpus.txt:{line}: no recording named {name!r}"
        for line, name in enumerate(text.splitlines(), 1)
        if name not in names
    ]


def check_corpus(repo: Path) -> list[str]:
    """Check the corpus against the current recorder, including derived spec names."""
    workspace: Path = repo / "rust" / "beryl3d"
    path = repo / "data" / "beryl3d" / "corpus.txt"

    def run(command: list[str]) -> str:
        return subprocess.run(
            command, cwd=workspace, check=True, text=True, capture_output=True
        ).stdout

    try:
        text = path.read_text(encoding="utf-8")
        run(
            [
                "cargo",
                "build",
                "--locked",
                "-q",
                "--profile",
                "test",
                "-p",
                "beryl_record",
            ]
        )
        exe = "record.exe" if os.name == "nt" else "record"
        current = Path(run([str(workspace / "target" / "debug" / exe)]).strip())
        return corpus_errors(text, {entry.name for entry in read(current.read_bytes())})
    except subprocess.CalledProcessError as error:
        return [f"could not read the current recordings pack: {error.stderr.strip()}"]
    except (OSError, ValueError) as error:
        return [str(error)]
