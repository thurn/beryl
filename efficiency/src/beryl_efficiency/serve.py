"""`just efficiency serve` and `just efficiency shot` (docs/efficiency-dashboard.md, "The
page"). The server listens on 127.0.0.1 only and answers `GET` only. Each address is a query:
`/bead/by-x.1?since=7d` is `efficiency bead by-x.1 --since 7d`, the same address with `.json`
appended is its JSON, and `section=1` returns the section alone, as the page's script fetches
it. `/` is the summary. It changes nothing but the store, through the ingest every query runs
when the store is stale.

`shot` renders one address to a PNG in the cache with the headless browser the UI's capture
scripts use, taking one of the machine's shared browser slots. It is a development aid and is
part of no check."""

from __future__ import annotations

import argparse
import json
import os
import re
import sqlite3
import subprocess
import sys
import threading
from collections.abc import Sequence
from dataclasses import dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qsl, urlsplit

from . import cli, page, paths
from .beads import main_checkout

HOST = "127.0.0.1"
PORT = 8765
# The parameters an address may carry, each the command's flag of the same name.
FLAGS: frozenset[str] = frozenset(
    {
        "since",
        "where",
        "epic",
        "role",
        "tier",
        "host",
        "provider",
        "bead",
        "by",
        "script",
        "lane",
        "family",
    }
)
# The queries an address may name: every query that only reads the store.
ADDRESSABLE: frozenset[str] = frozenset(cli.QUERIES) - {"ingest", "sql"}
# Host headers the server answers, so that a page on another site cannot read it through a
# name that resolves to this machine.
LOCAL_NAMES: tuple[str, ...] = ("127.0.0.1", "localhost")


@dataclass
class Answer:
    status: int
    kind: str
    body: bytes


class Refused(Exception):
    def __init__(self, status: int, message: str) -> None:
        super().__init__(message)
        self.status = status


def argv_of(path: str, query: str) -> tuple[list[str], bool, bool]:
    """The command's arguments for an address, whether it asks for JSON, and whether for
    the section alone."""
    as_json = path.endswith(".json")
    if as_json:
        path = path[: -len(".json")]
    parts = [p for p in path.split("/") if p]
    if not parts:
        parts = ["summary"]
    name = parts[0]
    if name not in ADDRESSABLE:
        raise Refused(HTTPStatus.NOT_FOUND, f"no query {name}")
    argv = list(parts)
    section = False
    for key, value in parse_qsl(query, keep_blank_values=True):
        if key == "section":
            section = value not in ("", "0")
        elif key in FLAGS:
            argv += [f"--{key}", value]
        else:
            raise Refused(HTTPStatus.BAD_REQUEST, f"no parameter {key}")
    return argv, as_json, section


def command_of(argv: Sequence[str]) -> str:
    """The command a reader can run for the same result."""
    shown = [a if re.fullmatch(r"[\w.:=,/@+-]+", a) else repr(a) for a in argv]
    return "just efficiency " + " ".join(shown)


def respond(path: str, query: str, no_ingest: bool) -> Answer:
    """The page, section or JSON for an address."""
    argv, as_json, section = argv_of(path, query)
    try:
        args = cli.parser().parse_args([*argv, *(["--no-ingest"] if no_ingest else [])])
    except SystemExit:
        raise Refused(HTTPStatus.BAD_REQUEST, f"bad query: {' '.join(argv)}")
    try:
        result = cli.QUERIES[argv[0]][1](args)
    except argparse.ArgumentTypeError as e:
        raise Refused(HTTPStatus.BAD_REQUEST, str(e))
    except (sqlite3.Error, KeyError) as e:
        raise Refused(HTTPStatus.NOT_FOUND, str(e.args[0] if e.args else e))
    found = result.json()
    if as_json:
        return Answer(
            HTTPStatus.OK, "application/json", json.dumps(found, indent=1).encode()
        )
    if result.query not in page.RENDERERS:
        raise Refused(HTTPStatus.NOT_FOUND, f"no page for {result.query}; try .json")
    command = command_of(argv)
    shown = page.section(
        result.query, result.data, result.fields, command if section else None
    )
    if not section:
        since = getattr(args, "since", None)
        shown = page.page(
            "/" + "/".join(p for p in path.split("/") if p),
            str(since) if since else None,
            shown,
            " ".join(argv[:2]),
            command,
        )
    return Answer(HTTPStatus.OK, "text/html; charset=utf-8", shown.encode())


class Handler(BaseHTTPRequestHandler):
    no_ingest: bool = False

    def send(self, answer: Answer) -> None:
        self.send_response(answer.status)
        self.send_header("Content-Type", answer.kind)
        self.send_header("Content-Length", str(len(answer.body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(answer.body)

    def do_GET(self) -> None:  # noqa: N802, the name http.server calls
        host = (self.headers.get("Host") or "").rsplit(":", 1)[0]
        if host not in LOCAL_NAMES:
            self.send(Answer(HTTPStatus.FORBIDDEN, "text/plain", b"local only"))
            return
        found = urlsplit(self.path)
        try:
            answer = respond(found.path, found.query, self.no_ingest)
        except Refused as e:
            answer = Answer(e.status, "text/plain; charset=utf-8", str(e).encode())
        self.send(answer)

    def refuse(self) -> None:
        self.send_response(HTTPStatus.METHOD_NOT_ALLOWED)
        self.send_header("Allow", "GET")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self) -> None:  # noqa: N802
        self.refuse()

    def do_PUT(self) -> None:  # noqa: N802
        self.refuse()

    def do_DELETE(self) -> None:  # noqa: N802
        self.refuse()

    def do_PATCH(self) -> None:  # noqa: N802
        self.refuse()

    def do_HEAD(self) -> None:  # noqa: N802
        self.refuse()

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.refuse()

    def log_message(self, format: str, *args: object) -> None:
        pass


def server(port: int, no_ingest: bool = False) -> HTTPServer:
    """A server on 127.0.0.1 at `port` (0 for any free port)."""

    class Bound(Handler):
        pass

    Bound.no_ingest = no_ingest
    return HTTPServer((HOST, port), Bound)


def serve(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="just efficiency serve", description="serve the page on 127.0.0.1"
    )
    parser.add_argument("--port", type=int, default=PORT)
    parser.add_argument(
        "--no-ingest", action="store_true", help="answer from the store as it is"
    )
    args = parser.parse_args(argv)
    found = server(int(args.port), bool(args.no_ingest))
    print(f"efficiency: http://{HOST}:{found.server_address[1]}/", flush=True)
    try:
        found.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        found.server_close()
    return 0


# The capture script, run by Node with the UI's Playwright.
SHOT_SCRIPT: Path = Path(__file__).with_name("shot.mjs")


def ui_folder() -> Path:
    """The UI folder whose `node_modules` holds Playwright: the main checkout's, which the
    worktrees' copies come from."""
    found = main_checkout(Path.cwd()) / "ui"
    if not (found / "node_modules" / "playwright").exists():
        raise SystemExit(f"efficiency: no Playwright in {found}/node_modules")
    return found


def shot(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="just efficiency shot",
        description="render one address of the page to a PNG in the cache",
    )
    parser.add_argument("address", help="for example / or /bead/by-x.1?since=7d")
    parser.add_argument("--dark", action="store_true")
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument(
        "--no-ingest", action="store_true", help="answer from the store as it is"
    )
    args = parser.parse_args(argv)
    if not args.no_ingest:
        cli.update()  # Once, so that the page's requests answer at once.
    running = server(0, True)
    thread = threading.Thread(target=running.serve_forever, daemon=True)
    thread.start()
    address = str(args.address)
    if not address.startswith("/"):
        address = "/" + address
    slug = re.sub(r"[^\w.-]+", "-", address).strip("-") or "summary"
    out = paths.cache_root() / "efficiency" / "shots"
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"{slug}-{args.width}-{'dark' if args.dark else 'light'}.png"
    cache = paths.cache_root()
    env = {
        **os.environ,
        # The same places ui/scripts' capture runs use (tools' shots.browser_env).
        "PLAYWRIGHT_BROWSERS_PATH": str(cache / "playwright"),
        "BERYL_BROWSER_SLOTS_DIR": str(cache / "browser-slots"),
    }
    try:
        ran = subprocess.run(
            [
                "node",
                str(SHOT_SCRIPT),
                str(ui_folder()),
                f"http://{HOST}:{running.server_address[1]}{address}",
                str(target),
                str(args.width),
                "dark" if args.dark else "light",
            ],
            env=env,
            check=False,
        )
    finally:
        running.shutdown()
        running.server_close()
    if ran.returncode != 0:
        print("efficiency: the shot failed", file=sys.stderr)
        return 1
    print(target)
    return 0
