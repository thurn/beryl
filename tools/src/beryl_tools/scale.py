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

It then prints what scripts cost (plan K24): the seconds `beryl_cli script-check` takes to play
every spec moment of each pooled card from both seats in a debug build, where every script the
session builds is held to the replay check and the rule invariants, per card and extrapolated to
the N clones. A clone is a copy of a real card file, so its scenarios are the same work as its
original's: the clones' cost is their originals' summed, measured once on the real cards. It is
reported with no bar.

Budgets are not asserted yet (milestone 7 adds `--assert`: check 60 s, release 180 s, one-card
rebuild 2 s, 4 GiB). Times are load-adjusted as the checks' are (`checks.Clock`). The one-card
rebuild is a `cargo check` one, the edit-and-look loop.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from . import checks, nearest, paths, script_checks

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
# A card's script check in the run log: `script:<card>/<moment>/<seat>`.
SCRIPT_CHECK: re.Pattern[str] = re.compile(r"^script:([^/]+)/[^/]+/[^/]+$")
# The cards the script cost names by name, the costliest first.
COSTLIEST = 5
# `script-check`'s last line: its checks' own seconds, past the process's start.
SCRIPT_SUMMARY: re.Pattern[str] = re.compile(
    r"^script-check: .* in ([\d.]+) s$", re.MULTILINE
)


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
    lines += [f"    c{i}::scenarios::SCENARIOS," for i in ids]
    lines.append("];")
    return "\n".join(lines) + "\n"


def card_crates() -> list[str]:
    """The checkout's crates a card file may use: those `beryl_cards` depends on by path, so
    a clone compiles wherever its original does."""
    manifest = tomllib.loads((CRATES / "beryl_cards" / "Cargo.toml").read_text())
    dependencies: dict[str, object] = manifest["dependencies"]
    return sorted(
        name
        for name, spec in dependencies.items()
        if isinstance(spec, dict) and "path" in spec
    )


def shard_manifest(name: str) -> str:
    dependencies = "".join(
        f'{crate} = {{ path = "{CRATES / crate}" }}\n'
        for crate in ["beryl_cards", *card_crates()]
    )
    return f"""[package]
name = "{name}"
version = "0.0.0"
edition = "2024"
publish = false

[features]
# A clone's scenarios sit behind its crate's `scenarios` feature, as the real cards' do; the
# rehearsal builds them, as the tests and tools do.
default = ["scenarios"]
scenarios = []

[dependencies]
{dependencies}"""


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


@dataclass
class Scripts:
    """What a script-check run of the pool's cards cost: each card's checks' seconds summed, by
    slug, how many checks there were, the run's seconds and the seconds its checks took, past
    the process's start, by its own clock."""

    cards: dict[str, float]
    checks: int
    seconds: float  # load-adjusted
    wall: float
    checking: float


def card_seconds(lines: Iterable[str]) -> tuple[dict[str, float], int]:
    """Each card's script checks' seconds summed, by its slug, from `script-check`'s run-log
    lines, and how many checks there were; fixtures' recordings are no card's."""
    cards: dict[str, float] = {}
    count = 0
    for line in lines:
        try:
            record = json.loads(line)
            check, seconds = str(record["check"]), float(record["seconds"])
        except (json.JSONDecodeError, KeyError, TypeError, ValueError):
            continue
        found = SCRIPT_CHECK.match(check)
        if found is None or found.group(1) == "fixtures":
            continue
        cards[found.group(1)] = cards.get(found.group(1), 0.0) + seconds
        count += 1
    return cards, count


def clone_seconds(cards: dict[str, float], pool: list[Source], count: int) -> float:
    """The script checks' seconds of `count` clones: clone `i` is `pool[i % len(pool)]`, as
    [`generate`] picks them, and costs what its original does."""
    each = [cards.get(nearest.slug(s.name), 0.0) for s in pool]
    rounds, rest = divmod(count, len(each))
    return rounds * math.fsum(each) + math.fsum(each[:rest])


def measure_scripts(pool: list[Source]) -> Scripts:
    """Runs the script checks of the pool's cards, every spec moment from both seats, and reads
    what each cost from the results they log."""
    if not script_checks.build():
        sys.exit("scale: beryl_cli did not build for the script checks")
    slugs = sorted({nearest.slug(s.name) for s in pool})
    with tempfile.TemporaryDirectory() as tmp:
        results, printed = Path(tmp) / "results.jsonl", Path(tmp) / "out.txt"
        clock = checks.Clock()
        began = time.monotonic()
        with open(printed, "w") as out:
            code = script_checks.run(slugs, results, out=out)
        wall, adjusted = time.monotonic() - began, clock.now()
        output = printed.read_text()
        print(output, end="", file=sys.stderr)
        if code != 0:
            sys.exit(
                "scale: a script check failed: `just test-rust`, or the script lane"
            )
        lines = results.read_text().splitlines() if results.exists() else []
    summary = SCRIPT_SUMMARY.search(output)
    cards, count = card_seconds(lines)
    checking = float(summary.group(1)) if summary else wall
    return Scripts(cards, count, adjusted, wall, checking)


def script_report(scripts: Scripts, pool: list[Source], cards: int) -> str:
    """The script cost, per card and for the `cards` clones, at the run's own parallelism."""
    measured = [nearest.slug(s.name) for s in pool]
    spent = sum(scripts.cards.values())
    names = sorted(set(measured))
    mean = sum(scripts.cards.get(n, 0.0) for n in names) / len(names)
    costliest = sorted(scripts.cards.items(), key=lambda kv: -kv[1])[:COSTLIEST]
    clones = clone_seconds(scripts.cards, pool, cards)
    # The run's checks overlap on its threads: the clones' would as much.
    parallel = max(spent / scripts.checking, 1.0) if scripts.checking > 0 else 1.0
    run = checks.took(scripts.wall, scripts.seconds)
    lines = [
        "",
        f"{'script checks':<26}{run:>34}   {scripts.checks:,} checks: the spec moments "
        f"of {len(names)} cards from both seats, every script built and replayed",
        f"{'  per card':<26}{mean:>32.3f} s   of checks on average; the costliest: "
        + ", ".join(f"{name} {seconds:.2f} s" for name, seconds in costliest),
        f"{f'  {cards:,} clones':<26}{clones:>32,.0f} s   of checks, about "
        f"{clones / parallel:,.0f} s at this run's parallelism ({parallel:.1f}x)",
    ]
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
    print("scale: script checks", file=sys.stderr)
    scripts = measure_scripts(pool)
    print(report(args.cards, len(shards), len(pool), runs))
    print(script_report(scripts, pool, args.cards))
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
