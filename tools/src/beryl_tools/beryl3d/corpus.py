"""Record and check card spec moments one set crate at a time."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from .. import card_files, paths
from . import pack
from . import paths as b3d_paths

CRATES: Path = paths.RUST / "cards"
UPDATE = "just b3d corpus --update"
# These inputs affect every part, along with every card crate under `rust/cards`: each part is
# checked against a baseline pack recorded from the whole catalog, so a card in one crate can
# change the verdict of another crate's part.
SHARED = (
    "rust/beryl3d",
    "rust/crates",
    "rust/Cargo.toml",
    "rust/Cargo.lock",
    "data/beryl3d",
    "data/card-store.tsv",
    "data/set-groups.json",
    "data/design-tokens.json",
    "data/catalogs.json",
    "data/table-commands.json",
    "data/save-breaks.json",
    "data/ui-conventions.json",
    "docs/sample-decks",
    "ui/fixtures.json",
    "ui/src/generated",
    "tools/src/beryl_tools/beryl3d/corpus.py",
    "tools/src/beryl_tools/beryl3d/pack.py",
    "tools/src/beryl_tools/card_files.py",
)


@dataclass(frozen=True)
class Part:
    crate: str
    digest: str


@dataclass(frozen=True)
class Sweep:
    missing: list[str]
    faults: list[str]
    tests: int

    def message(self) -> str:
        lines: list[str] = []
        if self.missing:
            lines.extend(
                [
                    f"{len(self.missing)} recording(s) have signatures missing from the corpus:",
                    *self.missing[:20],
                    f"Run `{UPDATE}` to add representatives.",
                ]
            )
        if self.faults:
            lines.append(f"{len(self.faults)} interface or recording failure(s):")
            lines.extend(self.faults[:20])
        return "\n".join(lines)


def crates() -> list[str]:
    return sorted(path.parent.name for path in CRATES.glob("*/Cargo.toml"))


def digest_files(digest: str, files: Iterable[str]) -> str:
    result = hashlib.sha256(digest.encode())
    for name in files:
        path = paths.REPO / name
        result.update(os.fsencode(name) + b"\0")
        if path.is_file():
            result.update(hashlib.sha256(path.read_bytes()).digest())
        result.update(b"\0")
    return result.hexdigest()


def parts(wanted: Iterable[str]) -> list[Part]:
    """One digest per crate, shared by manual sweep checkpoints and Ignite's `next`.

    A part records its own crate's cards and checks them against a baseline pack recorded from
    every crate, so its digest covers every card crate, the shared inputs and its crate's name.
    Editing a card in any crate therefore invalidates every part, and the same inputs give the
    same digests, so a resumed sweep reuses the parts it completed."""
    names = sorted(set(wanted))
    if not names:
        return []
    unknown = set(names) - set(crates())
    if unknown:
        raise ValueError(f"unknown card crate(s): {', '.join(sorted(unknown))}")
    raw = subprocess.check_output(
        [
            "git",
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            *SHARED,
            "rust/cards",
        ],
        cwd=paths.REPO,
    )
    files = sorted({os.fsdecode(name) for name in raw.split(b"\0") if name})
    baseline = digest_files("", files)
    return [
        Part(name, hashlib.sha256(f"{baseline}\0{name}".encode()).hexdigest())
        for name in names
    ]


def inspect(
    inspector: Path, selected: Path, *arguments: str
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(inspector), "corpus", *arguments],
        env={**os.environ, pack.PACK_ENV: str(selected)},
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        check=False,
    )


def output(done: subprocess.CompletedProcess[str], codes: tuple[int, ...]) -> str:
    if done.returncode not in codes:
        raise ValueError(done.stderr.strip() or f"b3d corpus exited {done.returncode}")
    return done.stdout


def sweep(
    pending: Iterable[Part], inspector: Path, recorder: Path
) -> Iterable[tuple[Part, Sweep]]:
    """Yield each completed part, keeping its verdict for a resumed sweep of the same sources."""
    with tempfile.TemporaryDirectory(prefix="b3d-corpus-") as temp:
        baseline = Path(temp) / "signatures.bin"
        cards: dict[str, list[str]] = {}
        for part in pending:
            kept = b3d_paths.cache() / f"corpus-{part.digest}.json"
            if kept.is_file():
                yield part, Sweep(**json.loads(kept.read_text()))
                continue
            if not baseline.is_file():
                corpus_pack = pack.record(recorder)
                output(
                    inspect(inspector, corpus_pack, "--snapshot", str(baseline)), (0,)
                )
                for name, path in card_files.implemented().items():
                    cards.setdefault(path.parent.parent.name, []).append(name)
            selected = pack.record(
                recorder, "--cards", *sorted(cards.get(part.crate, []))
            )
            done = inspect(inspector, selected, "--against", str(baseline))
            body = output(done, (0, 1))
            result = Sweep(**json.loads(body))
            temporary = kept.with_suffix(f".{os.getpid()}.tmp")
            temporary.write_text(body)
            os.replace(temporary, kept)
            yield part, result


def run(arguments: list[str], inspector: Path, recorder: Path) -> int:
    parser = argparse.ArgumentParser(prog="just b3d corpus")
    parser.add_argument("--sweep", action="store_true", required=True)
    parser.add_argument("--crate", action="append", help="sweep only this card crate")
    args = parser.parse_args(arguments)
    print(
        "just b3d corpus --sweep" + "".join(f" --crate {x}" for x in args.crate or [])
    )
    failed = False
    try:
        for part, result in sweep(parts(args.crate or crates()), inspector, recorder):
            message = result.message()
            print(f"{part.crate}: {result.tests} tests", flush=True)
            if message:
                print(message, file=sys.stderr, flush=True)
                failed = True
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"corpus sweep: {error}", file=sys.stderr)
        return 1
    return int(failed)
