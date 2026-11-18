"""The adapter for Beryl 3D's families in `ignite/checks/beryl3d.toml`, and the command that makes
its `b3d` build product.

`python -m beryl_tools.ignite.beryl3d <family> list` prints the family's checks, and
`python -m beryl_tools.ignite.beryl3d <family> run <check>...` runs those named and reports one
result per check (`beryl_tools.ignite.report`). `build` makes the `b3d` product: the build that
the tests, `record` and `gate` share, then the recordings pack.

The programs run are the ones the product built, in rust/beryl3d/target/debug, never through
`just b3d`, whose commands build again and take CPU tokens from the pool Ignite replaces. Ignite
gives each command its grant in `IGNITE_CORES` (and `IGNITE_CORES_FILE` when it changes while the
command runs), which `beryl_inspect::runner::threads` and `granted` read.

The interface tests, the gate and timeline parity all read the recordings pack, so every check
of theirs reads what a recording reads: the crates the recorder and `b3d` are built from, which
the manifest names (a test holds that list to [`main_crates`]), and the card files its own
recording plays ([`recording_cards`]), which each check lists as inputs of its own. A card file
that a recording does not play is not among that recording's checks' inputs, so editing it
reruns none of them.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from collections.abc import Callable, Iterable
from pathlib import Path

from .. import card_files, card_store, paths, set_groups
from ..beryl3d import gate, purity, timeline
from ..beryl3d import paths as b3d_paths
from . import Listed, Result, listed, report
from .rust import asked, clippy_results, parse_packages, unformatted

WORKSPACE: Path = b3d_paths.WORKSPACE
# The `test` profile builds into target/debug, as `dev` does.
DEBUG: Path = WORKSPACE / "target" / "debug"

# The one build that the tests, `record` and `gate` share (`cli.SHARED_BUILD` in
# tools/src/beryl_tools/beryl3d/cli.py says why it builds every target in the `test` profile):
# with it, the tests that follow compile nothing more.
SHARED_BUILD: list[str] = [
    *["cargo", "build", "--locked", "-q", "--workspace", "--profile", "test"],
    *["--tests", "--bins", "--examples"],
]

# Clippy's own target directory (`cli.CLIPPY_TARGET` says why), and clippy with every lint
# capped at a warning, as the Rust family runs it (`rust.CLIPPY` says why): every unit then
# succeeds, cargo keeps its result, and a run after an unchanged one compiles nothing.
CLIPPY: list[str] = [
    *["cargo", "clippy", "--locked", "--workspace", "--all-targets"],
    *["--target-dir", "target/clippy"],
    *["--config", 'build.rustflags=["--cap-lints=warn"]'],
]

# The crates the recordings and the programs that read them are built from: `record`, and `b3d`
# and `gate` in `beryl_inspect`.
ROOTS: tuple[str, ...] = ("beryl_inspect", "beryl_record")
CARDS: Path = paths.REPO / "rust" / "cards"
DECKS: Path = paths.REPO / "docs" / "sample-decks"

# The check of `b3d-interface` that holds the frame limits on the whole run (`tests.median_frames`
# in data/beryl3d/gates.json), which no single test's result says.
FRAME_LIMITS = "frame-limits"
# The one check of `b3d-frame-counts`: Callgrind counts every board at once in the
# beryl3d-bench machine, and the scaling check compares two boards' counts.
FRAME_COUNTS = "b3d-frame-counts:boards"

# The source rules of tools/src/beryl_tools/beryl3d/purity.py, by check name.
SOURCE_RULES: dict[str, Callable[[Path], list[str]]] = {
    "scene-purity": purity.scene_purity,
    "lints-match": purity.lints_match,
    "data-names": purity.data_names,
    "scene-literals": purity.scene_literals,
}


def run_out(
    command: list[str], cwd: Path = WORKSPACE
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command, cwd=cwd, capture_output=True, text=True, errors="replace", check=False
    )


def build() -> int:
    """Makes the `b3d` product: the shared build, then the recordings pack for the present
    sources, which `record` builds only when it is missing and whose path it prints."""
    built = subprocess.run(SHARED_BUILD, cwd=WORKSPACE, check=False).returncode
    if built != 0:
        return built
    return subprocess.run(
        [str(DEBUG / "record")], cwd=WORKSPACE, check=False
    ).returncode


# What a recording reads.


def path_dependencies(manifest: dict[str, object]) -> list[str]:
    """The `path` of each dependency a crate is built with: its dependencies and build
    dependencies, for every target, but not its dev dependencies, which no recording runs.
    """
    tables: list[object] = [
        manifest.get("dependencies"),
        manifest.get("build-dependencies"),
    ]
    targets = manifest.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if isinstance(target, dict):
                tables += [target.get("dependencies"), target.get("build-dependencies")]
    found: list[str] = []
    for table in tables:
        if not isinstance(table, dict):
            continue
        for spec in table.values():
            if isinstance(spec, dict) and isinstance(spec.get("path"), str):
                found.append(str(spec["path"]))
    return found


def recorder_crates(repo: Path = paths.REPO) -> list[Path]:
    """Every crate directory the [`ROOTS`] are built from through path dependencies, under
    `repo`, sorted: Beryl 3D's own, the main workspace's and the card crates."""
    found: set[Path] = set()
    todo = [repo / "rust" / "beryl3d" / "crates" / name for name in ROOTS]
    while todo:
        crate = todo.pop()
        manifest = crate / "Cargo.toml"
        if crate in found or not manifest.is_file():
            continue
        found.add(crate)
        parsed = tomllib.loads(manifest.read_text())
        todo += [Path(os.path.normpath(crate / p)) for p in path_dependencies(parsed)]
    return sorted(found)


def main_crates(repo: Path = paths.REPO) -> list[str]:
    """The main workspace's crates among [`recorder_crates`], as the manifest names them:
    `rust/crates/<crate>/**`. Beryl 3D's own crates are named with `crate:` and the card crates
    by their files."""
    crates = repo / "rust" / "crates"
    return [
        f"{c.relative_to(repo).as_posix()}/**"
        for c in recorder_crates(repo)
        if c.parent == crates
    ]


# The deck a fixture names `wu-control`: `decks::WU_CONTROL` is this list's main deck.
WU_CONTROL: Path = DECKS / "2025-11-18-17lands-cb8d5d5f-wu-control.txt"

# A JSON string, its text between the quotes.
STRING: re.Pattern[str] = re.compile(r'"((?:[^"\\]|\\.)*)"')


def card_names() -> dict[str, str]:
    """Each card file by its card's name and by each face's name, from the card store."""
    found: dict[str, str] = {}
    with card_store.Store(card_store.STORE) as store:
        for path in card_files.files(CARDS):
            name = card_files.name_of(path, store)
            if name is None:
                continue
            file = path.relative_to(paths.REPO).as_posix()
            for spelled in [name, *name.split(" // ")]:
                found[spelled] = file
    return found


def deck_cards(text: str, by_module: dict[str, str]) -> set[str]:
    """The card files of the cards a decklist names, as `4 Counterspell`."""
    found: set[str] = set()
    for line in text.splitlines():
        match = re.match(r"^\s*\d+\s+(.+?)\s*$", line)
        if match:
            module = set_groups.card_module(match.group(1))
            if module in by_module:
                found.add(by_module[module])
    return found


def fixture_decks(
    defined: list[dict[str, object]], decks: dict[str, str], files: Iterable[str]
) -> dict[str, set[str]]:
    """The card files each fixture's decks hold, by fixture, for the fixtures that play from a
    deck: `decks` (agents' games, `wu-control` or `fuzz:<seed>`) or a `table` with a deck
    file. `decks` maps a deck's name (`wu-control` or a file under docs/sample-decks) to its
    list. An agent's choices depend on the cards it cannot see, which no view names, so its
    whole deck is read. A fuzz deck may hold any card, so it reads every card file."""
    files = list(files)
    by_module = {Path(f).stem: f for f in files}
    found: dict[str, set[str]] = {}
    for fixture in defined:
        names: list[str] = []
        listed_decks = fixture.get("decks")
        if isinstance(listed_decks, list):
            names += [str(d) for d in listed_decks]
        table = fixture.get("table")
        if isinstance(table, dict) and isinstance(table.get("deck"), str):
            names.append(str(table["deck"]))
        cards: set[str] = set()
        for name in names:
            if name.startswith("fuzz:"):
                cards |= set(files)
            elif name in decks:
                cards |= deck_cards(decks[name], by_module)
        if cards:
            found[str(fixture["name"])] = cards
    return found


def recording_cards(
    exported: dict[str, list[str]], names: dict[str, str], decks: dict[str, set[str]]
) -> dict[str, list[str]]:
    """The card files each recording plays, by recording: those whose card a message of it
    names, from either seat (`exported` holds each recording's message files), and those of the
    decks its fixture plays from (`decks`, from [`fixture_decks`])."""
    found: dict[str, list[str]] = {}
    for recording, texts in exported.items():
        cards = set(decks.get(recording, set()))
        for text in texts:
            cards |= {names[s] for s in set(STRING.findall(text)) if s in names}
        found[recording] = sorted(cards)
    return found


def pack_name() -> str:
    """The recordings pack's file name, which `record` prints the path of last."""
    done = run_out([str(DEBUG / "record")])
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip() or f"record: exit {done.returncode}")
    return Path(done.stdout.strip().splitlines()[-1]).name


def cards_by_recording() -> dict[str, list[str]]:
    """[`recording_cards`] for every recording in the present pack. The pack's name is a hash
    of everything a recording reads, so the result is kept beside it in Beryl 3D's cache, as
    `ignite-cards-<pack>.json`, and exporting every recording (about 400 MB) is done once a
    pack."""
    kept = b3d_paths.cache() / f"ignite-cards-{pack_name()}.json"
    if kept.exists():
        return json.loads(kept.read_text())
    defined = json.loads((paths.REPO / "ui" / "fixtures.json").read_text())["fixtures"]
    decks = {d.name: d.read_text(encoding="utf-8") for d in sorted(DECKS.glob("*.txt"))}
    decks["wu-control"] = WU_CONTROL.read_text(encoding="utf-8")
    names = card_names()
    by_deck = fixture_decks(defined, decks, sorted(set(names.values())))
    directory = Path(tempfile.mkdtemp(prefix="b3d-cards-"))
    try:
        b3d_text(["export", str(directory)])
        exported: dict[str, list[str]] = {}
        for file in sorted(directory.glob("*.json")):
            stem = file.name.removesuffix(".json").rsplit(".", 1)[0]
            exported.setdefault(stem.replace("__", "/"), []).append(
                file.read_text(encoding="utf-8", errors="replace")
            )
    finally:
        shutil.rmtree(directory, ignore_errors=True)
    found = recording_cards(exported, names, by_deck)
    partial = kept.with_name(f"{kept.name}.{os.getpid()}.tmp")
    partial.write_text(json.dumps(found))
    os.replace(partial, kept)
    return found


def b3d_text(arguments: list[str]) -> str:
    done = run_out([str(DEBUG / "b3d"), *arguments])
    if done.returncode != 0:
        raise SystemExit(
            done.stderr.strip() or f"b3d {arguments[0]}: exit {done.returncode}"
        )
    return done.stdout


# The interface tests.


def test_names(*switches: str) -> list[str]:
    """Every interface test's name, `<recording>/<seat>/<window>` (`b3d test --list` prints each
    with a `b3d:` prefix)."""
    done = run_out([str(DEBUG / "b3d"), "test", "--list", *switches])
    if done.returncode != 0:
        raise SystemExit(
            done.stderr.strip() or f"b3d test --list: exit {done.returncode}"
        )
    return [line.removeprefix("b3d:") for line in done.stdout.split()]


def recording_of(test: str) -> str:
    """The recording an interface test plays: its name without the seat and window."""
    return test.rsplit("/", 2)[0]


def interface_list(family: str) -> list[Listed]:
    """One check per interface test, each reading the card files its recording plays. A stress
    board, which `beryl_inspect` makes with no recording, reads none. [`FRAME_LIMITS`] reads
    every test's."""
    cards = cards_by_recording()
    tests = test_names()
    checks = [Listed(f"{family}:{t}", cards.get(recording_of(t), [])) for t in tests]
    if family == "b3d-interface":
        every = sorted({c for t in tests for c in cards.get(recording_of(t), [])})
        checks.append(Listed(f"{family}:{FRAME_LIMITS}", every))
    return checks


def common_prefix(names: list[str]) -> str:
    """The longest prefix of all `names`, which `b3d test` takes to play fewer tests."""
    if not names:
        return ""
    first, last = min(names), max(names)
    size = 0
    while size < min(len(first), len(last)) and first[size] == last[size]:
        size += 1
    return first[:size]


def interface_results(
    output: dict[str, object], family: str, wanted: list[str]
) -> list[Result]:
    """Each wanted check's result from `b3d test --json`'s output: a test passes or fails by its
    entry in `results`, which counts a fault over the list of allowed faults and an entry of the
    list that is now too high, and [`FRAME_LIMITS`] by `over_limits`. A test the run did not
    play is left out, so incomplete."""

    def rows(key: str) -> list[object]:
        found = output.get(key)
        return found if isinstance(found, list) else []

    outcomes: dict[str, str] = {}
    for row in rows("results"):
        if isinstance(row, dict):
            outcomes[str(row["test"]).removeprefix("b3d:")] = str(row["outcome"])
    messages: dict[str, list[str]] = {}
    for key in ("over_listed", "listed_too_high"):
        for why in rows(key):
            for test in outcomes:
                if test in str(why):
                    messages.setdefault(test, []).append(str(why))
    for fault in rows("faults"):
        if isinstance(fault, dict) and "test" in fault:
            test = str(fault["test"]).removeprefix("b3d:")
            messages.setdefault(test, []).append(json.dumps(fault))
    results: list[Result] = []
    for name in wanted:
        check = f"{family}:{name}"
        if name == FRAME_LIMITS:
            over = [str(o) for o in rows("over_limits")]
            results.append(Result(check, "fail" if over else "pass", "\n".join(over)))
        elif name in outcomes:
            text = "\n".join(messages.get(name, []))
            if outcomes[name] == "fail" and not text:
                text = f"just b3d test {name}"
            results.append(Result(check, outcomes[name], text))
    return results


def interface_run(family: str, checks: list[str], switches: list[str]) -> list[Result]:
    wanted = asked(family, checks)
    if not wanted:
        return []
    tests = [w for w in wanted if w != FRAME_LIMITS]
    # The median of the frame limits is held only over a run of every test.
    prefix = "" if FRAME_LIMITS in wanted else common_prefix(tests)
    command = [str(DEBUG / "b3d"), "test", "--json", *switches]
    if prefix:
        command.insert(2, prefix)
    done = run_out(command)
    try:
        output = json.loads(done.stdout)
    except json.JSONDecodeError:
        print(done.stderr or done.stdout, file=sys.stderr)
        return []
    return interface_results(output, family, wanted)


# The native frame gate.


def gate_boards() -> list[str]:
    done = run_out([str(DEBUG / "gate"), "--list"])
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip())
    return done.stdout.split()


def gate_list() -> list[Listed]:
    """One check per gated board, each reading the card files of its recording, if it has one
    (the stress boards are made by `beryl_inspect` and have none)."""
    cards = cards_by_recording()
    return [Listed(f"b3d-gate:{b}", cards.get(b, [])) for b in gate_boards()]


def gate_results(
    output: str, wanted: list[str], data: dict[str, object]
) -> list[Result]:
    """Each wanted board's result from the `gate` program's output, held to the limits under
    `frames` in data/beryl3d/gates.json and to the Enhanced tier's particles."""
    boards = {b.name: b for b in gate.parse(output)}
    enhanced = {t.name: t for t in gate.parse_enhanced(output)}
    frames = data.get("frames")
    limits = frames if isinstance(frames, dict) else {}
    tiers = data.get("tiers")
    particles = None
    if isinstance(tiers, dict):
        particles = (tiers.get("enhanced") or {}).get("particles")
    results: list[Result] = []
    for name in wanted:
        board = boards.get(name)
        if board is None:
            continue
        lines = gate.failures([board], limits)
        if name in enhanced:
            lines += gate.enhanced_failures([board], [enhanced[name]], particles)
        outcome = "fail" if lines else "pass"
        results.append(Result(f"b3d-gate:{name}", outcome, "\n".join(lines)))
    return results


def gate_run(checks: list[str]) -> list[Result]:
    wanted = asked("b3d-gate", checks)
    if not wanted:
        return []
    command = [str(DEBUG / "gate")]
    for name in wanted:
        command += ["--board", name]
    done = run_out(command)
    if done.returncode != 0:
        print(done.stderr, file=sys.stderr)
        return []
    return gate_results(done.stdout, wanted, json.loads(gate.GATES.read_text()))


# Timeline parity of the recorded fixtures.


def fixtures() -> list[str]:
    defined = json.loads((paths.REPO / "ui" / "fixtures.json").read_text())
    return [str(f["name"]) for f in defined["fixtures"]]


def timeline_list() -> list[Listed]:
    """One check per fixture, each reading the card files of its recording and the web
    interface's timeline, `ui/scripts/timeline.mjs` with every file it imports."""
    cards = cards_by_recording()
    ui = paths.REPO / "ui"
    web = [p.relative_to(paths.REPO).as_posix() for p in timeline.web_sources(ui)]
    return [Listed(f"b3d-timeline:{f}", [*web, *cards.get(f, [])]) for f in fixtures()]


def timeline_results(
    found: dict[str, timeline.Difference],
    names: list[tuple[str, str]],
    allowed: dict[str, int],
    wanted: list[str],
) -> list[Result]:
    """Each wanted fixture's result: it fails when one of its seats differs from the web
    interface's timeline by more steps than data/beryl3d/timeline-parity.json allows, or when
    no seat of it was recorded. `found` are the differences over every exported recording and
    seat in `names`."""
    bad = {d.key: d for d in timeline.over_limit(found, allowed)}
    results: list[Result] = []
    for fixture in wanted:
        keys = [timeline.key_of(n, s) for n, s in names if n == fixture]
        check = f"b3d-timeline:{fixture}"
        if not keys:
            results.append(Result(check, "fail", f"{fixture} has no recording"))
            continue
        over = [bad[k] for k in keys if k in bad]
        message = timeline.report(over, "node ui/scripts/timeline.mjs") if over else ""
        results.append(Result(check, "fail" if over else "pass", message))
    return results


def timeline_run(checks: list[str]) -> list[Result]:
    wanted = asked("b3d-timeline", checks)
    if not wanted:
        return []
    directory = Path(tempfile.mkdtemp(prefix="b3d-parity-"))
    try:
        b3d_text(["export", str(directory)])
        names: list[tuple[str, str]] = []
        for file in sorted(directory.glob("*.json")):
            stem, seat = file.name.removesuffix(".json").rsplit(".", 1)
            names.append((stem.replace("__", "/"), seat))
        web = subprocess.run(
            ["node", "scripts/timeline.mjs", str(directory)],
            cwd=paths.REPO / "ui",
            capture_output=True,
            text=True,
            check=False,
        )
        if web.returncode != 0:
            print(web.stderr, file=sys.stderr)
            return []
        printed = b3d_text(["timeline", "--all"]).splitlines()
        # The second line names the file of every recording's timeline (`timeline.native_lines`).
        native_file = Path(printed[1])
        native = native_file.read_text().splitlines()
        native_file.unlink()
    finally:
        shutil.rmtree(directory, ignore_errors=True)
    scope = {timeline.key_of(n, s) for n, s in names if n in wanted}
    found = timeline.differences(
        timeline.parse(web.stdout.splitlines()), timeline.parse(native), scope
    )
    return timeline_results(found, names, timeline.load_allowlist(), wanted)


# The instruction counts in the beryl3d-bench machine.


def frame_counts_list() -> list[Listed]:
    cards = cards_by_recording()
    boards = sorted({c for b in gate_boards() for c in cards.get(b, [])})
    return [Listed(FRAME_COUNTS, boards)]


def frame_counts_run(checks: list[str]) -> list[Result]:
    """Counts every board under Callgrind with `just b3d gate --counts`. Its machine, its
    toolchain and the sources it sends in are set up by tools/src/beryl_tools/bench.py and
    beryl3d/vm.py, which this adapter does not import, so it runs the command."""
    if FRAME_COUNTS not in checks:
        return []
    done = run_out(
        [sys.executable, "-m", "beryl_tools.cli", "b3d", "gate", "--counts"], paths.REPO
    )
    if done.returncode == 2:
        # The machine could not count (no Lima, not Apple silicon): no verdict.
        print(done.stdout + done.stderr, file=sys.stderr)
        return []
    if done.returncode == 0:
        return [Result(FRAME_COUNTS, "pass")]
    failures = [
        line.removeprefix("FAIL ")
        for line in done.stderr.splitlines()
        if line.startswith("FAIL ")
    ]
    return [Result(FRAME_COUNTS, "fail", "\n".join(failures) or done.stderr.strip())]


# Formatting, clippy and the source rules.


def packages() -> list[str]:
    done = run_out(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"]
    )
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip())
    return [p.name for p in parse_packages(done.stdout, paths.REPO)]


def package_dirs() -> dict[str, str]:
    done = run_out(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"]
    )
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip())
    return {p.name: p.dir for p in parse_packages(done.stdout, paths.REPO)}


def format_results(
    returncode: int, output: str, dirs: dict[str, str], wanted: list[str]
) -> list[Result]:
    """Each wanted package's result from `cargo fmt --check`: it fails when a file in its
    directory has a diff."""
    bad = unformatted(output)
    results: list[Result] = []
    for name in wanted:
        if name not in dirs:
            continue
        root = (paths.REPO / dirs[name]).resolve()
        mine = {f: line for f, line in bad.items() if f.resolve().is_relative_to(root)}
        check = f"b3d-format:{name}"
        if not mine:
            results.append(Result(check, "pass" if returncode == 0 or bad else "fail"))
            continue
        first, line = next(iter(mine.items()))
        listing = "\n".join(
            f.resolve().relative_to(paths.REPO).as_posix() for f in mine
        )
        where = first.resolve().relative_to(paths.REPO).as_posix()
        message = f"not formatted (run `just fmt`):\n{listing}"
        results.append(Result(check, "fail", message, f"{where}:{line}"))
    return results


def format_run(checks: list[str]) -> list[Result]:
    wanted = asked("b3d-format", checks)
    dirs = package_dirs()
    chosen = [w for w in wanted if w in dirs]
    if not chosen:
        return []
    done = run_out(["cargo", "fmt", "--check", *[a for n in chosen for a in ("-p", n)]])
    return format_results(done.returncode, done.stdout, dirs, chosen)


def clippy_run(checks: list[str]) -> list[Result]:
    wanted = asked("b3d-clippy", checks)
    if not wanted:
        return []
    done = subprocess.run(
        [*CLIPPY, "--message-format=json"],
        cwd=WORKSPACE,
        stdout=subprocess.PIPE,
        text=True,
        errors="replace",
        check=False,
    )
    found = clippy_results(done.stdout, wanted)
    return [
        Result(
            r.check.replace("clippy:", "b3d-clippy:", 1),
            r.outcome,
            r.message,
            r.at.replace("rust/", "rust/beryl3d/", 1) if r.at else "",
        )
        for r in found
    ]


def source_run(checks: list[str]) -> list[Result]:
    results: list[Result] = []
    for name in asked("b3d-source", checks):
        rule = SOURCE_RULES.get(name)
        if rule is None:
            continue
        found = rule(paths.REPO)
        results.append(
            Result(f"b3d-source:{name}", "fail" if found else "pass", "\n".join(found))
        )
    return results


# The command line.


LISTS: dict[str, Callable[[], list[Listed]]] = {
    "b3d-format": lambda: [
        Listed(f"b3d-format:{n}", [f"{d}/**"]) for n, d in package_dirs().items()
    ],
    "b3d-clippy": lambda: [
        Listed(f"b3d-clippy:{n}", [f"crate:{n}"]) for n in packages()
    ],
    "b3d-source": lambda: [Listed(f"b3d-source:{n}") for n in SOURCE_RULES],
    "b3d-interface": lambda: interface_list("b3d-interface"),
    "b3d-every-tick": lambda: interface_list("b3d-every-tick"),
    "b3d-gate": gate_list,
    "b3d-timeline": timeline_list,
    "b3d-frame-counts": frame_counts_list,
}

RUNS: dict[str, Callable[[list[str]], list[Result]]] = {
    "b3d-format": format_run,
    "b3d-clippy": clippy_run,
    "b3d-source": source_run,
    "b3d-interface": lambda c: interface_run("b3d-interface", c, []),
    "b3d-every-tick": lambda c: interface_run("b3d-every-tick", c, ["--every-tick"]),
    "b3d-gate": gate_run,
    "b3d-timeline": timeline_run,
    "b3d-frame-counts": frame_counts_run,
}


def main(args: list[str]) -> int:
    match args:
        case ["build"]:
            return build()
        case [family, "list"] if family in LISTS:
            listed(LISTS[family]())
            return 0
        case [family, "run", *checks] if family in RUNS:
            results = RUNS[family](checks)
            report(results)
            return int(any(r.outcome == "fail" for r in results))
        case _:
            families = ", ".join(LISTS)
            print(
                f"usage: beryl3d.py <family> list | <family> run <check>... | build\n"
                f"families: {families}",
                file=sys.stderr,
            )
            return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
