"""`ui-shots`: screenshots of UI states without opening a window. It records the game fixtures
ui/fixtures.json defines with `beryl_cli record-fixture`, serves the UI with Vite, captures the
routes listed in ui/shots.json with headless Playwright WebKit (ui/scripts/shots.mjs) and prints
the paths. Pages run in a plain browser on the fixture transport (ui/src/ipc.ts).

`ui-test` plays the game screen through a fixture the same way, dragging, clicking and hovering
as a player would (ui/scripts/interactions.mjs).

`ui-live` serves live games instead: `beryl_cli serve` runs the app's game commands, Vite serves
the UI against it (`?live=`, src/ipc.ts), and ui/scripts/drive.mjs holds a headless WebKit page
open on it that an agent drives one step at a time over HTTP, until interrupted.

Screenshots go to the cache (`ui-shots/<checkout>/`), never into the tree; WebKit is installed
there on first use.
"""

from __future__ import annotations

import argparse
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from . import paths, warm

SERVER_TIMEOUT = 60.0
NPM_BIN: Path = paths.UI / "node_modules" / ".bin"
# The game fixtures' definitions, and where they are recorded to: gitignored, served by Vite.
FIXTURES: Path = paths.UI / "fixtures.json"
RECORDED: Path = paths.UI / "public" / "fixtures"


class UiError(Exception):
    """Serving the UI or installing WebKit failed; the message says why."""


def out_dir() -> Path:
    """This checkout's screenshots: a fresh folder per run, so nothing stale is left there."""
    return paths.cache_root() / "ui-shots" / paths.REPO.name


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def wait_for_server(url: str, server: subprocess.Popen[bytes]) -> bool:
    """Polls until Vite answers, it exits, or the timeout passes."""
    deadline = time.monotonic() + SERVER_TIMEOUT
    while time.monotonic() < deadline and server.poll() is None:
        try:
            with urllib.request.urlopen(url, timeout=2):
                return True
        except (urllib.error.URLError, OSError):
            time.sleep(0.1)
    return False


def webkit_env() -> dict[str, str]:
    """The environment for the capture scripts, with Playwright WebKit installed in the cache."""
    warm.ensure()
    env = {
        **os.environ,
        "PLAYWRIGHT_BROWSERS_PATH": str(paths.cache_root() / "playwright"),
    }
    # A no-op once WebKit is there.
    install = subprocess.run(
        [str(NPM_BIN / "playwright"), "install", "webkit"],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )
    if install.returncode != 0:
        raise UiError(
            install.stdout + install.stderr + "installing Playwright WebKit failed"
        )
    return env


def record_fixtures(only: list[str] | None = None) -> int:
    """Plays the fixtures' seeded games (those named in `only`, or all) and records them into
    RECORDED, fresh each run."""
    shutil.rmtree(RECORDED, ignore_errors=True)
    return subprocess.run(
        ["cargo", "run", "--quiet", "--package", "beryl_cli", "--"]
        + ["record-fixture", str(FIXTURES), str(RECORDED)]
        + (only or []),
        cwd=paths.RUST,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        check=False,
    ).returncode


@contextmanager
def served_ui(env: dict[str, str]) -> Iterator[str]:
    """Serves ui/ with Vite on a free port for the duration; yields its base URL."""
    port = free_port()
    url = f"http://127.0.0.1:{port}/"
    with tempfile.TemporaryFile() as log:
        server = subprocess.Popen(
            [str(NPM_BIN / "vite"), "--host", "127.0.0.1", "--port", str(port)],
            cwd=paths.UI,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            if not wait_for_server(url, server):
                log.seek(0)
                raise UiError(
                    log.read().decode(errors="replace") + f"Vite did not serve {url}"
                )
            yield url
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()


def ui_shots(args: argparse.Namespace) -> int:
    out = out_dir()
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    try:
        env = webkit_env()
        code = record_fixtures()
        if code == 0:
            with served_ui(env) as url:
                code = subprocess.run(
                    ["node", "scripts/shots.mjs", url, str(out), "shots.json"]
                    + list(args.only),
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    check=False,
                ).returncode
    except UiError as e:
        print(f"ui-shots: {e}", file=sys.stderr)
        code = 1
    if code != 0:
        print("ui-shots: FAILED", file=sys.stderr)
    return code


# The fixtures ui/scripts/interactions.mjs plays.
INTERACTION_FIXTURES = [
    "greedy-turn-9",
    "opponent-turn",
    "combat-attack",
    "combat-block",
    "combat-blockers",
    "choice-scry",
    "choice-jace",
    "choice-pay",
    "hand-pair",
    "cast-kicker",
    "pay-default",
    "pay-other",
    "exiled-beneath",
]


def ui_test(args: argparse.Namespace) -> int:
    try:
        env = webkit_env()
        code = record_fixtures(INTERACTION_FIXTURES)
        if code == 0:
            with served_ui(env) as url:
                code = subprocess.run(
                    ["node", "scripts/interactions.mjs", url, str(RECORDED)],
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    check=False,
                ).returncode
    except UiError as e:
        print(f"ui-test: {e}", file=sys.stderr)
        code = 1
    if code != 0:
        print("ui-test: FAILED", file=sys.stderr)
    return code


def ui_live(args: argparse.Namespace) -> int:
    out = (
        paths.cache_root()
        / "ui-live"
        / paths.REPO.name
        / time.strftime("%Y%m%d-%H%M%S")
    )
    out.mkdir(parents=True)
    profile = ["--release"] if args.release else []
    built = subprocess.run(
        ["cargo", "build", "--quiet", "--package", "beryl_cli", *profile],
        cwd=paths.RUST,
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("ui-live: building beryl_cli failed", file=sys.stderr)
        return built.returncode
    binary = (
        paths.RUST / "target" / ("release" if args.release else "debug") / "beryl_cli"
    )
    game_port, drive_port = free_port(), args.port or free_port()
    children: list[subprocess.Popen[bytes]] = []
    try:
        env = webkit_env()
        children.append(
            subprocess.Popen(
                [str(binary), "serve", "--port", str(game_port)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
            )
        )
        with served_ui(env) as url:
            app = f"{url}?live=http://127.0.0.1:{game_port}#/"
            children.append(
                subprocess.Popen(
                    ["node", "scripts/drive.mjs", str(drive_port), app, str(out)]
                    + [str(args.width), str(args.height)],
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                )
            )
            drive = f"http://127.0.0.1:{drive_port}/"
            if not wait_for_server(drive, children[-1]):
                raise UiError(f"the driver did not answer at {drive}")
            print(f"ui-live: app {app}", flush=True)
            print(f"ui-live: drive {drive} (POST a Playwright snippet)", flush=True)
            print(f"ui-live: screenshots {out}", flush=True)
            while all(c.poll() is None for c in children):
                time.sleep(0.5)
            raise UiError("the game server or the driver exited")
    except UiError as e:
        print(f"ui-live: {e}", file=sys.stderr)
        return 1
    finally:
        for c in children:
            c.terminate()


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("ui-shots", help="screenshot UI states headless (ui/shots.json)")
    p.add_argument("only", nargs="*", help="shot names; all of them by default")
    p.set_defaults(func=ui_shots)
    p = sub.add_parser(
        "ui-test", help="play the game screen headless through a fixture"
    )
    p.set_defaults(func=ui_test)
    p = sub.add_parser(
        "ui-live",
        help="live games in a headless page an agent drives over HTTP, until interrupted",
    )
    p.add_argument(
        "--port", type=int, default=0, help="the driver's port; any free one"
    )
    p.add_argument("--width", type=int, default=1600)
    p.add_argument("--height", type=int, default=900)
    p.add_argument(
        "--release", action="store_true", help="optimized agents (`just dev` is debug)"
    )
    p.set_defaults(func=ui_live)
