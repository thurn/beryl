"""`just b3d`: Beryl 3D's commands. With no argument it lists them, one line each;
`just b3d <command> --help` prints a command's options and two examples.

Every command follows the rules in docs/beryl-3d.md ('The agent interface'): its output begins
with the command that reproduces it, is bounded, and names any file it writes for the run.
"""

from __future__ import annotations

import argparse
import difflib
import os
import re
import shlex
import shutil
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from .. import caches, system, tokens, warm
from .. import paths as repo_paths
from . import (
    app,
    camera,
    gate,
    ios_smoke,
    lane,
    live,
    paths,
    phone,
    phone_search,
    sweep,
    test_fit,
    timeline,
    vm,
)

# Exit code for a command line `just b3d` cannot run, as argparse uses.
USAGE = 2


@dataclass(frozen=True)
class Command:
    name: str
    # The one line `just b3d` lists it with.
    summary: str
    # Two command lines, each with what it does, for `--help`; none for a passthrough command,
    # whose program prints its own.
    examples: tuple[tuple[str, str], ...]
    run: Callable[[argparse.Namespace], int]
    # Adds the command's own arguments to its parser.
    arguments: Callable[[argparse.ArgumentParser], None] = lambda _parser: None
    # Whether the command's arguments, `--help` among them, go unread to a program that parses
    # them and prints its own first line (the `b3d` program's text commands).
    passthrough: bool = False


def run_lane(_args: argparse.Namespace) -> int:
    from .. import checks  # It imports this package's lane.

    return checks.run_all(lane.lanes(), "b3d", checks.FAST_BUDGET).code


def warm_target() -> None:
    """Before each cargo command, clones `$BERYL_CACHE/b3d/target` into the workspace's
    `target` when the cache holds one and the workspace has none, or else prunes the workspace's
    `target` when it is crowded, and sweeps the builds newer ones replaced every few hours
    (caches.tidy_target): a worktree that lives long held every artifact of every build it
    ran, 46-53 GB in each of five worktrees on 2026-11-16."""
    target = paths.WORKSPACE / "target"
    cached = paths.cache() / "target"
    if target.exists():
        caches.tidy_target(target)
        return
    with system.file_lock(paths.target_lock()):
        # Again under the lock: the sweep's steps start together, and each one that found no
        # target before the first clone landed cloned all 133,575 files again and threw the
        # copy away (five clones for one sweep on 2026-11-15).
        if cached.is_dir() and not target.exists() and warm.clone_into(cached, target):
            # Cargo rebuilds a workspace crate whose sources are newer than its last build, and
            # a new worktree's checkout wrote every file afresh, so without these two the first
            # `just b3d gate` in a new worktree rebuilt every crate of the workspace, and the
            # engine crates under rust/crates it depends on, for 10 to 30 minutes under load.
            # Give each file the cached build saw the time it had then, and point the build's
            # absolute paths into rust/crates at this checkout (warm.py, `restore_sources`).
            warm.restore_sources(repo_paths.REPO, target)
            warm.rehome_paths(target, repo_paths.REPO)
            # So that the builds' reads of units they do not rebuild count as uses, and
            # `warm.prune` and `target_sweep.sweep` keep those units (`warm.mark`).
            warm.mark(target)


def cargo(arguments: list[str], env: dict[str, str] | None = None) -> int:
    """Runs cargo with `arguments` in Beryl 3D's workspace, in `env` (this process's own by
    default)."""
    warm_target()
    return subprocess.run(
        ["cargo", *arguments],
        cwd=paths.WORKSPACE,
        env=os.environ if env is None else env,
        check=False,
    ).returncode


def build(arguments: list[str]) -> int:
    """Runs `cargo <arguments>`, a build in the workspace's own target directory, one at a time
    across every process of this checkout. Cargo's lock on the directory makes a second build
    wait while another runs, but the cargo shim takes its CPU tokens before cargo starts
    (cargo_pool.py), so the waiting build held a lease of up to two thirds of the pool while it
    did nothing. The fast check's Beryl 3D lanes start their builds together (lane.py), as do
    the sweep's steps. Here a build waits on a file lock first (`paths.build_lock`), holding
    no tokens, and then usually finds nothing left to do."""
    with system.file_lock(paths.build_lock()):
        return cargo(arguments)


def run_cargo(args: argparse.Namespace) -> int:
    return cargo(list(args.arguments))


def test_command(packages: list[str], filters: list[str], nextest: bool) -> list[str]:
    """The cargo arguments that test `packages` (the whole workspace for none) and the tests
    whose names contain any of `filters`: cargo-nextest where it is installed, else cargo
    test, as `just test-rust` chooses."""
    scope = [x for p in packages for x in ("-p", p)] or ["--workspace"]
    if nextest:
        return [
            *["nextest", "run", "--cargo-quiet", "--no-fail-fast", "--no-tests=pass"],
            *["--status-level=fail", "--final-status-level=fail", *scope, *filters],
        ]
    return ["test", "-q", *scope, *filters]


def test_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("-p", "--package", action="append", default=[])
    parser.add_argument("filter", nargs="*")


def run_cargo_test(args: argparse.Namespace) -> int:
    nextest = shutil.which("cargo-nextest") is not None
    # Every test of the workspace runs through test_fit.py, which logs each one's time and,
    # inside a fast check, runs only those that fit; its records are read while the tests build.
    whole = nextest and not args.package and not args.filter
    known = test_fit.known() if whole else None
    # The tests read recordings, which only the `record` program can build.
    recorded = record()
    if recorded != 0:
        return recorded
    if whole:
        return test_fit.run(known)
    return cargo(test_command(args.package, args.filter, nextest))


# The one build that the tests, `record` and `gate` share: every target that `cargo nextest run
# --workspace` builds, in cargo's `test` profile, and each program built as a program. Cargo
# resolves features for the packages a command names, so `cargo run -p beryl_record` and
# `cargo run -p beryl_inspect` each built `beryl_scene`, `beryl_inspect` and the engine with
# other features than the tests do, and the lane compiled them three times after an edit to
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


def record() -> int:
    """Builds the recordings pack for the present sources with the `record` program of
    `beryl_record` if it is missing. `b3d` does not link the engine that records, so that it
    links and starts fast after an edit (docs/beryl-3d.md, 'Edit-to-output time'). It prints
    the pack's path."""
    built = shared_build()
    if built != 0:
        return built
    line = [str(program("record"))]
    # `record` spreads its jobs over `runner::spread`, which adds threads as its lease grows.
    return tokens.run_program(
        line, grow=True, cwd=paths.WORKSPACE, check=False
    ).returncode


def run_record(_args: argparse.Namespace) -> int:
    return record()


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
    name: str, arguments: list[str], binary: Path | None = None, grow: bool = False
) -> int:
    """Runs the built program `name` (the file `binary`, or else `linked(name)`) with
    `arguments`, inside a lease of CPU tokens, since its checks spread over as many threads as
    the lease allows (`beryl_inspect::runner::threads`). With `grow`, for a command whose work
    goes through `runner::spread`, the lease grows while it runs (`tokens.run_program`). When
    it says the recordings pack is missing, builds the pack and runs it again."""
    line = [str(binary or linked(name)), *arguments]
    code = tokens.run_program(line, grow=grow, check=False).returncode
    if code != MISSING_PACK:
        return code
    recorded = record()
    if recorded != 0:
        return recorded
    return tokens.run_program(line, grow=grow, check=False).returncode


def run_b3d(command: str) -> Callable[[argparse.Namespace], int]:
    """Builds the `b3d` program of `beryl_inspect` and runs its `command` with the arguments."""

    def run(args: argparse.Namespace) -> int:
        built = build(["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"])
        if built != 0:
            return built
        return run_program("b3d", [command, *args.arguments])

    return run


def run_shared_b3d(command: str) -> Callable[[argparse.Namespace], int]:
    """Runs the `b3d` program that `shared_build` built with its `command` and the arguments.
    The fast check's `test` step runs this way. Building `b3d` alone (`run_b3d`) is a `dev`
    build with other features than the shared `test` build, so it compiled eight crates again
    (111 CPU-seconds and 22 s at load 11, with a warm target), and it held the checkout's build
    lock (`build`) meanwhile, so the `cargo-test` and `timeline-parity` lanes, whose first
    build finds nothing to do, waited behind it. In 12 of the 20 fast checks between 04:30
    and 06:10 UTC on 2026-11-16 whose shared build finished, `cargo-test` was cut off at the
    180 s limit after about 1 CPU-second of work. Both binaries ran every interface test in
    11-13 s on all cores, about 150 CPU-seconds each."""

    def run(args: argparse.Namespace) -> int:
        built = shared_build()
        if built != 0:
            return built
        # The shared commands (`test`) play their tests through `runner::spread`.
        return run_program("b3d", [command, *args.arguments], program("b3d"), grow=True)

    return run


def b3d_command(name: str, summary: str, shared: bool = False) -> Command:
    """A text command of the `b3d` program, which reads its own options and prints its own
    `--help` (`cli` in `beryl_inspect`). It runs the `b3d` that `shared_build` built when
    `shared` is set, and otherwise builds `b3d` alone, which relinks fastest after an edit.
    """
    run = run_shared_b3d(name) if shared else run_b3d(name)
    return Command(name, summary, (), run, passthrough=True)


def run_render(command: str) -> Callable[[argparse.Namespace], int]:
    """Builds the `b3d-render` program of `beryl_pictures` and runs its `command` with the
    arguments. Like `b3d`, it links no engine, so that it relinks and starts fast after an edit
    (docs/beryl-3d.md, 'Edit-to-output time')."""

    def run(args: argparse.Namespace) -> int:
        built = build(["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"])
        if built != 0:
            return built
        return run_program("b3d-render", [command, *args.arguments])

    return run


def render_command(name: str, summary: str) -> Command:
    """A picture command of the `b3d-render` program, which reads its own options and prints
    its own `--help` (`beryl_pictures`)."""
    return Command(name, summary, (), run_render(name), passthrough=True)


# Clippy's own target directory, nested in the workspace's as the main workspace's is
# (`paths.CLIPPY_TARGET`). Its check units are not the ones a build produces, so in the shared
# directory it would share no work with the build, only Cargo's lock on it, and the fast check's
# lanes could not run it beside the shared build (lane.py).
CLIPPY_TARGET: Path = paths.WORKSPACE / "target" / "clippy"


def run_clippy(_args: argparse.Namespace) -> int:
    return cargo(
        ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        {**os.environ, "CARGO_TARGET_DIR": str(CLIPPY_TARGET)},
    )


def run_fmt(args: argparse.Namespace) -> int:
    return cargo(["fmt", "--all", *(["--check"] if args.check else [])])


COMMANDS: tuple[Command, ...] = (
    b3d_command(
        "list",
        "every recording whose name starts with a prefix, with its length and views",
    ),
    b3d_command(
        "info",
        "a recording's moments: views, script steps and prompts with their buttons",
    ),
    b3d_command(
        "tree",
        "the elements on screen at one moment of a recording, as an indented tree",
    ),
    b3d_command(
        "diff",
        "the elements that appeared, left, moved or changed state between two moments",
    ),
    b3d_command(
        "check",
        "every interface check on every tick of a recording, and the faults",
    ),
    b3d_command(
        "test",
        "every interface test whose name starts with a prefix, at its check times",
        shared=True,
    ),
    b3d_command(
        "timeline",
        "when each step of each script of a recording begins and ends, as JSON lines",
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
    Command(
        "timeline-parity",
        "holds the web interface's and Beryl 3D's timelines of every recording to each other",
        (
            (
                "just b3d timeline-parity verbs-zones",
                "the recordings named verbs-zones",
            ),
            (
                "just b3d timeline-parity --fixtures",
                "the recorded fixtures only, as the lane does",
            ),
        ),
        lambda args: timeline.run_parity(args),
        timeline.parity_arguments,
    ),
    Command(
        "lane",
        "runs the beryl3d lanes' steps, as the fast check does for a Beryl 3D change",
        (
            ("just b3d lane", "every step of the lanes"),
            (
                "CHECK_HANG_LIMIT_S=600 just b3d lane",
                "every step, each allowed 600 s before it counts as hung",
            ),
        ),
        run_lane,
    ),
    Command(
        "sweep",
        "Beryl 3D's checks on origin/staging when its code or inputs changed, filing failures",
        (
            ("just b3d sweep", "sweep the staging tip if one is due"),
            (
                "just b3d sweep --dry-run --commit HEAD --step cargo-test",
                "run one step at HEAD and print the bead it would file",
            ),
        ),
        sweep.sweep_command,
        sweep.sweep_arguments,
    ),
    Command(
        "sweep-run",
        "runs the sweep's steps at HEAD, bisecting and filing failures (the sweep's tree)",
        (
            ("just b3d sweep-run --dry-run", "every step, printing what it would file"),
            (
                "just b3d sweep-run --step clippy --since origin/master",
                "one step, bisecting a failure from master",
            ),
        ),
        sweep.sweep_run,
        sweep.sweep_run_arguments,
    ),
    Command(
        "cargo",
        "runs cargo with the given arguments in rust/beryl3d",
        (
            ("just b3d cargo check -p beryl_scene", "type-check one crate"),
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
                "just b3d cargo-test -p beryl_scene pose",
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
    ),
    Command(
        "clippy",
        "runs clippy over every target of the workspace, with warnings as errors",
        (
            ("just b3d clippy", "lint the workspace"),
            (
                "just b3d cargo clippy -p beryl_scene",
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
            ("just b3d gate", "hold every gated board to data/beryl3d/gates.json"),
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
        "smoke",
        "starts the app with no window on verbs-zones and waits for its first frame",
        (
            ("just b3d smoke", "build the app and see its first frame"),
            ("just b3d smoke --no-build", "the app as last built"),
        ),
        app.run_smoke,
        app.smoke_arguments,
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
    Command(
        "ios-smoke",
        "replays a recording in the app's iOS host in the headless Simulator, and saves the screen",
        (
            (
                "just b3d ios-smoke --touch",
                "touch, rotate and idle a live game through a UI test",
            ),
            (
                "just b3d ios-smoke --lifecycle",
                "suspend and resume a replay, trim memory and exercise platform requests",
            ),
        ),
        phone.checked(ios_smoke.run_ios_smoke),
        ios_smoke.ios_smoke_arguments,
    ),
    Command(
        "android-smoke",
        "replays a recording in the app's Android host in a headless emulator, on Vulkan or GLES",
        (
            (
                "just b3d android-smoke --touch",
                "replay verbs-zones on Vulkan and check touch, Back, rotation and idle",
            ),
            (
                "just b3d android-smoke --lifecycle --gles",
                "check Android lifecycle and requests on OpenGL ES 3.0",
            ),
        ),
        phone.checked(phone.run_android_smoke),
        phone.android_smoke_arguments,
    ),
    Command(
        "phone-search",
        "plays a live game against mcts-hidden on the iOS Simulator and the Android emulator, and"
        " compares late frames while the AI searched and while it did not",
        (
            ("just b3d phone-search", "iOS, then Android on Vulkan"),
            (
                "just b3d phone-search --gles",
                "iOS, then Android on Vulkan and on OpenGL ES 3.0",
            ),
        ),
        phone.checked(phone_search.run_phone_search),
        phone_search.phone_search_arguments,
    ),
    Command(
        "ios-device",
        "installs the app's iOS host on the paired iPhone and replays two recordings, when asked",
        (
            (
                "just b3d ios-device --user-asked",
                "the user asked for it: install and replay",
            ),
            ("just b3d ios-device", "a person's run, which needs no flag"),
        ),
        phone.checked(phone.run_ios_device),
        phone.ios_device_arguments,
    ),
    Command(
        "camera-fit",
        "solves the desktop camera's pitch and field of view from cards measured in Arena",
        (
            ("just b3d camera-fit", "fit data/beryl3d/camera-fit.json"),
            (
                "just b3d camera-fit --file cards.json",
                "fit another file of measured cards",
            ),
        ),
        camera.run_camera_fit,
        camera.camera_fit_arguments,
    ),
    Command(
        "vm",
        "creates and provisions the beryl3d-bench count machine; runs a command there",
        (
            ("just b3d vm", "create, start and provision the machine"),
            (
                "just b3d vm vulkaninfo --summary",
                "run a command in the machine, from rust/",
            ),
        ),
        vm.run_vm,
        vm.vm_arguments,
    ),
)


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
    print(shlex.join(["just", "b3d", *argv]), flush=True)
    return command.run(args)


def b3d(args: argparse.Namespace) -> int:
    return main(list(args.extra))


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    # Every argument after `b3d` is this package's to parse, `--help` among them.
    p = sub.add_parser("b3d", help="Beryl 3D's commands", add_help=False)
    p.set_defaults(func=b3d, passthrough=True)
