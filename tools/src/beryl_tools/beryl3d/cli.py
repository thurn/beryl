"""`just b3d`: Beryl 3D's commands. With no argument it lists them, one line each;
`just b3d <command> --help` prints a command's options and two examples.

Every command follows the rules in docs/beryl-3d.md ('The agent interface'): its output begins
with the command that reproduces it, is bounded, and names any file it writes for the run.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from types import ModuleType
from typing import TextIO

from .. import system
from .. import paths as repo_paths
from . import (
    app,
    bug,
    camera,
    cargo_tests,
    clear_save,
    compare,
    corpus,
    gate,
    live,
    match,
    motion_film,
    new,
    pack,
    parity,
    paths,
    perf,
    phone,
    play,
    purity,
    spec,
    stand_in,
    tiers,
    timeline,
    trend,
    vm,
)
from .command import Command

# Exit code for a command line `just b3d` cannot run, as argparse uses.
USAGE = 2
# The environment variable that tells a program how many threads it may use.
CORES_ENV = "IGNITE_CORES"


def cargo(arguments: list[str], env: dict[str, str] | None = None) -> int:
    """Runs cargo with `arguments` in Beryl 3D's workspace, in `env` (this process's own by
    default)."""
    return subprocess.run(
        ["cargo", *arguments],
        cwd=paths.WORKSPACE,
        env=os.environ if env is None else env,
        check=False,
    ).returncode


def build(arguments: list[str]) -> int:
    """Runs `cargo <arguments>`, a build in the workspace's own target directory, one at a time
    across every process of this checkout. Cargo's lock on the directory already makes a second
    build wait while another runs. Taking a file lock first (`paths.build_lock`) makes the
    waiting explicit and means a second build usually finds nothing left to do."""
    with system.file_lock(paths.build_lock()):
        return cargo(arguments)


def quick_build(arguments: list[str]) -> int:
    """Runs `cargo <arguments>` as `build` does, for a program an agent waits on after an edit."""
    return build(arguments)


def run_cargo(args: argparse.Namespace) -> int:
    return cargo(list(args.arguments))


def test_command(packages: list[str], filters: list[str], nextest: bool) -> list[str]:
    """Build the workspace as `SHARED_BUILD` does so selecting tests preserves features."""
    if nextest:
        return [
            *["nextest", "run", "--cargo-quiet", "--no-fail-fast", "--no-tests=pass"],
            *["--status-level=fail", "--final-status-level=fail", "--workspace"],
            *filterset(packages, filters),
        ]
    if packages:
        return cargo_tests.BUILD
    return ["test", "-q", "--workspace", *(["--", *filters] if filters else [])]


def filterset(packages: list[str], filters: list[str]) -> list[str]:
    """The nextest `-E` arguments that select the tests of `packages` whose names contain any
    of `filters`, or none when both are empty. A name filter on nextest's command line is
    written as `test(...)` here, because how nextest combines name filters with a filterset
    has changed between its releases."""
    parts = [
        f"({' | '.join(f'{kind}({value})' for value in values)})"
        for kind, values in (
            ("package", [f"={p}" for p in packages]),
            ("test", filters),
        )
        if values
    ]
    return ["-E", " & ".join(parts)] if parts else []


def test_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("-p", "--package", action="append", default=[])
    parser.add_argument("filter", nargs="*")


def run_cargo_test(args: argparse.Namespace) -> int:
    nextest = shutil.which("cargo-nextest") is not None
    # The tests read recordings, which only the `record` program can build.
    recorded = record()
    if recorded != 0:
        return recorded
    command = test_command(args.package, args.filter, nextest)
    if not nextest and args.package:
        return cargo_tests.run(command, args.package, args.filter)
    return cargo(command)


# The one build that the tests, `record` and `gate` share: every target that `cargo nextest run
# --workspace` builds, in cargo's `test` profile, and each program built as a program. Cargo
# resolves features for the packages a command names, so `cargo run -p beryl_record` and
# `cargo run -p beryl_inspect` each built `beryl_scene`, `beryl_inspect` and the engine with
# other features than the tests do, and they compiled them three times after an edit to
# `beryl_scene`. With this build the tests that follow it compile nothing more.
SHARED_BUILD: list[str] = [
    *["build", "-q", "--workspace", "--profile", "test"],
    *["--tests", "--bins", "--examples"],
]


def shared_build() -> int:
    return build(SHARED_BUILD)


def program(name: str) -> Path:
    """The program `name` that `shared_build` built. The `test` profile builds into
    `target/debug`, as `dev` does."""
    return system.exe(paths.WORKSPACE / "target" / "debug" / name)


# The status `b3d` and `b3d-render` exit with when the recordings pack for the present sources
# is missing (`beryl_inspect::recordings::MISSING_STATUS`).
MISSING_PACK = 3
PACK_ENV: str = pack.PACK_ENV


def record(*selection: str, stdout: TextIO | None = None) -> int:
    """Builds the recordings pack for the present sources with the `record` program of
    `beryl_record` if it is missing. `b3d` does not link the engine that records, so that it
    links and starts fast after an edit (docs/beryl-3d.md, 'Edit-to-output time'). It prints
    the pack's path."""
    built = shared_build()
    if built != 0:
        return built
    # Cargo refreshes the top-level copy even when it compiles nothing. Use the linker's
    # file so a warm invocation does not pay macOS's first-run check again (`linked`).
    line = [str(linked("record")), *selection]
    return subprocess.run(
        line, cwd=paths.WORKSPACE, check=False, stdout=stdout
    ).returncode


def record_arguments(parser: argparse.ArgumentParser) -> None:
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument(
        "--all", action="store_true", help="record the entire catalog"
    )
    selection.add_argument("--prefix", help="record names matching this prefix")
    selection.add_argument(
        "--cards", nargs="+", help="record these cards' spec moments"
    )


def run_record(args: argparse.Namespace) -> int:
    if args.all:
        return record("--all")
    if args.prefix is not None:
        return record("--prefix", args.prefix)
    if args.cards is not None:
        return record("--cards", *args.cards)
    return record()


# The valued options may precede the recording in the native CLI.
RECORDING_OPTIONS = {
    "--seat",
    "--layout",
    "--size",
    "--scale",
    "--tier",
    "--at",
    "--do",
    "--from",
    "--to",
    "--within",
    "--role",
    "--crop",
    "--frames",
    "--every",
    "--stop-after",
    "--only",
}


def pack_selection(arguments: list[str]) -> tuple[str, ...]:
    """The recorder arguments for an explicit local request, or the default corpus."""
    if not arguments or "--help" in arguments or "-h" in arguments:
        return ()
    command, *args = arguments
    if (
        command == "test"
        and "--all" in args
        or command == "timeline"
        and "--all" in args
    ):
        return ("--all",)
    if command == "corpus":
        return () if "--snapshot" in args or "--against" in args else ("--all",)
    positionals: list[str] = []
    only = ""
    remaining = iter(args)
    for arg in remaining:
        flag, eq, value = arg.partition("=")
        if flag in RECORDING_OPTIONS:
            value = value if eq else next(remaining, "")
            if flag == "--only":
                only = value
        elif not arg.startswith("--"):
            positionals.append(arg)
    if command == "export" and not only:
        return ("--all",)
    name = only if command == "export" else next(iter(positionals), "")
    if command == "list":
        return ("--prefix", name) if name else ("--all",)
    if command == "test" and name:
        return ("--prefix", name)
    if command in {
        "info",
        "tree",
        "explain",
        "diff",
        "check",
        "timeline",
        "png",
        "film",
        "export",
    }:
        if name.startswith("b3d:"):
            name = name.removeprefix("b3d:").rsplit("/", 2)[0]
        if "/" in name and not name.startswith("story/"):
            names = (paths.DATA / "corpus.txt").read_text().splitlines()
            if name not in names:
                return ("--prefix", f"{name}/")
    return ()


def linked(name: str) -> Path:
    """The binary `name` as the linker wrote it, in `target/debug/deps`. Cargo copies it to
    `target/debug/<name>` as a new file on every build, even one that changed nothing, and
    macOS checks each new executable before its first run: 1 to 9 s for `b3d` under the
    machine's usual load. The linker's own file keeps its inode until the next link, so
    running it pays that check once per link rather than once per command. The linker's file
    is named for the crate's form of `name`, with `_` for each `-`."""
    debug = paths.WORKSPACE / "target" / "debug"
    stem = name.replace("-", "_")
    pattern = re.compile(rf"{re.escape(stem)}-[0-9a-f]{{16}}")
    found = [p for p in (debug / "deps").glob(f"{stem}-*") if pattern.fullmatch(p.name)]
    if not found:
        return debug / name
    # A test harness built for the same crate (`cargo test`) has the same file name pattern
    # and may be newer. Cargo records the program's build as `bin-<name>` in its
    # fingerprint directory and a harness's as `test-bin-<name>`.
    fingerprints = debug / ".fingerprint"
    program = [
        p
        for p in found
        if any(fingerprints.glob(f"*-{p.name.rsplit('-', 1)[1]}/bin-{name}"))
    ]
    return max(program or found, key=lambda p: p.stat().st_mtime)


def run_program(
    name: str,
    arguments: list[str],
    binary: Path | None = None,
    want: int = 0,
) -> int:
    """Runs the built program `name` (the file `binary`, or else `linked(name)`) with
    `arguments`. Its checks spread over as many threads as `IGNITE_CORES` allows, or over every
    CPU when that is not set (`beryl_inspect::runner::threads`), and a `want` of 1 limits it
    to one thread. When it says the recordings pack is missing, builds the pack and runs it
    again."""
    return launch_program(name, arguments, binary, want=want).returncode


def launch_program(
    name: str,
    arguments: list[str],
    binary: Path | None = None,
    *,
    want: int = 0,
    capture: bool = False,
    recorder: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run an inspector command against its local selection, if it requests one."""
    line: list[str] = [str(binary or linked(name)), *arguments]
    env: dict[str, str] = (
        {**os.environ, CORES_ENV: str(want)} if want else dict(os.environ)
    )
    if name == "b3d" and arguments[:1] == ["map"]:
        env["BERYL3D_LAUNCHER_COMMANDS"] = json.dumps(command_entries())
    selection = () if PACK_ENV in env else pack_selection(arguments)
    if selection:
        if recorder is None:
            built = shared_build()
            if built:
                return subprocess.CompletedProcess[str](line, built, "", "")
            recorder = linked("record")
        try:
            env[PACK_ENV] = str(pack.record(recorder, *selection))
        except subprocess.CalledProcessError as error:
            return subprocess.CompletedProcess[str](line, error.returncode, "", "")

    def launch() -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            line, env=env, capture_output=capture, text=True, check=False
        )

    done = launch()
    if done.returncode != MISSING_PACK or PACK_ENV in env:
        return done
    if recorder is None:
        recorded = record()
        if recorded:
            return subprocess.CompletedProcess[str](line, recorded, "", "")
    else:
        try:
            pack.record(recorder)
        except subprocess.CalledProcessError as error:
            return subprocess.CompletedProcess[str](line, error.returncode, "", "")
    return launch()


def run_b3d(command: str, single: bool = False) -> Callable[[argparse.Namespace], int]:
    """Builds the `b3d` program of `beryl_inspect` and runs its `command` with the arguments.
    A `single` command runs on one thread: `just b3d tree` works for under a second, so
    starting a dozen threads for it only adds start-up time."""

    def run(args: argparse.Namespace) -> int:
        built = quick_build(["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"])
        if built != 0:
            return built
        return run_program("b3d", [command, *args.arguments], want=1 if single else 0)

    return run


def run_shared_b3d(command: str) -> Callable[[argparse.Namespace], int]:
    """Runs the `b3d` program that `shared_build` built with its `command` and the arguments.
    Building `b3d` alone (`run_b3d`) is a `dev` build with other features than the shared
    `test` build, so it compiles eight crates again; `test` uses the shared build so that it
    compiles nothing beyond what `cargo-test` and `record` already built."""

    def run(args: argparse.Namespace) -> int:
        built = shared_build()
        if built != 0:
            return built
        if command == "corpus" and "--sweep" in args.arguments:
            return corpus.run(args.arguments, program("b3d"), program("record"))
        return launch_program(
            "b3d",
            [command, *args.arguments],
            program("b3d"),
            recorder=program("record"),
        ).returncode

    return run


def b3d_command(
    name: str, summary: str, shared: bool = False, single: bool = False
) -> Command:
    """A text command of the `b3d` program, which reads its own options and prints its own
    `--help` (`cli` in `beryl_inspect`). It runs the `b3d` that `shared_build` built when
    `shared` is set, and otherwise builds `b3d` alone, which relinks fastest after an edit.
    A `single` command does its work on one thread (`run_b3d`).
    """
    run = run_shared_b3d(name) if shared else run_b3d(name, single)
    return Command(name, summary, (), run, passthrough=True)


def run_render(command: str) -> Callable[[argparse.Namespace], int]:
    """Builds the `b3d-render` program of `beryl_pictures` and runs its `command` with the
    arguments. Like `b3d`, it links no engine, so that it relinks and starts fast after an edit
    (docs/beryl-3d.md, 'Edit-to-output time')."""

    def run(args: argparse.Namespace) -> int:
        arguments = ["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"]
        built = quick_build(arguments)
        if built != 0:
            return built
        # `png` took 0.38 s on one thread and on four, and 0.73 s and 0.77 s with every face
        # drawn, so it runs on one thread.
        return run_program("b3d-render", [command, *args.arguments], want=1)

    return run


def render_command(name: str, summary: str) -> Command:
    """A picture command of the `b3d-render` program, which reads its own options and prints
    its own `--help` (`beryl_pictures`)."""
    return Command(name, summary, (), run_render(name), passthrough=True)


# Clippy's own target directory, nested in the workspace's as the main workspace's is
# (`paths.CLIPPY_TARGET`). Its check units are not the ones a build produces, so in the shared
# directory it would share no work with the build, only Cargo's lock on it.
CLIPPY_TARGET: Path = paths.WORKSPACE / "target" / "clippy"


def run_clippy(_args: argparse.Namespace) -> int:
    return cargo(
        ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        {**os.environ, "CARGO_TARGET_DIR": str(CLIPPY_TARGET)},
    )


def run_fmt(args: argparse.Namespace) -> int:
    return cargo(["fmt", "--all", *(["--check"] if args.check else [])])


# The commands of the modules that register their own, listed after this module's, in order. A
# module adds a command by writing `COMMANDS` and adding its name here.
MODULES: tuple[ModuleType, ...] = (
    app,
    phone,
    camera,
    vm,
    parity,
    play,
    perf,
    trend,
    spec,
    motion_film,
    bug,
    clear_save,
    new,
    match,
)

OWN_COMMANDS: tuple[Command, ...] = (
    b3d_command("data", "validate the client tuning files", single=True),
    b3d_command(
        "map", "the client’s source and data, from its registries", single=True
    ),
    b3d_command(
        "list",
        "every recording whose name starts with a prefix, with its length and views",
        single=True,
    ),
    b3d_command(
        "info",
        "a recording's moments: views, script steps and prompts with their buttons",
        single=True,
    ),
    b3d_command(
        "tree",
        "the elements on screen at one moment of a recording, as an indented tree",
        single=True,
    ),
    b3d_command("explain", "why an element is placed and styled", single=True),
    b3d_command(
        "diff",
        "the elements that appeared, left, moved or changed state between two moments",
        single=True,
    ),
    b3d_command(
        "check",
        "every interface check on every tick of a recording, and the faults",
    ),
    b3d_command(
        "corpus",
        "one name and count for each distinct recording signature",
        shared=True,
    ),
    b3d_command(
        "test",
        "interface tests for the corpus and stories, or a recording prefix",
        shared=True,
    ),
    b3d_command(
        "timeline",
        "when each step of each script of a recording begins and ends, as JSON lines",
    ),
    b3d_command(
        "story",
        "list stories or write a story from a recording's moment",
        single=True,
    ),
    b3d_command(
        "export",
        "every recording's messages, as the fixture files the web interface reads",
    ),
    render_command(
        "png",
        "draws one moment of a recording offscreen to a PNG file, and with --tree its tree",
    ),
    render_command(
        "film",
        "draws up to 24 frames of a recording as a grid in one PNG, a line of text per frame",
    ),
    compare.COMMAND,
    Command(
        "levels",
        "reports upward imports and toolkit protocol counts during the client core split",
        (
            ("just b3d levels", "list each upward import with its file and line"),
            (
                "just b3d levels --update",
                "lower counts after removing imports or protocol names",
            ),
        ),
        lambda args: purity.levels_step(*(["--update"] if args.update else [])),
        lambda p: p.add_argument(
            "--update", action="store_true", help="lower the recorded counts"
        ),
    ),
    Command(
        "timeline-parity",
        "holds the web interface's and Beryl 3D's timelines of every recording to each other",
        (
            (
                "just b3d timeline-parity --fixtures",
                "the recorded fixtures only",
            ),
            (
                "just b3d timeline-parity --fixtures --refresh-web",
                "rewrite the committed web timelines the b3d-timeline check reads",
            ),
        ),
        lambda args: timeline.run_parity(args),
        timeline.parity_arguments,
    ),
    Command(
        "cargo",
        "runs cargo with the given arguments in rust/beryl3d",
        (
            ("just b3d cargo check -p app_client", "type-check one crate"),
            ("just b3d cargo build --release", "an optimized build of the workspace"),
        ),
        run_cargo,
        lambda p: p.add_argument("arguments", nargs=argparse.REMAINDER),
    ),
    Command(
        "cargo-test",
        "runs the workspace's tests, or those of the crates named with -p",
        (
            ("just b3d cargo-test", "every test of the workspace"),
            (
                "just b3d cargo-test -p kit_space pose",
                "tests of one crate named pose",
            ),
        ),
        run_cargo_test,
        test_arguments,
    ),
    Command(
        "record",
        "builds the recordings pack for the present sources if it is missing",
        (
            ("just b3d record", "build the pack, or print the path of the one built"),
            (
                "just b3d record && just b3d list",
                "build the pack, then list its recordings",
            ),
        ),
        run_record,
        record_arguments,
    ),
    Command(
        "clippy",
        "runs clippy over every target of the workspace, with warnings as errors",
        (
            ("just b3d clippy", "lint the workspace"),
            (
                "just b3d cargo clippy -p app_client",
                "lint one crate, warnings allowed",
            ),
        ),
        run_clippy,
    ),
    Command(
        "fmt",
        "formats the workspace with rustfmt; --check only reports",
        (
            ("just b3d fmt", "format every crate"),
            ("just b3d fmt --check", "fail if any file is not formatted"),
        ),
        run_fmt,
        lambda p: p.add_argument("--check", action="store_true"),
    ),
    Command(
        "gate",
        "plays the gated boards natively and fails on a work count or allocation over its limit",
        (
            ("just b3d gate", "hold every gated board to data/beryl3d/budgets.json"),
            (
                "just b3d gate --gpu",
                "count shader cost and, in beryl3d-bench, fragments shaded",
            ),
        ),
        gate.run_gate,
        gate.gate_arguments,
    ),
    Command(
        "app",
        "runs the app; an agent gets no window unless it names one, and then gives --for",
        (
            (
                "just b3d app --fixture verbs-zones --for 10s",
                "replay a recording with no window for 10 s",
            ),
            (
                "just b3d app --fixture verbs-zones --pause-at 1500ms --window background --for 5s",
                "a window behind the others, its clock stopped at 1.5 s",
            ),
        ),
        app.run_app,
        app.app_arguments,
    ),
    Command(
        "live",
        "asks the running debug app for its tree or a picture, or gives it an action",
        (
            (
                "just b3d live tree",
                "the elements on screen now, as `just b3d tree` prints them",
            ),
            (
                "just b3d live do 'press button \"Auto Pay\"'",
                "press that button; `live png` draws the result",
            ),
        ),
        live.run_live,
        live.live_arguments,
    ),
    Command(
        "release-socket",
        "builds the release app and fails if its binary names a live socket",
        (
            (
                "just b3d release-socket",
                "build release and look for the socket's file name",
            ),
            ("just b3d release-socket --help", "this text"),
        ),
        live.run_release_socket,
    ),
    Command(
        "stand-in",
        "plays a whole game against expert in the windowless app through its live socket",
        (
            (
                "just b3d stand-in",
                "build the app, play a game, quit and resume it mid-game",
            ),
            ("just b3d stand-in --no-build", "the app as last built"),
        ),
        stand_in.run_stand_in,
        stand_in.stand_in_arguments,
    ),
    Command(
        "smoke",
        "starts the app with no window and waits for its first frame and its start screen",
        (
            ("just b3d smoke", "build the app and see its start screen"),
            ("just b3d smoke --no-build", "the app as last built"),
        ),
        app.run_smoke,
        app.smoke_arguments,
    ),
    Command(
        "tiers",
        "the tier chosen at startup and its drop, in three runs of the app with no window",
        (
            ("just b3d tiers", "build the app and check the three runs' frame logs"),
            ("just b3d tiers --no-build", "the app as last built"),
        ),
        tiers.run_tiers,
        tiers.tiers_arguments,
    ),
    Command(
        "frames",
        "a frame log's summary and its 10 worst frames; the newest of this checkout if no path",
        (
            ("just b3d frames", "the newest log of a run from this checkout"),
            (
                "just b3d frames ~/Library/Caches/beryl/b3d/frames/1794743250854-11214.jsonl",
                "one log",
            ),
        ),
        app.run_frames,
        app.frames_arguments,
    ),
)

COMMANDS: tuple[Command, ...] = (
    *OWN_COMMANDS,
    *(command for module in MODULES for command in module.COMMANDS),
)


def command_entries() -> list[dict[str, str]]:
    """Names, descriptions and registration files for the inspector's command map."""
    registries = [(Path(__file__), OWN_COMMANDS)]
    for module in MODULES:
        source = module.__file__
        assert source is not None
        registries.append((Path(source), module.COMMANDS))
    return [
        {
            "name": command.name,
            "detail": command.summary,
            "path": path.resolve().relative_to(repo_paths.REPO).as_posix(),
        }
        for path, commands in registries
        for command in commands
    ]


def listing() -> str:
    width = max(len(c.name) for c in COMMANDS)
    lines = ["just b3d", *(f"  {c.name:<{width}}  {c.summary}" for c in COMMANDS)]
    lines.append("`just b3d <command> --help` prints a command's options and examples.")
    return "\n".join(lines)


def parser(command: Command) -> argparse.ArgumentParser:
    width = max(len(line) for line, _ in command.examples)
    examples = "\n".join(
        f"  {line:<{width}}  {what}" for line, what in command.examples
    )
    made = argparse.ArgumentParser(
        prog=f"just b3d {command.name}",
        description=command.summary,
        epilog=f"examples:\n{examples}",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    command.arguments(made)
    return made


def main(argv: list[str]) -> int:
    """Runs `just b3d` with the arguments after `b3d`."""
    if not argv or argv[0] in ("-h", "--help"):
        print(listing())
        return 0
    found = {c.name: c for c in COMMANDS}
    command = found.get(argv[0])
    if command is None:
        nearest = difflib.get_close_matches(argv[0], list(found), n=1, cutoff=0.0)
        print(
            f"just b3d: no command {argv[0]!r}; the nearest is {nearest[0]!r}."
            " `just b3d` lists them.",
            file=sys.stderr,
        )
        return USAGE
    if command.passthrough:
        return command.run(argparse.Namespace(arguments=argv[1:]))
    args = parser(command).parse_args(argv[1:])
    stream = sys.stderr if command.name == "gate" and args.json else sys.stdout
    print(shlex.join(["just", "b3d", *argv]), file=stream, flush=True)
    return command.run(args)


def b3d(args: argparse.Namespace) -> int:
    return main(list(args.extra))


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    # Every argument after `b3d` is this package's to parse, `--help` among them.
    p = sub.add_parser("b3d", help="Beryl 3D's commands", add_help=False)
    p.set_defaults(func=b3d, passthrough=True)
