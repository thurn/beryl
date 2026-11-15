"""`just b3d`: Beryl 3D's commands. With no argument it lists them, one line each;
`just b3d <command> --help` prints a command's options and two examples.

Every command follows the rules in docs/beryl-3d.md ('The agent interface'): its output begins
with the command that reproduces it, is bounded, and names any file it writes for the run.
"""

from __future__ import annotations

import argparse
import difflib
import os
import shlex
import shutil
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass

from .. import system, warm
from . import gate, lane, paths, sweep, vm

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
    nextest = shutil.which("cargo-nextest") is not None
    return cargo(test_command(args.package, args.filter, nextest))


def run_clippy(_args: argparse.Namespace) -> int:
    return cargo(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])


def run_fmt(args: argparse.Namespace) -> int:
    return cargo(["fmt", "--all", *(["--check"] if args.check else [])])


COMMANDS: tuple[Command, ...] = (
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
    args = parser(command).parse_args(argv[1:])
    print(shlex.join(["just", "b3d", *argv]), flush=True)
    return command.run(args)


def b3d(args: argparse.Namespace) -> int:
    return main(list(args.extra))


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    # Every argument after `b3d` is this package's to parse, `--help` among them.
    p = sub.add_parser("b3d", help="Beryl 3D's commands", add_help=False)
    p.set_defaults(func=b3d, passthrough=True)
