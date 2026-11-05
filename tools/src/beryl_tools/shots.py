"""`ui-shots`: screenshots of UI states without opening a window. It serves the UI with Vite,
captures the routes listed in ui/shots.json with headless Playwright WebKit (ui/scripts/shots.mjs)
and prints the paths. Pages run in a plain browser on the fixture transport (ui/src/ipc.ts).

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
from pathlib import Path

from . import paths, warm

SERVER_TIMEOUT = 60.0
NPM_BIN: Path = paths.UI / "node_modules" / ".bin"


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


def ui_shots(args: argparse.Namespace) -> int:
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
        print(install.stdout + install.stderr, file=sys.stderr)
        print("ui-shots: installing Playwright WebKit failed", file=sys.stderr)
        return install.returncode

    port = free_port()
    url = f"http://127.0.0.1:{port}/"
    out = out_dir()
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
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
                sys.stderr.write(log.read().decode(errors="replace"))
                print(f"ui-shots: Vite did not serve {url}", file=sys.stderr)
                return 1
            code = subprocess.run(
                ["node", "scripts/shots.mjs", url, str(out), "shots.json", *args.only],
                cwd=paths.UI,
                env=env,
                stdin=subprocess.DEVNULL,
                check=False,
            ).returncode
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
    if code != 0:
        print("ui-shots: FAILED", file=sys.stderr)
    return code


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("ui-shots", help="screenshot UI states headless (ui/shots.json)")
    p.add_argument("only", nargs="*", help="shot names; all of them by default")
    p.set_defaults(func=ui_shots)
