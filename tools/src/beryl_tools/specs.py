"""`specs`: the cards' spec pages, every moment of a card's play a live game, and the tools an
agent reproduces, looks at, audits and drives a card's behavior with. It builds `beryl_cli`,
runs `beryl_cli specs-serve` and serves the UI with Vite, which proxies the spec server under
/spec-api:

- no flag: serves the pages for a human and prints their URLs until interrupted;
- `--check`: lists every generated moment and the candidates generation skipped;
- `--describe <card> [--moment M]`: what each moment shows from each seat, as text;
- `--views <card> [--moment M]`: each view shown from each seat, its events, stack and zones;
- `--film`: one sheet per moment of its playout from both seats (ui/scripts/film.mjs);
- `--motion`: audits that every event animates clearly for both seats (ui/scripts/motion.mjs);
- `--shots`: a settled screenshot of every moment from both seats (ui/scripts/specs.mjs);
- `--drive`: a headless page an agent posts Playwright snippets to (ui/scripts/drive.mjs),
  restarted with the spec server rebuilt when Rust changes, and itself when its scripts do.

Cards are named by slug or by name ("Teferi, Time Raveler"); a card with no spec gets the
specced cards nearest it in rules text. Each card's film and shots replace only that card's.
`--release` builds `beryl_cli` optimized and serves the UI's production build.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import cast

from . import card_store, cards, paths, scryfall, system, warm
from .shots import (
    UiError,
    free_port,
    port_free,
    served_ui,
    wait_for_server,
    browser_env,
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
        env=paths.run_env(),
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("specs: building beryl_cli failed", file=sys.stderr)
        return None
    return system.exe(
        paths.RUN_TARGET / ("release" if release else "debug") / "beryl_cli"
    )


def build_ui() -> bool:
    """Builds the UI's production bundle into ui/dist, for `vite preview`."""
    built = subprocess.run(
        [system.npm_bin("vite"), "build", "--logLevel", "warn"],
        cwd=paths.UI,
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if built.returncode != 0:
        print("specs: building the UI failed", file=sys.stderr)
    return built.returncode == 0


def slug(name: str) -> str:
    """A card name in a URL, as `beryl_cli`'s `specs::slug` makes it: `currency-converter`."""
    out = ""
    for c in name:
        if c.isascii() and c.isalnum():
            out += c.lower()
        elif c in " -" and out and not out.endswith("-"):
            out += "-"
    return out.rstrip("-")


CARD_SOURCES: Path = paths.RUST / "crates" / "beryl_cards" / "src"


def implemented() -> set[str]:
    """The slugs of the implemented cards: each card's file under beryl_cards, by its name."""
    return {
        f.stem.replace("_", "-")
        for f in CARD_SOURCES.glob("*/*.rs")
        if f.stem not in ("mod", "beryl_scenario")
    }


WORD: re.Pattern[str] = re.compile(r"[a-z+\-/{}0-9]+")
COMMON: frozenset[str] = frozenset(
    "a an the of to and or it its this that you your each target card cards from".split()
)


def words(text: str, name: str) -> set[str]:
    """A rules text's words and word pairs, less its card's name and the commonest words:
    what two cards' mechanics share."""
    text = text.lower().replace(name.lower(), "~")
    found = [w for w in WORD.findall(text) if w not in COMMON]
    return set(found) | {f"{a} {b}" for a, b in zip(found, found[1:])}


def rules_text(record: card_store.Json) -> str:
    """The rules text of each face of a card's record."""
    return "\n".join(
        str(f.get("oracle_text", ""))
        for f in cast(list[card_store.Json], record["faces"])
    )


def oracle(name: str) -> str | None:
    """A card's rules text: the card store's (by its name, or the slug of a deck card's), else
    Scryfall's; None offline or unknown."""
    with card_store.Store() as store:
        deck = {slug(n): n for n in cards.m1_names()}
        record = store.card(deck.get(slug(name), name))
    if record is not None:
        return rules_text(record)
    url = "https://api.scryfall.com/cards/named?fuzzy=" + urllib.parse.quote(name)
    try:
        found = scryfall.http_get(url)
    except (urllib.error.URLError, OSError, ValueError):
        return None
    faces = found.get("card_faces")
    if isinstance(faces, list):
        return "\n".join(str(f.get("oracle_text", "")) for f in faces)
    return str(found.get("oracle_text", ""))


def nearest(name: str, specced: set[str], most: int = 5) -> list[str]:
    """The specced cards whose rules text shares most with `name`'s, nearest first."""
    text = oracle(name)
    if text is None:
        return []
    mine = words(text, name)
    scored: list[tuple[float, str]] = []
    with card_store.Store() as store:
        # The decks' cards, asked of the store one by one: never the whole pool.
        for card in store.cards(cards.m1_names()):
            if slug(str(card["name"])) not in specced or slug(
                str(card["name"])
            ) == slug(name):
                continue
            theirs = words(rules_text(card), str(card["name"]))
            shared = len(mine & theirs) / max(1, len(mine | theirs))
            if shared > 0:
                scored.append((shared, str(card["name"])))
    return [n for _, n in sorted(scored, reverse=True)[:most]]


def resolve(names: list[str]) -> list[str] | None:
    """The slugs of `names` (slugs or card names); None, having said which have no spec and
    the specced cards nearest each in rules text, if any has none."""
    specced = implemented()
    slugs = [slug(n) for n in names]
    unknown = [n for n, s in zip(names, slugs) if s not in specced]
    for n in unknown:
        near = nearest(n, specced)
        hint = f"; nearest specced: {', '.join(near)}" if near else ""
        print(f"specs: no spec for {n}{hint}", file=sys.stderr)
    return None if unknown else slugs


def moment_args(args: argparse.Namespace) -> list[str]:
    return ["--moment", args.moment] if args.moment else []


def fresh(kind: str, slugs: list[str]) -> Path:
    """This checkout's output of `kind`, emptied of `slugs`' (of everything, if none): each
    card's output replaces only its own."""
    out = out_dir(kind)
    if not slugs:
        shutil.rmtree(out, ignore_errors=True)
    for s in slugs:
        shutil.rmtree(out / s, ignore_errors=True)
    out.mkdir(parents=True, exist_ok=True)
    return out


def capture(args: argparse.Namespace, url: str, env: dict[str, str]) -> int:
    """Runs the capture script the flags ask for against the served UI at `url`."""
    size = ["--viewport", args.viewport] if args.viewport else []
    if args.film:
        out = fresh("film", args.cards)
        command = [
            "scripts/film.mjs",
            url,
            str(out),
            *args.cards,
            *moment_args(args),
            *size,
        ]
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
            *size,
            *(["--minor"] if args.minor else []),
        ]
    else:
        out = fresh("shots", args.cards)
        command = [
            "scripts/specs.mjs",
            url,
            str(out),
            *args.cards,
            *moment_args(args),
            *size,
            *(["--select", args.select] if args.select else []),
            *(["--crop", args.crop] if args.crop else []),
        ]
    return subprocess.run(
        ["node", *command],
        cwd=paths.UI,
        env=env,
        stdin=subprocess.DEVNULL,
        check=False,
    ).returncode


def serve_specs(binary: Path, port: int) -> subprocess.Popen[bytes]:
    """Starts `beryl_cli specs-serve` on `port`. It runs from this checkout as `cargo run`
    would, so a bug report filed from a spec page names the commit (`bugs.rs`'s `commit`).
    """
    env = {
        **os.environ,
        "BERYL_FILE_BUGS": str(paths.REPO),
        "CARGO_MANIFEST_DIR": str(paths.REPO / "rust" / "crates" / "beryl_cli"),
    }
    return subprocess.Popen(
        [str(binary), "specs-serve", "--port", str(port)],
        cwd=paths.REPO,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        env=env,
    )


def warm_specs(port: int) -> None:
    """Asks the spec server for its listing, which generates every spec, so the first page
    after a start does not wait on it."""
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/specs", timeout=600):
                return
        except (urllib.error.URLError, OSError):
            time.sleep(0.5)


def newest(root: Path, suffix: str) -> float:
    """The latest modification time of the files under `root` ending in `suffix`."""
    latest = 0.0
    for folder, subfolders, files in os.walk(root):
        subfolders[:] = [d for d in subfolders if d not in ("target", "node_modules")]
        for name in files:
            if name.endswith(suffix):
                try:
                    latest = max(latest, os.stat(os.path.join(folder, name)).st_mtime)
                except OSError:
                    pass
    return latest


def drive(
    args: argparse.Namespace,
    url: str,
    env: dict[str, str],
    servers: list[subprocess.Popen[bytes]],
    spec_port: int,
    binary: Path,
) -> int:
    """Runs the driver at the spec pages until interrupted. A change to the Rust sources
    rebuilds beryl_cli and restarts the spec server on its port (the driver's next `open` gets
    the new game); a change to ui/scripts restarts the driver on its port; either exiting is
    started again."""
    out: Path = out_dir("drive") / f"{time.strftime('%Y%m%d-%H%M%S')}-{os.getpid()}"
    out.mkdir(parents=True)
    port: int = args.drive_port or free_port()
    address: str = f"http://127.0.0.1:{port}/"
    crates = paths.RUST / "crates"
    scripts = paths.UI / "scripts"

    def start_driver() -> subprocess.Popen[bytes]:
        driver = subprocess.Popen(
            ["node", "scripts/drive.mjs", str(port), f"{url}scenarios", str(out)],
            cwd=paths.UI,
            env=env,
            stdin=subprocess.DEVNULL,
        )
        if not wait_for_server(address, driver):
            raise UiError(f"the driver did not answer at {address}")
        return driver

    driver = start_driver()
    print(f"scenarios: {url}scenarios", flush=True)
    print(f"playground: {url}playground", flush=True)
    print(f"specs: drive {address} (POST a Playwright snippet)", flush=True)
    print(f"specs: screenshots {out}", flush=True)
    rust, js = newest(crates, ".rs"), newest(scripts, ".mjs")
    try:
        while True:
            time.sleep(2)
            changed = newest(crates, ".rs")
            if changed > rust or servers[-1].poll() is not None:
                rust = changed
                print("specs: rebuilding beryl_cli for a Rust change", flush=True)
                if system.WINDOWS:
                    # Windows replaces no running binary: the old server stops first.
                    servers[-1].terminate()
                    servers[-1].wait()
                built = build(args.release)
                if built is None and not system.WINDOWS:
                    print("specs: keeping the old spec server", flush=True)
                    continue
                if built is None:
                    print("specs: restarting the old spec server", flush=True)
                servers[-1].terminate()
                servers[-1].wait()
                binary = built or binary
                servers.append(serve_specs(binary, spec_port))
                warm_specs(spec_port)
                print("specs: spec server restarted; open moments again", flush=True)
            changed = newest(scripts, ".mjs")
            if changed > js or driver.poll() is not None:
                js = changed
                system.terminate(driver)
                driver.wait()
                driver = start_driver()
                print("specs: driver restarted; open moments again", flush=True)
    finally:
        system.terminate(driver)
    return 0


def specs(args: argparse.Namespace) -> int:
    warm.ensure()
    resolved = resolve(args.cards)
    if resolved is None:
        return 2
    args.cards = resolved
    binary = build(args.release)
    if binary is None:
        return 1
    if args.check:
        return subprocess.run(
            [str(binary), "specs-check", *args.cards],
            stdin=subprocess.DEVNULL,
            check=False,
        ).returncode
    if args.describe or args.views:
        if len(args.cards) != 1:
            print("specs: --describe and --views take one card", file=sys.stderr)
            return 2
        moment = [args.moment] if args.moment else []
        views = ["--views"] if args.views else []
        return subprocess.run(
            [str(binary), "specs-describe", *views, args.cards[0], *moment],
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
    servers = [serve_specs(binary, spec_port)]
    # A plain SIGTERM would end Python without the cleanup below, orphaning the server.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        if capturing or args.drive:
            env = {
                **browser_env(),
                "BERYL_SPECS_PORT": str(spec_port),
                "BERYL_CLI": str(binary),
            }
            port = None if capturing else args.port or None
            with served_ui(env, port, preview=args.release) as url:
                if args.drive:
                    return drive(args, url, env, servers, spec_port, binary)
                return capture(args, url, env)
        env = {**os.environ, "BERYL_SPECS_PORT": str(spec_port)}
        with served_ui(env, args.port or None, preview=args.release) as url:
            base = url.replace("127.0.0.1", "localhost")
            print(f"playground: {base}playground", flush=True)
            print(f"scenarios: {base}scenarios", flush=True)
            for card in args.cards:
                print(f"playground: {base}playground/{card}", flush=True)
            while servers[-1].poll() is None:
                time.sleep(0.5)
            raise UiError("the spec server exited")
    except UiError as e:
        print(f"specs: {e}", file=sys.stderr)
        return 1
    finally:
        servers[-1].terminate()


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "specs",
        aliases=["playground", "scenarios"],
        help="card specs: serve, describe, film, audit motion or drive each moment",
    )
    p.add_argument(
        "cards",
        nargs="*",
        help="card slugs or names, e.g. currency-converter or 'Teferi, Time Raveler'",
    )
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
        "--views",
        action="store_true",
        help="each view shown from each seat: its events, stack and zones",
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
        "--viewport", help="the pages' size for --shots, --film, --motion: 1180x820"
    )
    p.add_argument(
        "--select",
        help="--shots: only the seats where this selector shows, e.g. .prompt",
    )
    p.add_argument(
        "--crop", help="--shots: only this selector's element, or a region X,Y,W,H"
    )
    p.add_argument(
        "--minor", action="store_true", help="--motion: print minor findings too"
    )
    p.add_argument(
        "--release",
        action="store_true",
        help="optimized beryl_cli and the UI's production build",
    )
    p.set_defaults(func=specs)
