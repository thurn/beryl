"""`ui-shots`: screenshots of UI states without opening a window. It records the game fixtures
ui/fixtures.json defines with `beryl_cli record-fixture`, serves the UI with Vite, captures the
routes listed in ui/shots.json with headless Playwright WebKit (ui/scripts/shots.mjs) and prints
the paths. Pages run in a plain browser on the fixture transport (ui/src/ipc.ts).

Screenshots go to the cache (`ui-shots/<checkout>/`), never into the tree; WebKit is installed
there on first use. The serving and the replica art are shared with the mockup harness
(mockups.py).
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
# The mockups' parts and art windows, and where the art windows are cut to: gitignored, served by
# Vite in development only (src/replica/).
REGIONS: Path = paths.REPO / "docs" / "ui_mockups" / "regions.json"
REPLICA_ART: Path = paths.UI / "replica-art"
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


def cut_art(env: dict[str, str]) -> int:
    """Cuts each card's art window out of the mockups into REPLICA_ART, for the replicas."""
    shutil.rmtree(REPLICA_ART, ignore_errors=True)
    return subprocess.run(
        ["node", "scripts/mockups.mjs", "art", str(REGIONS), str(REPLICA_ART)],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def record_fixtures() -> int:
    """Plays the fixtures' seeded games and records them into RECORDED, fresh each run."""
    shutil.rmtree(RECORDED, ignore_errors=True)
    return subprocess.run(
        ["cargo", "run", "--quiet", "--package", "beryl_cli", "--"]
        + ["record-fixture", str(FIXTURES), str(RECORDED)],
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
        # The replica shots draw the art windows cut from the mockups; the game shots replay
        # recorded fixtures.
        code = cut_art(env) or record_fixtures()
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


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("ui-shots", help="screenshot UI states headless (ui/shots.json)")
    p.add_argument("only", nargs="*", help="shot names; all of them by default")
    p.set_defaults(func=ui_shots)
