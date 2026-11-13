"""The desktop app: `dev` opens it with hot reload for humans; `smoke` builds it and launches it
hidden (no window, no Dock icon) until the UI reports that it loaded; `bundle` makes the
release .app, or on Windows its installer, playing on the stage."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

from . import checks, paths, system, warm

BINARY: Path = system.exe(paths.TAURI_TARGET / "debug" / "beryl")


def tauri(*args: str) -> int:
    warm.ensure(tauri=True)
    env = paths.tauri_env()
    if args and args[0] == "dev":
        env["BERYL_FILE_BUGS"] = str(paths.REPO)
    if args[:2] == ("build", "--bundles"):
        # Launching the packaged app from Finder drops the terminal's environment.
        uv, bd, dolt = (shutil.which(tool) for tool in ("uv", "bd", "dolt"))
        if uv and bd:
            tool_dirs = dict.fromkeys(
                str(Path(tool).absolute().parent) for tool in (uv, bd, dolt) if tool
            )
            env.update(
                BERYL_BUNDLE_BUGS_REPO=str(paths.REPO),
                BERYL_BUNDLE_BUGS_UV=str(Path(uv).absolute()),
                BERYL_BUNDLE_BUGS_PATH=os.pathsep.join([*tool_dirs, os.defpath]),
            )
    return subprocess.run(
        [system.which("npx"), "--no-install", "tauri", *args],
        cwd=paths.UI,
        env=env,
        check=False,
    ).returncode


def dev(args: argparse.Namespace) -> int:
    if paths.is_agent() and not args.window:
        print(
            "dev: agents never open a window; `just smoke` builds and launches the app headless",
            file=sys.stderr,
        )
        return 2
    return tauri("dev")


def smoke(args: argparse.Namespace) -> int:
    if not args.no_build:
        if tauri("build", "--debug", "--no-bundle") != 0:
            return checks.CANNOT_RUN
    # The app exits as soon as its UI reports that it loaded. `--seconds` only bounds a hang:
    # the app ends itself with an error then, and the timeout here catches one that cannot.
    try:
        run = subprocess.run(
            [str(BINARY)],
            env={**os.environ, "BERYL_SMOKE": str(args.seconds)},
            stdin=subprocess.DEVNULL,
            check=False,
            timeout=args.seconds + 30,
        )
    except subprocess.TimeoutExpired:
        print(
            f"smoke: FAILED: the app appears hung, it did not exit in {args.seconds} s"
        )
        return 1
    print(f"smoke: {'passed' if run.returncode == 0 else 'FAILED'}")
    return run.returncode


def bundle(args: argparse.Namespace) -> int:
    kind, folder = ("nsis", "nsis") if system.WINDOWS else ("app", "macos")
    code = tauri("build", "--bundles", kind)
    if code == 0:
        print(f"bundle: {paths.TAURI_TARGET / 'release' / 'bundle' / folder}")
        if system.WINDOWS:
            print(f"bundle: {system.exe(paths.TAURI_TARGET / 'release' / 'beryl')}")
    return code


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("dev", help="the app with hot reload, in a window (humans)")
    p.add_argument("--window", action="store_true", help="open it even for an agent")
    p.set_defaults(func=dev)
    p = sub.add_parser(
        "smoke", help="build and launch the app headless until the UI loads"
    )
    p.add_argument(
        "--seconds",
        type=int,
        default=300,
        help="treat the app as hung after this long; a healthy start exits much sooner",
    )
    p.add_argument("--no-build", action="store_true")
    p.set_defaults(func=smoke)
    sub.add_parser(
        "bundle", help="the release Beryl.app, or Windows installer"
    ).set_defaults(func=bundle)
