"""Lists and runs the the client families in ignite/checks/client.toml.

The client product builds the workspace in the test profile and makes its recordings pack.
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
from ..client import gate, pack, purity
from ..client import paths as client_paths
from ..system import WINDOWS, exe
from . import Listed, Result, client_validation, listed, report
from .rust import asked, parse_packages, unformatted
from .rust import clippy_run as run_clippy

WORKSPACE: Path = client_paths.WORKSPACE
# The `test` profile builds into target/debug, as `dev` does.
DEBUG: Path = WORKSPACE / "target" / "debug"

# The one build that the tests, `record` and `gate` share (`cli.SHARED_BUILD` in
# tools/src/beryl_tools/client/cli.py says why it builds every target in the `test` profile):
# with it, the tests that follow compile nothing more.
SHARED_BUILD: list[str] = [
    *["cargo", "build", "--locked", "-q", "--workspace", "--profile", "test"],
    *["--tests", "--bins", "--examples"],
]

# Keep Clippy out of the shared build directory and cap lints so Cargo caches diagnostics.
# The Windows target sets its own rustflags in .cargo/config.toml.
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

# The crates the recordings and the programs that read them are built from: `record`, and `client`
# and `gate` in `beryl_inspect`.
ROOTS: tuple[str, ...] = ("beryl_inspect", "beryl_record")
CARDS: Path = paths.REPO / "engine" / "cards"
DECKS: Path = paths.REPO / "docs" / "sample-decks"

INTERFACE_FAMILY = "client-interface"
FRAME_LIMITS_FAMILY = "client-interface-frame-limits"
FRAME_LIMITS = f"{FRAME_LIMITS_FAMILY}:corpus"
BUDGET_SUITES: dict[str, str] = {
    FRAME_LIMITS_FAMILY: FRAME_LIMITS,
    "client-gate": "client-gate:boards",
}


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
    """Makes the `client` product: the shared build, then the recordings pack for the present
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
    `repo`, sorted: The client's own, the main workspace's and the card crates."""
    found: set[Path] = set()
    todo = [repo / "client" / "crates" / name for name in ROOTS]
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
    `engine/crates/<crate>/**`. The client's own crates are named with `crate:` and the card crates
    by their files."""
    crates = repo / "engine" / "crates"
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
    of everything a recording reads, so the result is kept beside it in The client's cache, as
    `ignite-cards-<pack>.json`. The pack is read here rather than through `client export`, which
    wrote 450 MB of files and took 8 CPU-seconds, run on few threads while the machine is busy.
    The screens and stress boards that `client` makes itself are not in the pack, so they read no
    card file: `beryl_inspect` links no card crate."""
    path = pack_path()
    kept = client_paths.cache() / f"ignite-cards-{path.name}.json"
    if kept.exists():
        return json.loads(kept.read_text())
    defined = json.loads(
        (paths.REPO / client_paths.FIXTURE_DEFINITIONS).read_text(encoding="utf-8")
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


def client_text(arguments: list[str]) -> str:
    done = run_out([str(exe(DEBUG / "client")), *arguments])
    if done.returncode != 0:
        raise SystemExit(
            done.stderr.strip() or f"client {arguments[0]}: exit {done.returncode}"
        )
    return done.stdout


# The interface tests.


def test_names() -> list[str]:
    """Every interface test's name, `<recording>/<seat>/<window>` (`client test --list` prints each
    with a `client:` prefix)."""
    done = run_out([str(exe(DEBUG / "client")), "test", "--list"])
    if done.returncode != 0:
        raise SystemExit(
            done.stderr.strip() or f"client test --list: exit {done.returncode}"
        )
    return [line.removeprefix("client:") for line in done.stdout.split()]


def recording_of(test: str) -> str:
    """The recording an interface test plays: its name without the seat and window."""
    return test.rsplit("/", 2)[0]


def interface_list() -> list[Listed]:
    """One check per interface test, hashed over the card files its recording plays."""
    cards = cards_by_recording()
    return [
        Listed(f"{INTERFACE_FAMILY}:{t}", cards.get(recording_of(t), []))
        for t in test_names()
    ]


def frame_limits_list() -> list[Listed]:
    """The corpus-wide check reads the union of the ordinary checks' card files."""
    cards = cards_by_recording()
    every = sorted({c for t in test_names() for c in cards.get(recording_of(t), [])})
    return [Listed(FRAME_LIMITS, every)]


def common_prefix(names: list[str]) -> str:
    """The longest prefix of all `names`, which `b3d test` takes to play fewer tests."""
    if not names:
        return ""
    first, last = min(names), max(names)
    size = 0
    while size < min(len(first), len(last)) and first[size] == last[size]:
        size += 1
    return first[:size]


def interface_results(output: dict[str, object], wanted: list[str]) -> list[Result]:
    """Each wanted check's result from `b3d test --json`'s output: a test passes or fails by its
    entry in `results`, which counts a fault over the list of allowed faults and an entry of the
    list that is now too high. A test the run did not play is left out, so incomplete.
    """

    def rows(key: str) -> list[object]:
        found = output.get(key)
        return found if isinstance(found, list) else []

    outcomes: dict[str, str] = {}
    for row in rows("results"):
        if isinstance(row, dict):
            outcomes[str(row["test"]).removeprefix("client:")] = str(row["outcome"])
    messages: dict[str, list[str]] = {}
    for key in ("over_listed", "listed_too_high"):
        for why in rows(key):
            for test in outcomes:
                if test in str(why):
                    messages.setdefault(test, []).append(str(why))
    for fault in rows("faults"):
        if isinstance(fault, dict) and "test" in fault:
            test = str(fault["test"]).removeprefix("client:")
            messages.setdefault(test, []).append(json.dumps(fault))
    results: list[Result] = []
    for name in wanted:
        check = f"{INTERFACE_FAMILY}:{name}"
        if name in outcomes:
            text = "\n".join(messages.get(name, []))
            if outcomes[name] == "fail" and not text:
                text = f"just client test {name}"
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


def interface_run(checks: list[str]) -> list[Result]:
    wanted = asked(INTERFACE_FAMILY, checks)
    results: list[Result] = []
    for prefix in prefix_groups(wanted):
        output = replay_output([prefix])
        if output is not None:
            results += interface_results(output, wanted)
    return results


def replay_output(arguments: list[str]) -> dict[str, object] | None:
    done = run_out([str(exe(DEBUG / "client")), "test", *arguments, "--json"])
    try:
        output = json.loads(done.stdout)
    except json.JSONDecodeError:
        print(done.stderr or done.stdout, file=sys.stderr)
        return None
    return output if isinstance(output, dict) else None


def frame_limits_run(checks: list[str]) -> list[Result]:
    output = replay_output(["--frame-limits"]) if FRAME_LIMITS in checks else None
    if output is None:
        return []
    tests, over = output.get("tests"), output.get("over_limits")
    if not isinstance(tests, int) or tests <= 0 or not isinstance(over, list):
        return []
    return [Result(FRAME_LIMITS, "fail" if over else "pass", "\n".join(map(str, over)))]


# The native frame gate.


def gate_boards() -> list[str]:
    done = run_out([str(exe(DEBUG / "gate")), "--list"])
    if done.returncode != 0:
        raise SystemExit(done.stderr.strip())
    return done.stdout.split()


def gate_list() -> list[Listed]:
    """One check per gated board, each reading the card files of its recording, if it has one
    (the stress boards are made by `beryl_inspect` and have none)."""
    from .. import calibration
    from ..client import frame_profiles

    cards = cards_by_recording()
    digest = ""
    try:
        digest = frame_profiles.native().digest
    except calibration.SetupError as error:
        print(f"client-gate setup: {error}", file=sys.stderr)
    return [
        Listed(f"client-gate:{b}", cards.get(b, []), digest=digest)
        for b in gate_boards()
    ]


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
            results.append(Result(f"client-gate:{name}", "fail", "\n".join(lines)))
        elif tiered is not None:
            results.append(Result(f"client-gate:{name}", "pass"))
    return results


def gate_run(checks: list[str]) -> list[Result]:
    from .. import calibration
    from ..client import frame_profiles

    wanted = asked("client-gate", checks)
    if not wanted:
        return []
    try:
        budgets = gate.profile_budgets(gate.read_budgets(), frame_profiles.native())
    except calibration.SetupError as error:
        return [
            Result(f"client-gate:{name}", "incomplete", str(error)) for name in wanted
        ]
    command = [str(exe(DEBUG / "gate"))]
    for name in wanted:
        command += ["--board", name]
    done = run_out(command)
    status = f"gate exited with status {done.returncode}"
    results: list[Result] = []
    missing = f"{status} without a complete Baseline and Enhanced board result"
    if done.returncode == 0:
        try:
            results = gate_results(done.stdout, wanted, budgets)
        except (KeyError, TypeError, ValueError) as error:
            missing = f"{status} with invalid board output: {error}"
    reported = {result.check for result in results}
    for check in checks:
        if check.startswith("client-gate:") and check not in reported:
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


def frame_counts_list() -> list[Listed]:
    from . import client_frame_counts

    return client_frame_counts.list_checks()


def frame_counts_run(checks: list[str]) -> list[Result]:
    from . import client_frame_counts

    return client_frame_counts.run(checks)


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
        check = f"client-format:{name}"
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
    wanted = asked("client-format", checks)
    dirs = package_dirs()
    chosen = [w for w in wanted if w in dirs]
    if not chosen:
        return []
    done = run_out(["cargo", "fmt", "--check", *[a for n in chosen for a in ("-p", n)]])
    return format_results(done.returncode, done.stdout, dirs, chosen)


def clippy_run(checks: list[str]) -> list[Result]:
    return run_clippy(
        checks, command=CLIPPY, workspace=WORKSPACE, family="client-clippy"
    )


def source_run(checks: list[str]) -> list[Result]:
    results: list[Result] = []
    for name in asked("client-source", checks):
        rule = purity.SOURCE_RULES.get(name)
        if rule is None:
            continue
        found = rule(paths.REPO)
        results.append(
            Result(
                f"client-source:{name}", "fail" if found else "pass", "\n".join(found)
            )
        )
    return results


# A manual listing or an older daemon may list before building its product.
LIST_NEEDS_BUILD: frozenset[str] = frozenset(
    {
        INTERFACE_FAMILY,
        FRAME_LIMITS_FAMILY,
        "client-gate",
        "client-frame-counts",
    }
)


def frame_trend_run(checks: list[str]) -> list[Result]:
    from . import client_frame_trend

    return client_frame_trend.run(checks)


def budget_sample_name(family: str) -> str:
    """Offer one sample per verified commit without building the client to list it."""
    from ..client.trend import current_commit

    return f"{BUDGET_SUITES[family]}/{current_commit()}"


def budget_sample_run(family: str, check: str) -> Result:
    current = budget_sample_name(family)
    if check != current:
        # A saved idle run can start again after the verified checkout moves.
        return Result(check, "incomplete", f"Checkout now offers {current}.")
    if family == FRAME_LIMITS_FAMILY:
        results = frame_limits_run([FRAME_LIMITS])
    else:
        results = gate_run([f"client-gate:{board}" for board in gate_boards()])
    if not results:
        return Result(check, "incomplete", "The native runner did not report a sample.")
    problems = [row for row in results if row.outcome != "pass"]
    if not problems:
        return Result(check, "pass")
    message = "\n".join(f"{row.check}: {row.message}" for row in problems)
    outcome = "fail" if any(row.outcome == "fail" for row in problems) else "incomplete"
    return Result(check, outcome, message)


LISTS: dict[str, Callable[[], list[Listed]]] = {
    "client-format": lambda: [
        Listed(f"client-format:{n}", [f"{d}/**"]) for n, d in package_dirs().items()
    ],
    "client-clippy": lambda: [
        Listed(f"client-clippy:{n}", [f"crate:{n}"]) for n in packages()
    ],
    "client-source": lambda: [
        Listed(f"client-source:{n}", purity.SOURCE_INPUTS.get(n, []))
        for n in purity.SOURCE_RULES
    ],
    "client-validation": client_validation.checks,
    INTERFACE_FAMILY: interface_list,
    FRAME_LIMITS_FAMILY: frame_limits_list,
    "client-gate": gate_list,
    "client-frame-counts": frame_counts_list,
    # An exploration family's checks come from `next`.
    "client-frame-trend": lambda: [],
}

RUNS: dict[str, Callable[[list[str]], list[Result]]] = {
    "client-format": format_run,
    "client-clippy": clippy_run,
    "client-source": source_run,
    "client-validation": lambda c: client_validation.run(DEBUG, c),
    INTERFACE_FAMILY: interface_run,
    FRAME_LIMITS_FAMILY: frame_limits_run,
    "client-gate": gate_run,
    "client-frame-counts": frame_counts_run,
    "client-frame-trend": frame_trend_run,
}


def main(args: list[str]) -> int:
    match args:
        case ["build"]:
            return build()
        case ["client-frame-trend", "list" | "next"]:
            from . import client_frame_trend

            for entry in client_frame_trend.next_checks():
                print(json.dumps(entry), flush=True)
            return 0
        case ["client-corpus", *rest]:
            from .client_corpus import main as corpus_main

            return corpus_main(rest, DEBUG)
        case [family, "next"] if family in BUDGET_SUITES:
            listed([Listed(budget_sample_name(family))])
            return 0
        case [family, "run", check] if family in BUDGET_SUITES and check.startswith(
            f"{BUDGET_SUITES[family]}/"
        ):
            result = budget_sample_run(family, check)
            report([result])
            return int(result.outcome == "fail")
        case [family, "list"] if family in LISTS:
            if family in LIST_NEEDS_BUILD and not exe(DEBUG / "record").is_file():
                built = build(to_stderr=True)
                if built != 0:
                    return built
            listed(LISTS[family]())
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
