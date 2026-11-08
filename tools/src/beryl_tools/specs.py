"""`specs`: the cards' spec pages, every moment of a card's play a live game, and the tools an
agent reproduces, looks at, audits and drives a card's behavior with. It builds `beryl_cli`,
runs `beryl_cli specs-serve` and serves the UI with Vite, which proxies the spec server under
/spec-api:

- no flag: serves the pages for a human and prints their URLs until interrupted;
- `--check`: lists every generated moment and the candidates generation skipped;
- `--describe <card> [--moment M]`: what each moment shows from each seat, as text;
- `--film`: one sheet per moment of its playout from both seats (ui/scripts/film.mjs);
- `--motion`: audits that every event animates clearly for both seats (ui/scripts/motion.mjs);
- `--shots`: a settled screenshot of every moment from both seats (ui/scripts/specs.mjs);
- `--drive`: a headless page an agent posts Playwright snippets to (ui/scripts/drive.mjs).

`--release` builds `beryl_cli` optimized and serves the UI's production build.
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
from .shots import (
    NPM_BIN,
    UiError,
    free_port,
    port_free,
    served_ui,
    wait_for_server,
    webkit_env,
)


def out_dir(kind: str) -> Path:
    """This checkout's output of `kind` (shots, film, motion, drive)."""
    return paths.cache_root() / f"spec-{kind}" / paths.REPO.name


def build(release: bool) -> Path | None:
    """Builds beryl_cli, optimized if `release`; returns its binary, or None if the build
    failed."""
    profile = ["--release"] if release else []
    built = subprocess.run(
        ["cargo", "build", "--quiet", *profile, "--package", "beryl_cli"],
        cwd=paths.RUST,
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("specs: building beryl_cli failed", file=sys.stderr)
        return None
    return paths.RUST / "target" / ("release" if release else "debug") / "beryl_cli"


def build_ui() -> bool:
    """Builds the UI's production bundle into ui/dist, for `vite preview`."""
    built = subprocess.run(
        [str(NPM_BIN / "vite"), "build", "--logLevel", "warn"],
        cwd=paths.UI,
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("specs: building the UI failed", file=sys.stderr)
    return built.returncode == 0


def moment_args(args: argparse.Namespace) -> list[str]:
    return ["--moment", args.moment] if args.moment else []


def capture(args: argparse.Namespace, url: str, env: dict[str, str]) -> int:
    """Runs the capture script the flags ask for against the served UI at `url`."""
    if args.film:
        out = out_dir("film")
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir(parents=True)
        command = ["scripts/film.mjs", url, str(out), *args.cards, *moment_args(args)]
    elif args.motion:
        out = out_dir("motion")
        out.mkdir(parents=True, exist_ok=True)
        report = out / f"{time.strftime('%Y%m%d-%H%M%S')}.json"
        command = [
            "scripts/motion.mjs",
            url,
            str(report),
            *args.cards,
            *moment_args(args),
        ]
    else:
        out = out_dir("shots")
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir(parents=True)
        command = ["scripts/specs.mjs", url, str(out), *args.cards]
    return subprocess.run(
        ["node", *command],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def drive(args: argparse.Namespace, url: str, env: dict[str, str]) -> int:
    """Runs the driver at the spec pages until it or the server exits."""
    out = out_dir("drive") / time.strftime("%Y%m%d-%H%M%S")
    out.mkdir(parents=True)
    port = args.drive_port or free_port()
    driver = subprocess.Popen(
        ["node", "scripts/drive.mjs", str(port), f"{url}specs", str(out)],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
    )
    try:
        address = f"http://127.0.0.1:{port}/"
        if not wait_for_server(address, driver):
            raise UiError(f"the driver did not answer at {address}")
        print(f"specs: pages {url}specs", flush=True)
        print(f"specs: drive {address} (POST a Playwright snippet)", flush=True)
        print(f"specs: screenshots {out}", flush=True)
        while driver.poll() is None:
            time.sleep(0.5)
        raise UiError("the driver exited")
    finally:
        driver.terminate()


def specs(args: argparse.Namespace) -> int:
    warm.ensure()
    binary = build(args.release)
    if binary is None:
        return 1
    if args.check:
        return subprocess.run(
            [str(binary), "specs-check", *args.cards],
            stdin=subprocess.DEVNULL,
            check=False,
        ).returncode
    if args.describe:
        if len(args.cards) != 1:
            print("specs: --describe takes one card", file=sys.stderr)
            return 2
        moment = [args.moment] if args.moment else []
        return subprocess.run(
            [str(binary), "specs-describe", args.cards[0], *moment],
            stdin=subprocess.DEVNULL,
            check=False,
        ).returncode
    capturing = args.shots or args.film or args.motion
    if args.port and not capturing and not port_free(args.port):
        print(f"specs: port {args.port} is in use", file=sys.stderr)
        return 1
    if args.release and not build_ui():
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
        if capturing or args.drive:
            env = {**webkit_env(), "BERYL_SPECS_PORT": str(spec_port)}
            port = None if capturing else args.port or None
            with served_ui(env, port, preview=args.release) as url:
                if args.drive:
                    return drive(args, url, env)
                return capture(args, url, env)
        env = {**os.environ, "BERYL_SPECS_PORT": str(spec_port)}
        with served_ui(env, args.port or None, preview=args.release) as url:
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
        "specs",
        help="card specs: serve, describe, film, audit motion or drive each moment",
    )
    p.add_argument("cards", nargs="*", help="card slugs, e.g. currency-converter")
    p.add_argument("--moment", help="only this moment, by its name")
    p.add_argument(
        "--port", type=int, default=1430, help="the pages' port (0: any free one)"
    )
    mode = p.add_mutually_exclusive_group()
    mode.add_argument(
        "--check", action="store_true", help="list every moment and the skipped ones"
    )
    mode.add_argument(
        "--describe", action="store_true", help="each moment from each seat, as text"
    )
    mode.add_argument(
        "--film", action="store_true", help="a sheet of each moment's playout"
    )
    mode.add_argument(
        "--motion",
        action="store_true",
        help="audit that every event animates clearly for both seats",
    )
    mode.add_argument(
        "--shots", action="store_true", help="screenshot every moment settled"
    )
    mode.add_argument(
        "--drive", action="store_true", help="a headless page to post snippets to"
    )
    p.add_argument(
        "--drive-port", type=int, default=0, help="the driver's port (default: any)"
    )
    p.add_argument(
        "--release",
        action="store_true",
        help="optimized beryl_cli and the UI's production build",
    )
    p.set_defaults(func=specs)
