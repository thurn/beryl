"""Lists and runs the Beryl 3D families in ignite/checks/beryl3d.toml.

The b3d product builds the workspace in the test profile and makes its recordings pack.
Checks run the binaries it built. The recorder and the inspector read the core grant from
IGNITE_CORES and IGNITE_CORES_FILE. Each replay check hashes the card files its recording
plays, alongside the shared inputs declared in the manifest.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tomllib
from collections.abc import Callable, Iterable
from pathlib import Path

from .. import card_files, card_store, paths, set_groups
from ..beryl3d import gate, pack, purity, timeline
from ..beryl3d import paths as b3d_paths
from ..system import WINDOWS, exe
from . import Listed, Result, beryl3d_validation, listed, report
from .rust import asked, parse_packages, unformatted
from .rust import clippy_run as run_clippy

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

# Keep Clippy out of the shared build directory and cap lints so Cargo caches diagnostics.
# The Windows target sets its own rustflags in rust/.cargo/config.toml.
CLIPPY_FLAGS = (
    "target.x86_64-pc-windows-msvc.rustflags" if WINDOWS else "build.rustflags"
)
CLIPPY: list[str] = [
    *["cargo", "clippy", "--locked", "--workspace", "--all-targets"],
    *[
        "--target-dir",
        "target/clippy",
        "--config",
        f'{CLIPPY_FLAGS}=["--cap-lints=warn"]',
    ],
]

# The crates the recordings and the programs that read them are built from: `record`, and `b3d`
# and `gate` in `beryl_inspect`.
ROOTS: tuple[str, ...] = ("beryl_inspect", "beryl_record")
CARDS: Path = paths.REPO / "rust" / "cards"
DECKS: Path = paths.REPO / "docs" / "sample-decks"

FRAME_LIMITS = "frame-limits"
# The one check of `b3d-frame-counts`: Callgrind counts every board at once in the
# beryl3d-bench machine, and the scaling check compares two boards' counts.
FRAME_COUNTS = "b3d-frame-counts:boards"


def run_out(
    command: list[str], cwd: Path = WORKSPACE
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def build(to_stderr: bool = False) -> int:
    """Makes the `b3d` product: the shared build, then the recordings pack for the present
    sources, which `record` builds only when it is missing and whose path it prints. With
    `to_stderr`, what they print goes to stderr, so that a listing on stdout stays clean.
    """
    out = sys.stderr if to_stderr else None
    built = subprocess.run(SHARED_BUILD, cwd=WORKSPACE, stdout=out, check=False)
    if built.returncode != 0:
        return built.returncode
    return subprocess.run(
        [str(exe(DEBUG / "record"))], cwd=WORKSPACE, stdout=out, check=False
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
    seen: dict[str, set[str]], names: dict[str, str], decks: dict[str, set[str]]
) -> dict[str, list[str]]:
    """The card files each recording plays, by recording: those whose card a message of it
    names, from either seat (`seen` holds the names each recording's messages spell, from
    [`pack.names_in`]), and those of the decks its fixture plays from (`decks`, from
    [`fixture_decks`])."""
    found: dict[str, list[str]] = {}
    for recording, spelled in seen.items():
        cards = set(decks.get(recording, set()))
        cards |= {names[s] for s in spelled if s in names}
        found[recording] = sorted(cards)
    return found


def pack_path() -> Path:
    """The present recordings pack, whose path `record` prints last."""
    done = run_out([str(exe(DEBUG / "record"))])
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip() or f"record: exit {done.returncode}")
    return Path(done.stdout.strip().splitlines()[-1])


def pack_name() -> str:
    """The recordings pack's file name."""
    return pack_path().name


def cards_by_recording() -> dict[str, list[str]]:
    """[`recording_cards`] for every recording in the present pack. The pack's name is a hash
    of everything a recording reads, so the result is kept beside it in Beryl 3D's cache, as
    `ignite-cards-<pack>.json`. The pack is read here rather than through `b3d export`, which
    wrote 450 MB of files and took 8 CPU-seconds, run on few threads while the machine is busy.
    The screens and stress boards that `b3d` makes itself are not in the pack, so they read no
    card file: `beryl_inspect` links no card crate."""
    path = pack_path()
    kept = b3d_paths.cache() / f"ignite-cards-{path.name}.json"
    if kept.exists():
        return json.loads(kept.read_text())
    defined = json.loads(
        (paths.REPO / "ui" / "fixtures.json").read_text(encoding="utf-8")
    )["fixtures"]
    decks = {d.name: d.read_text(encoding="utf-8") for d in sorted(DECKS.glob("*.txt"))}
    decks["wu-control"] = WU_CONTROL.read_text(encoding="utf-8")
    names = card_names()
    by_deck = fixture_decks(defined, decks, sorted(set(names.values())))
    try:
        entries = pack.read(path.read_bytes())
    except ValueError as error:
        raise SystemExit(f"{path}: {error}") from error
    found = recording_cards(pack.names_seen(entries, names), names, by_deck)
    partial = kept.with_name(f"{kept.name}.{os.getpid()}.tmp")
    partial.write_text(json.dumps(found))
    os.replace(partial, kept)
    return found


def b3d_text(arguments: list[str]) -> str:
    done = run_out([str(exe(DEBUG / "b3d")), *arguments])
    if done.returncode != 0:
        raise SystemExit(
            done.stderr.strip() or f"b3d {arguments[0]}: exit {done.returncode}"
        )
    return done.stdout


# The interface tests.


def test_names(*switches: str) -> list[str]:
    """Every interface test's name, `<recording>/<seat>/<window>` (`b3d test --list` prints each
    with a `b3d:` prefix)."""
    done = run_out([str(exe(DEBUG / "b3d")), "test", "--list", *switches])
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
            if (
                outcomes
                and output.get("deferred") == 0
                and isinstance(output.get("over_limits"), list)
            ):
                over = [str(o) for o in rows("over_limits")]
                results.append(
                    Result(check, "fail" if over else "pass", "\n".join(over))
                )
        elif name in outcomes:
            text = "\n".join(messages.get(name, []))
            if outcomes[name] == "fail" and not text:
                text = f"just b3d test {name}"
            results.append(Result(check, outcomes[name], text))
    return results


def prefix_groups(tests: list[str]) -> list[str]:
    """The prefixes to run `b3d test` with so that it plays the named `tests` and little else.
    `b3d test` takes one prefix, and the common prefix of tests from several recordings is empty
    or short, which plays every test (a batch did, 21 of the tip's 184, at about 3,775 billion
    instructions each). So there is one prefix per recording: the longest prefix its named tests
    share, which always includes the recording's name and a `/`."""
    by_recording: dict[str, list[str]] = {}
    for test in tests:
        by_recording.setdefault(recording_of(test), []).append(test)
    return [common_prefix(group) for _, group in sorted(by_recording.items())]


def interface_run(family: str, checks: list[str], switches: list[str]) -> list[Result]:
    wanted = asked(family, checks)
    if not wanted:
        return []
    tests = [w for w in wanted if w != FRAME_LIMITS]
    # The median of the frame limits is held only over a run of every test, which also plays
    # every named test.
    prefixes = [""] if FRAME_LIMITS in wanted else prefix_groups(tests)
    results: list[Result] = []
    for prefix in prefixes:
        command = [str(exe(DEBUG / "b3d")), "test", "--json", *switches]
        command[2:2] = [prefix] if prefix else []
        done = run_out(command)
        try:
            output = json.loads(done.stdout)
        except json.JSONDecodeError:
            print(done.stderr or done.stdout, file=sys.stderr)
            continue
        results += interface_results(output, family, wanted)
    return results


# The native frame gate.


def gate_boards() -> list[str]:
    done = run_out([str(exe(DEBUG / "gate")), "--list"])
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip())
    return done.stdout.split()


def gate_list() -> list[Listed]:
    """One check per gated board, each reading the card files of its recording, if it has one
    (the stress boards are made by `beryl_inspect` and have none)."""
    cards = cards_by_recording()
    return [Listed(f"b3d-gate:{b}", cards.get(b, [])) for b in gate_boards()]


def gate_results(output: str, wanted: list[str], budgets: gate.Budgets) -> list[Result]:
    """Each wanted board's result from the `gate` program's output and its budgets."""
    boards = {b.name: b for b in gate.parse(output)}
    enhanced = {t.name: t for t in gate.parse_enhanced(output)}
    particles = budgets["enhanced"].get("particles")
    results: list[Result] = []
    for name in wanted:
        board = boards.get(name)
        if board is None:
            continue
        tiered = enhanced.get(name)
        lines = gate.failures([board], budgets["boards"])
        if tiered is not None:
            lines += gate.enhanced_failures(
                [board], [tiered], particles["max"] if particles else None
            )
        if lines:
            results.append(Result(f"b3d-gate:{name}", "fail", "\n".join(lines)))
        elif tiered is not None:
            results.append(Result(f"b3d-gate:{name}", "pass"))
    return results


def gate_run(checks: list[str]) -> list[Result]:
    wanted = asked("b3d-gate", checks)
    if not wanted:
        return []
    command = [str(exe(DEBUG / "gate"))]
    for name in wanted:
        command += ["--board", name]
    done = run_out(command)
    status = f"gate exited with status {done.returncode}"
    results: list[Result] = []
    missing = f"{status} without a complete Baseline and Enhanced board result"
    if done.returncode == 0:
        budgets = gate.read_budgets()
        try:
            results = gate_results(done.stdout, wanted, budgets)
        except (KeyError, TypeError, ValueError) as error:
            missing = f"{status} with invalid board output: {error}"
    reported = {result.check for result in results}
    for check in checks:
        if check.startswith("b3d-gate:") and check not in reported:
            results.append(Result(check, "incomplete", missing))
    if (
        any(result.outcome == "incomplete" for result in results)
        and done.stdout.strip()
    ):
        print(done.stdout.rstrip()[-4000:], file=sys.stderr)
    if done.stderr.strip():
        print(done.stderr.rstrip(), file=sys.stderr)
    for result in results:
        if result.message:
            print(f"{result.check}: {result.message}", file=sys.stderr)
    return results


# Timeline parity of the recorded fixtures.


def fixtures() -> list[str]:
    defined = json.loads(
        (paths.REPO / "ui" / "fixtures.json").read_text(encoding="utf-8")
    )
    return [str(f["name"]) for f in defined["fixtures"]]


def timeline_list() -> list[Listed]:
    """One check per fixture, each reading the committed copy of the web interface's timeline
    of it (`timeline.copy_path`) and the card files its recording plays. No check reads a file
    of the web interface, so a change to the web interface runs none of them (docs/beryl-3d.md,
    decision C17)."""
    cards = cards_by_recording()
    return [
        Listed(
            f"b3d-timeline:{f}",
            [
                timeline.copy_path(f).relative_to(paths.REPO).as_posix(),
                *cards.get(f, []),
            ],
        )
        for f in fixtures()
    ]


def timeline_results(
    found: dict[str, timeline.Difference],
    names: list[tuple[str, str]],
    allowed: dict[str, int],
    wanted: list[str],
) -> list[Result]:
    """Each wanted fixture's result: it fails when it has no committed web timeline, or when one
    of its seats differs from that copy by more steps than data/beryl3d/timeline-parity.json
    allows. A fixture missing from the recordings pack fails as differing, since its copy has
    steps that Beryl 3D did not play; one whose copy and Beryl 3D's timeline are both empty
    passes. `found` are the differences over every recording and seat in `names`."""
    bad = {d.key: d for d in timeline.over_limit(found, allowed)}
    results: list[Result] = []
    for fixture in wanted:
        keys = [timeline.key_of(n, s) for n, s in names if n == fixture]
        check = f"b3d-timeline:{fixture}"
        copy = timeline.copy_shown(fixture)
        if not timeline.copy_path(fixture).is_file():
            results.append(
                Result(check, "fail", f"{copy} is missing; {timeline.REFRESH_WEB}")
            )
            continue
        over = [bad[k] for k in keys if k in bad]
        web_side = (
            f"(the web side is the committed copy {copy}; if the web interface changed "
            f"on purpose, {timeline.REFRESH_WEB})"
        )
        message = timeline.report(over, web_side) if over else ""
        results.append(Result(check, "fail" if over else "pass", message))
    return results


def timeline_run(checks: list[str]) -> list[Result]:
    """Compares Beryl 3D's timeline of each wanted fixture with the committed web timeline. It
    starts no `node` and reads nothing under ui/ beyond the fixtures' definitions."""
    wanted = asked("b3d-timeline", checks)
    if not wanted:
        return []
    printed = b3d_text(["timeline", "--all"]).splitlines()
    # The second line names the file of every recording's timeline (`timeline.native_lines`).
    native_file = Path(printed[1])
    native = timeline.parse(native_file.read_text().splitlines())
    native_file.unlink()
    web: timeline.Timelines = {}
    for fixture in wanted:
        web |= timeline.load_copy(fixture) or {}
    names = sorted(
        (recording, seat)
        for recording, seat in (k.rsplit("/", 1) for k in native.keys() | web.keys())
    )
    scope = {timeline.key_of(n, s) for n, s in names if n in wanted}
    found = timeline.differences(web, native, scope)
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
    return run_clippy(checks, command=CLIPPY, workspace=WORKSPACE, family="b3d-clippy")


def source_run(checks: list[str]) -> list[Result]:
    results: list[Result] = []
    for name in asked("b3d-source", checks):
        rule = purity.SOURCE_RULES.get(name)
        if rule is None:
            continue
        found = rule(paths.REPO)
        results.append(
            Result(f"b3d-source:{name}", "fail" if found else "pass", "\n".join(found))
        )
    return results


# A manual listing or an older daemon may list before building its product.
LIST_NEEDS_BUILD: frozenset[str] = frozenset(
    {
        "b3d-interface",
        "b3d-every-tick",
        "b3d-gate",
        "b3d-timeline",
        "b3d-frame-counts",
        "b3d-web-match",
    }
)


# Import family-specific adapters only when their family runs.
def web_match_list() -> list[Listed]:
    from . import beryl3d_match

    return beryl3d_match.list_web_match(cards_by_recording())


def web_match_run(checks: list[str]) -> list[Result]:
    from . import beryl3d_match

    return beryl3d_match.run_web_match(checks)


def web_drift_run(checks: list[str]) -> list[Result]:
    from . import beryl3d_match

    return beryl3d_match.run_web_drift(checks)


def timeline_drift_run(checks: list[str]) -> list[Result]:
    from . import beryl3d_timeline_drift

    return beryl3d_timeline_drift.run_timeline_drift(checks)


def frame_trend_run(checks: list[str]) -> list[Result]:
    from . import beryl3d_frame_trend

    return beryl3d_frame_trend.run(checks)


def next_checks(family: str) -> list[dict[str, str]]:
    if family == "b3d-web-drift":
        from . import beryl3d_match

        return beryl3d_match.next_web_drift()
    from . import beryl3d_timeline_drift

    return beryl3d_timeline_drift.next_timeline_drift()


LISTS: dict[str, Callable[[], list[Listed]]] = {
    "b3d-format": lambda: [
        Listed(f"b3d-format:{n}", [f"{d}/**"]) for n, d in package_dirs().items()
    ],
    "b3d-clippy": lambda: [
        Listed(f"b3d-clippy:{n}", [f"crate:{n}"]) for n in packages()
    ],
    "b3d-source": lambda: [
        Listed(f"b3d-source:{n}", purity.SOURCE_INPUTS.get(n, []))
        for n in purity.SOURCE_RULES
    ],
    "b3d-validation": beryl3d_validation.checks,
    "b3d-interface": lambda: interface_list("b3d-interface"),
    "b3d-every-tick": lambda: interface_list("b3d-every-tick"),
    "b3d-gate": gate_list,
    "b3d-timeline": timeline_list,
    "b3d-frame-counts": frame_counts_list,
    "b3d-web-match": lambda: web_match_list(),
    # An exploration family's checks come from `next`.
    "b3d-web-drift": lambda: [],
    "b3d-timeline-drift": lambda: [],
    "b3d-frame-trend": lambda: [],
}

RUNS: dict[str, Callable[[list[str]], list[Result]]] = {
    "b3d-format": format_run,
    "b3d-clippy": clippy_run,
    "b3d-source": source_run,
    "b3d-validation": lambda c: beryl3d_validation.run(DEBUG, c),
    "b3d-interface": lambda c: interface_run("b3d-interface", c, []),
    "b3d-every-tick": lambda c: interface_run("b3d-every-tick", c, ["--every-tick"]),
    "b3d-gate": gate_run,
    "b3d-timeline": timeline_run,
    "b3d-frame-counts": frame_counts_run,
    "b3d-web-match": lambda c: web_match_run(c),
    "b3d-web-drift": lambda c: web_drift_run(c),
    "b3d-timeline-drift": timeline_drift_run,
    "b3d-frame-trend": frame_trend_run,
}


def main(args: list[str]) -> int:
    match args:
        case ["build"]:
            return build()
        case ["b3d-frame-trend", "list" | "next"]:
            from . import beryl3d_frame_trend

            for entry in beryl3d_frame_trend.next_checks():
                print(json.dumps(entry), flush=True)
            return 0
        case ["b3d-corpus", *rest]:
            from .beryl3d_corpus import main as corpus_main

            return corpus_main(rest, DEBUG)
        case [family, "list"] if family in LISTS:
            if family in LIST_NEEDS_BUILD and not exe(DEBUG / "record").is_file():
                built = build(to_stderr=True)
                if built != 0:
                    return built
            listed(LISTS[family]())
            return 0
        case [("b3d-web-drift" | "b3d-timeline-drift") as family, "next"]:
            if not exe(DEBUG / "record").is_file():
                built = build(to_stderr=True)
                if built != 0:
                    return built
            for line in next_checks(family):
                print(json.dumps(line), flush=True)
            return 0
        case [family, "run", *checks] if family in RUNS:
            results = RUNS[family](checks)
            report(results)
            return int(any(r.outcome == "fail" for r in results))
        case _:
            families = ", ".join(LISTS)
            print(
                f"usage: beryl3d.py <family> list | <family> run <check>... | <exploration family> next | build\n"
                f"families: {families}",
                file=sys.stderr,
            )
            return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
