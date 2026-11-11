"""The scale rehearsal (docs/project-50k.md, Measurements): how the compiler copes with many cards.

`scale` generates a scratch Cargo workspace under `$BERYL_CACHE/scale` (never in the tree),
builds it and prints what it cost: a cold `cargo check`, a release build, a rebuild after
editing one card, and the largest compiler process's peak memory.

The workload is real cards. Each of the N definitions is a clone of one of the card files under
`rust/crates/beryl_cards/src/<color>/`, cycled until N is reached, with its own card name made
unique, so it carries its abilities, bodies and scenarios, not a stub. The clones are split over
K library crates (`shard0` ...), and a final binary, `all`, references every card and scenario
through `std::hint::black_box`, since the linker would strip tables nothing reads and the
release numbers would mean nothing. The crates depend on the checkout's own, by path; the engine
crates are built once in each profile before the clock starts, so only the clones are timed.

A clone keeps its `use crate::...` lines working: each shard re-exports the real card modules
(`crate::tokens`) and has a `names` module of its own, the real names
plus one constant per clone. Files reaching another card (`super::`, `crate::white::...`) are left out of the pool.

Budgets are not asserted yet (milestone 7 adds `--assert`: check 60 s, release 180 s, one-card
rebuild 2 s, 4 GiB). Times are load-adjusted as the checks' are (`checks.Clock`). The one-card
rebuild is a `cargo check` one, the edit-and-look loop.
"""

from __future__ import annotations

import argparse
import math
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

from . import checks, paths

DEFAULT_CARDS = 50_000
DEFAULT_CRATES = 20
BERYL_CARDS: Path = paths.RUST / "crates" / "beryl_cards" / "src"
CRATES: Path = paths.RUST / "crates"
# The modules a clone reaches by `crate::`, which a shard re-exports from `beryl_cards`.
REEXPORTED = ("tokens",)
# What a file may reach in its own crate; others, such as another card's private helper, can't
# be cloned.
CRATE_PATH: re.Pattern[str] = re.compile(r"\b(?:crate|super)::(\w+)")
# A card's own name: the constant its `CardDef` gives as `name`.
NAME_FIELD: re.Pattern[str] = re.compile(
    r"^\s*name: names::([A-Z0-9_]+),", re.MULTILINE
)
NAME_CONST: re.Pattern[str] = re.compile(r'pub const ([A-Z0-9_]+): &str = "(.*)";')
MAX_LINE = 100


@dataclass
class Source:
    """A card file to clone: its text, the constant naming it and that constant's string."""

    path: Path
    text: str
    const: str
    name: str


def sources(src: Path = BERYL_CARDS) -> list[Source]:
    """The real card files that can be cloned: those with a `CardDef` named by `names::...`
    that reach nothing in their crate but `names` and `tokens`."""
    names = dict(NAME_CONST.findall((src / "names.rs").read_text()))
    found = []
    for path in sorted(src.glob("*/*.rs")):
        if path.name == "mod.rs":
            continue
        text = path.read_text()
        field = NAME_FIELD.search(text)
        reaches = set(CRATE_PATH.findall(text)) - {"names", *REEXPORTED}
        if field is None or reaches or field.group(1) not in names:
            continue
        found.append(Source(path, text, field.group(1), names[field.group(1)]))
    return found


def clone(source: Source, index: int) -> str:
    """`source` as card number `index`, its name unique: every use of its `names::` constant
    becomes `names::<CONST>_<index>`."""
    unique = re.compile(rf"\bnames::{source.const}\b")
    text = unique.sub(f"names::{source.const}_{index}", source.text)
    return "#![allow(warnings)]\n" + text


def shard_counts(cards: int, crates: int) -> list[int]:
    """How many cards each crate holds: as even as can be, none empty."""
    crates = max(1, min(crates, cards))
    per = math.ceil(cards / crates)
    counts = [per] * (cards // per)
    if cards % per:
        counts.append(cards % per)
    return counts


def shard_lib(first: int, picks: list[Source]) -> str:
    """A shard's `lib.rs`: the modules it re-exports, its `names`, its clones and its tables."""
    ids = range(first, first + len(picks))
    lines = [
        "#![allow(warnings)]",
        f"pub use beryl_cards::{{{', '.join(REEXPORTED)}}};",
        "",
        "pub mod names {",
        "    pub use beryl_cards::names::*;",
    ]
    for i, pick in zip(ids, picks, strict=True):
        lines.append(f'    pub const {pick.const}_{i}: &str = "{pick.name} {i}";')
    lines.append("}")
    lines += [f"pub mod c{i};" for i in ids]
    lines.append("pub static CARDS: &[&beryl_engine::card::CardDef] = &[")
    lines += [f"    &c{i}::CARD," for i in ids]
    lines += ["];", "pub static SCENARIOS: &[&[beryl_scenario::Scenario]] = &["]
    lines += [f"    c{i}::SCENARIOS," for i in ids]
    lines.append("];")
    return "\n".join(lines) + "\n"


def shard_manifest(name: str) -> str:
    return f"""[package]
name = "{name}"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
beryl_cards = {{ path = "{CRATES / "beryl_cards"}" }}
beryl_engine = {{ path = "{CRATES / "beryl_engine"}" }}
beryl_scenario = {{ path = "{CRATES / "beryl_scenario"}" }}
"""


def main_rs(shards: list[str]) -> str:
    """The binary: it reads every card and scenario, so the linker keeps them."""
    lines = [
        "use std::hint::black_box;",
        "",
        "fn main() {",
        "    let (mut cards, mut scenarios) = (0, 0);",
    ]
    for shard in shards:
        lines += [
            f"    for card in black_box({shard}::CARDS) {{",
            "        cards += black_box(card).faces.len();",
            "    }",
            f"    for card in black_box({shard}::SCENARIOS) {{",
            "        for scenario in black_box(*card) {",
            "            scenarios += black_box(scenario).name.len();",
            "        }",
            "    }",
        ]
    lines += ['    println!("{cards} {scenarios}");', "}"]
    return "\n".join(lines) + "\n"


def workspace_manifest(shards: list[str]) -> str:
    """The workspace: the checkout's release profile, so the numbers are the app's."""
    members = ", ".join(f'"{m}"' for m in ["all", *shards])
    return f"""[workspace]
resolver = "3"
members = [{members}]

[profile.release]
debug = "line-tables-only"
codegen-units = 1
lto = "thin"
"""


def generate(root: Path, cards: int, crates: int, pool: list[Source]) -> list[str]:
    """Writes the workspace at `root`, keeping its `target` directory; returns the shards'
    names. The checkout's `Cargo.lock` pins the dependencies the same."""
    for old in root.glob("*"):
        if old.name != "target":
            shutil.rmtree(old) if old.is_dir() else old.unlink()
    shards = []
    next_card = 0
    for number, count in enumerate(shard_counts(cards, crates)):
        name = f"shard{number}"
        picks = [pool[i % len(pool)] for i in range(next_card, next_card + count)]
        src = root / name / "src"
        src.mkdir(parents=True)
        for i, pick in zip(range(next_card, next_card + count), picks, strict=True):
            (src / f"c{i}.rs").write_text(clone(pick, i))
        (src / "lib.rs").write_text(shard_lib(next_card, picks))
        (root / name / "Cargo.toml").write_text(shard_manifest(name))
        shards.append(name)
        next_card += count
    binary = root / "all"
    (binary / "src").mkdir(parents=True)
    (binary / "src" / "main.rs").write_text(main_rs(shards))
    manifest = shard_manifest("all")
    deps = "".join(f'{s} = {{ path = "../{s}" }}\n' for s in shards)
    (binary / "Cargo.toml").write_text(manifest + deps)
    (root / "Cargo.toml").write_text(workspace_manifest(shards))
    shutil.copy(paths.RUST / "Cargo.lock", root / "Cargo.lock")
    return shards


def edit_card(root: Path) -> None:
    """Changes the first clone's file, as an edit would."""
    path = root / "shard0" / "src" / "c0.rs"
    path.write_text(path.read_text() + f"\nconst EDITED: u64 = {time.time_ns()};\n")


@dataclass
class Run:
    seconds: float  # load-adjusted
    wall: float
    rss: int  # bytes: the largest process's peak resident set


def parse_time(output: str) -> tuple[float, int]:
    """Wall seconds and maximum resident set size in bytes from `/usr/bin/time -l`'s report."""
    wall = re.search(r"^\s*([\d.]+)\s+real", output, re.MULTILINE)
    rss = re.search(r"^\s*(\d+)\s+maximum resident set size", output, re.MULTILINE)
    if wall is None or rss is None:
        raise RuntimeError("no /usr/bin/time -l report in:\n" + output[-2000:])
    return float(wall.group(1)), int(rss.group(1))


def timed(cmd: list[str], root: Path) -> Run:
    """Runs `cmd` in the workspace under `/usr/bin/time -l`, timing it on the load-adjusted
    clock; a failure ends the run with the compiler's output."""
    env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target")}
    env.pop("CARGO_BUILD_TARGET_DIR", None)
    clock = checks.Clock()
    run = subprocess.run(
        ["/usr/bin/time", "-l", *cmd],
        cwd=root,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    adjusted = clock.now()
    if run.returncode != 0:
        sys.exit(f"scale: {' '.join(cmd)} failed:\n{run.stderr[-6000:]}")
    wall, rss = parse_time(run.stderr)
    return Run(adjusted, wall, rss)


def gib(nbytes: int) -> str:
    return f"{nbytes / 2**30:.2f} GiB"


def report(cards: int, shards: int, pool: int, runs: dict[str, Run]) -> str:
    lines = [
        f"{cards:,} cards: clones of {pool} real card files, in {shards} crates",
        "",
    ]
    for label, run in runs.items():
        lines.append(
            f"{label:<26}{checks.took(run.wall, run.seconds):>34}"
            f"   peak {gib(run.rss)}"
        )
    largest = max(runs.items(), key=lambda item: item[1].rss)
    lines.append(
        f"{'largest compiler process':<26}{gib(largest[1].rss):>34}   ({largest[0]})"
    )
    return "\n".join(lines)


def scale(args: argparse.Namespace) -> int:
    pool = sources()
    root = paths.cache_root() / "scale"
    root.mkdir(exist_ok=True)
    print(f"scale: generating {args.cards:,} cards in {root}", file=sys.stderr)
    shards = generate(root, args.cards, args.crates, pool)
    if (root / "target").exists():
        # The shards' artifacts and incremental state from an earlier run would make a cold
        # build warm.
        subprocess.run(
            ["cargo", "clean", *[f"--package={s}" for s in ["all", *shards]]],
            cwd=root,
            env={**os.environ, "CARGO_TARGET_DIR": str(root / "target")},
            capture_output=True,
            check=False,
        )
    print("scale: warming the engine crates", file=sys.stderr)
    timed(["cargo", "check", "--package=beryl_cards"], root)
    timed(["cargo", "build", "--release", "--package=beryl_cards"], root)
    runs: dict[str, Run] = {}
    print("scale: cold cargo check", file=sys.stderr)
    runs["cold cargo check"] = timed(["cargo", "check", "--workspace"], root)
    print("scale: release build", file=sys.stderr)
    runs["release build"] = timed(
        ["cargo", "build", "--release", "--package=all"], root
    )
    print("scale: one-card rebuild", file=sys.stderr)
    edit_card(root)
    runs["one-card rebuild (check)"] = timed(["cargo", "check", "--workspace"], root)
    print(report(args.cards, len(shards), len(pool), runs))
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "scale",
        help="build cloned cards at scale and time the compiler (docs/project-50k.md)",
    )
    p.add_argument(
        "--cards", type=int, default=DEFAULT_CARDS, help="definitions to build"
    )
    p.add_argument(
        "--crates", type=int, default=DEFAULT_CRATES, help="crates holding them"
    )
    p.set_defaults(func=scale)


if __name__ == "__main__":
    sys.exit(f"run it as `by scale`, not {os.path.basename(sys.argv[0])}")
