"""`specs`: the cards' spec pages, every moment of a card's play a live game. It builds
`beryl_cli`, runs `beryl_cli specs-serve` and serves the UI with Vite, which proxies the spec
server under /spec-api, and prints the pages' URLs until interrupted. `--shots` instead
screenshots every moment headless into the cache (ui/scripts/specs.mjs) and prints the paths,
for an agent to judge; `--check` plays every moment headless and checks what it expects.
"""

from __future__ import annotations

import argparse
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

from . import paths, warm
from .shots import UiError, free_port, port_free, served_ui, webkit_env


def out_dir() -> Path:
    """This checkout's spec screenshots, fresh each run."""
    return paths.cache_root() / "spec-shots" / paths.REPO.name


def build() -> Path | None:
    """Builds beryl_cli; returns its binary, or None if the build failed."""
    built = subprocess.run(
        ["cargo", "build", "--quiet", "--package", "beryl_cli"],
        cwd=paths.RUST,
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("specs: building beryl_cli failed", file=sys.stderr)
        return None
    return paths.RUST / "target" / "debug" / "beryl_cli"


def specs(args: argparse.Namespace) -> int:
    warm.ensure()
    binary = build()
    if binary is None:
        return 1
    if args.check:
        return subprocess.run(
            [str(binary), "specs-check", *args.cards],
            stdin=subprocess.DEVNULL,
            check=False,
        ).returncode
    if args.port and not port_free(args.port):
        print(f"specs: port {args.port} is in use", file=sys.stderr)
        return 1
    spec_port = free_port()
    server = subprocess.Popen(
        [str(binary), "specs-serve", "--port", str(spec_port)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
    )
    # A plain SIGTERM would end Python without the cleanup below, orphaning the server.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        if args.shots:
            env = {**webkit_env(), "BERYL_SPECS_PORT": str(spec_port)}
            out = out_dir()
            shutil.rmtree(out, ignore_errors=True)
            out.mkdir(parents=True)
            with served_ui(env) as url:
                return subprocess.run(
                    ["node", "scripts/specs.mjs", url, str(out), *args.cards],
                    cwd=paths.UI,
                    env=env,
                    stdin=subprocess.DEVNULL,
                    check=False,
                ).returncode
        env = {**os.environ, "BERYL_SPECS_PORT": str(spec_port)}
        with served_ui(env, args.port or None) as url:
            base = url.replace("127.0.0.1", "localhost")
            print(f"specs: {base}specs", flush=True)
            for card in args.cards:
                print(f"specs: {base}specs/{card}", flush=True)
            while server.poll() is None:
                time.sleep(0.5)
            raise UiError("the spec server exited")
    except UiError as e:
        print(f"specs: {e}", file=sys.stderr)
        return 1
    finally:
        server.terminate()


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "specs", help="serve the cards' spec pages, each moment a live game"
    )
    p.add_argument("cards", nargs="*", help="card slugs, e.g. currency-converter")
    p.add_argument(
        "--port", type=int, default=1430, help="the pages' port (0: any free one)"
    )
    p.add_argument(
        "--shots", action="store_true", help="screenshot every moment headless"
    )
    p.add_argument(
        "--check", action="store_true", help="play every moment headless and check it"
    )
    p.set_defaults(func=specs)
