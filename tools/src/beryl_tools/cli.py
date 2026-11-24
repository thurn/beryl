"""`by`: Beryl's command line. `by --help` lists the commands; the justfile wraps them."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from functools import partial
from types import ModuleType

from . import (
    audio,
    bench,
    bugs,
    card,
    cards,
    census,
    citations,
    conventions,
    count_gate,
    disk_usage,
    edit,
    efficiency_merge,
    fseventsd,
    oracle_discovery,
    oracles,
    paths,
    set_plan,
    scale,
    system,
    video,
    vm_host,
    workspace_hack,
)
from .client import cli as client_cli


def utf8_rerun(argv: list[str] | None) -> int | None:
    """On Windows, outside Python's UTF-8 mode, runs `by` again in it and returns its exit
    code: there text files are read and written in the ANSI code page by default, which garbles
    card names. Its children inherit the mode. None when already in it."""
    if not system.WINDOWS or sys.flags.utf8_mode:
        return None
    os.environ["PYTHONUTF8"] = "1"
    args = sys.argv[1:] if argv is None else argv
    proc = subprocess.Popen(
        [sys.executable, "-X", "utf8", "-m", "beryl_tools.cli", *args]
    )
    while True:
        try:
            return proc.wait()
        except KeyboardInterrupt:
            continue  # The console sent Ctrl-C to the child too: it answers it.


MODULES: tuple[ModuleType, ...] = (
    audio,
    bench,
    client_cli,
    bugs,
    card,
    cards,
    census,
    citations,
    conventions,
    count_gate,
    disk_usage,
    edit,
    fseventsd,
    oracle_discovery,
    oracles,
    set_plan,
    scale,
    system,
    video,
    vm_host,
    workspace_hack,
)


# The public app commands share The client's argument parsers and runners.
NATIVE_APP_COMMANDS = {
    "dev": "app",
    "bundle": "release",
    "smoke": "smoke",
    "table-pair": "table-pair",
    "mobile-setup": "mobile-setup",
    "ios": "ios",
    "ios-smoke": "ios-smoke",
    "ios-device": "ios-device",
    "android": "android",
    "android-smoke": "android-smoke",
    "mobile-bugs": "mobile-bugs",
}


def run_native(command: str, args: argparse.Namespace) -> int:
    return client_cli.main([command, *args.extra])


def register_native(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    native = {command.name: command.summary for command in client_cli.COMMANDS}
    for name, target in NATIVE_APP_COMMANDS.items():
        parser = sub.add_parser(name, help=native[target], add_help=False)
        parser.set_defaults(func=partial(run_native, target), passthrough=True)


def commands() -> dict[str, str]:
    """The module whose function runs each subcommand, by the subcommand's name: the module
    that registers it, for a subcommand with subcommands of its own."""
    sub = argparse.ArgumentParser().add_subparsers()
    found: dict[str, str] = {}
    for module in MODULES:
        known = set(sub.choices)
        module.register(sub)
        for name in set(sub.choices) - known:
            func = sub.choices[name].get_default("func")
            owner = func.__module__ if func is not None else module.__name__
            found[name] = str(owner).rsplit(".", 1)[-1]
    register_native(sub)
    found.update({name: "cli" for name in NATIVE_APP_COMMANDS})
    return found


def main(argv: list[str] | None = None) -> int:
    rerun = utf8_rerun(argv)
    if rerun is not None:
        return rerun
    parser = argparse.ArgumentParser(prog="by", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for module in MODULES:
        module.register(sub)
    register_native(sub)
    # Commands marked `passthrough` hand arguments they don't know to another tool.
    args, extra = parser.parse_known_args(argv)
    if extra and not getattr(args, "passthrough", False):
        parser.error(f"unrecognized arguments: {' '.join(extra)}")
    args.extra = extra
    try:
        return int(args.func(args))
    except KeyboardInterrupt:
        return 130


def entry() -> int:
    """The `by` command: `main`, after the setup every command first makes sure of. Tests call
    `main`, so that this setup's git commands never meet a test's stand-in for subprocess.
    """
    # So that a rebase keeps both sides' new entries of data/efficiency-changes.json.
    efficiency_merge.install(paths.REPO)
    return main()


if __name__ == "__main__":
    sys.exit(entry())
