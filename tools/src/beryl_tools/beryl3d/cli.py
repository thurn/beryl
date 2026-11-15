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

from .. import system, warm
from . import app, gate, lane, paths, phone, sweep, vm

# Exit code for a command line `just b3d` cannot run, as argparse uses.
USAGE = 2


@dataclass(frozen=True)
class Command:
    name: str
    # The one line `just b3d` lists it with.
    summary: str
    # Two command lines, each with what it does, for `--help`.
    examples: tuple[tuple[str, str], tuple[str, str]]
    run: Callable[[argparse.Namespace], int]
    # Adds the command's own arguments to its parser.
    arguments: Callable[[argparse.ArgumentParser], None] = lambda _parser: None
    # Whether the command's arguments, `--help` among them, go unread to a program that parses
    # them and prints its own first line (the `b3d` program's text commands).
    passthrough: bool = False


def run_lane(_args: argparse.Namespace) -> int:
    from .. import checks  # It imports this package's lane.

    return checks.run_all([lane.lane()], "b3d", checks.FAST_BUDGET).code


def warm_target() -> None:
    """Before a worktree's first cargo command, clones `$BERYL_CACHE/b3d/target` into the
    workspace's `target` when the cache holds one and the workspace has none."""
    target = paths.WORKSPACE / "target"
    cached = paths.cache() / "target"
    if target.exists():
        return
    with system.file_lock(paths.target_lock()):
        if cached.is_dir():
            warm.clone_into(cached, target)


def cargo(arguments: list[str]) -> int:
    """Runs cargo with `arguments` in Beryl 3D's workspace."""
    warm_target()
    return subprocess.run(
        ["cargo", *arguments], cwd=paths.WORKSPACE, env=os.environ, check=False
    ).returncode


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
    # The tests read recordings, which only the `record` program can build.
    recorded = record()
    if recorded != 0:
        return recorded
    nextest = shutil.which("cargo-nextest") is not None
    return cargo(test_command(args.package, args.filter, nextest))


# The status `b3d` and `b3d-render` exit with when the recordings pack for the present sources
# is missing (`beryl_inspect::recordings::MISSING_STATUS`).
MISSING_PACK = 3


def record() -> int:
    """Builds the recordings pack for the present sources with the `record` program of
    `beryl_record` if it is missing. `b3d` does not link the engine that records, so that it
    links and starts fast after an edit (docs/beryl-3d.md, 'Edit-to-output time')."""
    return cargo(["run", "-q", "-p", "beryl_record", "--bin", "record"])


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
    return max(found, key=lambda p: p.stat().st_mtime)


def run_program(name: str, arguments: list[str]) -> int:
    """Runs the built program `name` with `arguments`. When it says the recordings pack is
    missing, builds the pack and runs it again."""
    line = [str(linked(name)), *arguments]
    code = subprocess.run(line, env=os.environ, check=False).returncode
    if code != MISSING_PACK:
        return code
    recorded = record()
    if recorded != 0:
        return recorded
    return subprocess.run(line, env=os.environ, check=False).returncode


def run_b3d(command: str) -> Callable[[argparse.Namespace], int]:
    """Builds the `b3d` program of `beryl_inspect` and runs its `command` with the arguments."""

    def run(args: argparse.Namespace) -> int:
        built = cargo(["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"])
        if built != 0:
            return built
        return run_program("b3d", [command, *args.arguments])

    return run


def b3d_command(
    name: str, summary: str, examples: tuple[tuple[str, str], tuple[str, str]]
) -> Command:
    """A text command of the `b3d` program, which reads its own options and `--help`."""
    return Command(name, summary, examples, run_b3d(name), passthrough=True)


def run_render(command: str) -> Callable[[argparse.Namespace], int]:
    """Builds the `b3d-render` program of `beryl_3d` and runs its `command` with the arguments."""

    def run(args: argparse.Namespace) -> int:
        built = cargo(
            [
                "build",
                "-q",
                "-p",
                "beryl_3d",
                "--features",
                "tools",
                "--bin",
                "b3d-render",
            ]
        )
        if built != 0:
            return built
        return run_program("b3d-render", [command, *args.arguments])

    return run


def render_command(
    name: str, summary: str, examples: tuple[tuple[str, str], tuple[str, str]]
) -> Command:
    """A picture command of the `b3d-render` program, which reads its own options and `--help`."""
    return Command(name, summary, examples, run_render(name), passthrough=True)


def run_clippy(_args: argparse.Namespace) -> int:
    return cargo(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])


def run_fmt(args: argparse.Namespace) -> int:
    return cargo(["fmt", "--all", *(["--check"] if args.check else [])])


COMMANDS: tuple[Command, ...] = (
    b3d_command(
        "list",
        "every recording whose name starts with a prefix, with its length and views",
        (
            ("just b3d list", "every recording"),
            ("just b3d list verbs", "the recordings whose names start with verbs"),
        ),
    ),
    b3d_command(
        "info",
        "a recording's moments: views, script steps and prompts with their buttons",
        (
            ("just b3d info verbs-zones", "every moment, from the first seat"),
            ("just b3d info verbs-zones --json", "the same as JSON"),
        ),
    ),
    b3d_command(
        "tree",
        "the elements on screen at one moment of a recording, as an indented tree",
        (
            ("just b3d tree verbs-zones --at 1500ms", "the tree 1.5 s in"),
            (
                "just b3d tree verbs-zones --at view:2 --within hand.yours",
                "your hand once view 2 is still",
            ),
        ),
    ),
    b3d_command(
        "diff",
        "the elements that appeared, left, moved or changed state between two moments",
        (
            (
                "just b3d diff verbs-zones --from 800ms --to 1500ms",
                "what changed between two times",
            ),
            (
                "just b3d diff verbs-zones --from view:1 --to view:1 --do 'hover card #8'",
                "what hovering a card changes",
            ),
        ),
    ),
    b3d_command(
        "check",
        "every interface check on every tick of a recording, and the faults",
        (
            ("just b3d check verbs-zones", "every tick, first seat, 1600x900"),
            (
                "just b3d check verbs-zones --seat opponent --size 1440x900",
                "the second seat in a smaller window",
            ),
        ),
    ),
    render_command(
        "png",
        "draws one moment of a recording offscreen to a PNG file, and with --tree its tree",
        (
            (
                "just b3d png verbs-zones --at 1500ms --tree",
                "the frame 1.5 s in, and its tree",
            ),
            (
                "just b3d png verbs-zones --at 1500ms --crop 'card \"Island\" #8'",
                "only the box around one element",
            ),
        ),
    ),
    render_command(
        "film",
        "draws up to 24 frames of a recording as a grid in one PNG, a line of text per frame",
        (
            (
                "just b3d film verbs-zones --from 0ms --to 3s --frames 12",
                "twelve frames over the first three seconds",
            ),
            (
                "just b3d film verbs-zones --from 800ms --to 1500ms --every 100ms --crop 'card #8'",
                "one card, followed every 100 ms",
            ),
        ),
    ),
    Command(
        "lane",
        "runs the beryl3d lane's steps, as the fast check does for a Beryl 3D change",
        (
            ("just b3d lane", "every step of the lane"),
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
            ("just b3d ios-smoke", "build, install and replay verbs-zones"),
            (
                "just b3d ios-smoke --fixture stress-255",
                "replay the 255-card stress board",
            ),
        ),
        phone.checked(phone.run_ios_smoke),
        phone.ios_smoke_arguments,
    ),
    Command(
        "android-smoke",
        "replays a recording in the app's Android host in a headless emulator, on Vulkan or GLES",
        (
            (
                "just b3d android-smoke",
                "build, install and replay verbs-zones on Vulkan",
            ),
            (
                "just b3d android-smoke --gles",
                "the same with Vulkan hidden: OpenGL ES 3.0",
            ),
        ),
        phone.checked(phone.run_android_smoke),
        phone.android_smoke_arguments,
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
