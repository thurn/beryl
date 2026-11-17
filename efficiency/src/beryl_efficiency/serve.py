"""`just efficiency serve` and `just efficiency shot` (docs/efficiency-dashboard.md, "The
page"). The server listens on 127.0.0.1 only and answers `GET` only. Each address is a query:
`/bead/by-x.1?since=7d` is `efficiency bead by-x.1 --since 7d`, the same address with `.json`
appended is its JSON, and `section=1` returns the section alone, as the page's script fetches
it. `/` is the summary.

A request never waits for an ingest. The server answers from the store as it is, keeps each
answer until the next ingest, and refreshes the store beside the requests by running
`efficiency ingest` as a separate process at background priority whenever the store is over
five minutes old while the page is open. `/status.json` says when the store was last
updated and whether an ingest is running, which the page shows in its header.

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
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qsl, urlsplit

from . import cli, ingest, page, paths, store
from . import ci_views, views  # noqa: F401, it adds pages to page
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
        "ticket",
        "by",
        "script",
        "lane",
        "family",
        "rule",
    }
)
# The queries an address may name: every query that only reads the store.
ADDRESSABLE: frozenset[str] = (frozenset(cli.QUERIES) | {"ticket"}) - {"ingest", "sql"}
# How often the server looks at the store's age, in seconds.
POLL_SECONDS = 5.0
# How old the store may grow while the page is open before the server ingests, in seconds.
# Longer than a query's `cli.MAX_AGE`: each ingest and the answers it makes old cost CPU on
# a machine that is short of it.
REFRESH_SECONDS = 300.0
# How long after its last request the page counts as open, in seconds. Its script asks for
# `/status.json` every few seconds while it is open.
VIEWED_SECONDS = 60.0
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


def version() -> int | None:
    """The id of the store's last ingest: an answer computed at one version stays right
    until the next. None when the store is missing or has no ingest yet."""
    path = paths.store()
    if not path.exists():
        return None
    try:
        db = store.open_read_only(path)
        try:
            found = db.execute("SELECT max(id) FROM ingests").fetchone()
        finally:
            db.close()
    except sqlite3.Error:
        return None
    return int(found[0]) if found and found[0] is not None else None


def status(updating: bool) -> Answer:
    """When the store was last updated and whether an ingest is running, for the page's
    header."""
    updated: str | None = None
    path = paths.store()
    if path.exists():
        try:
            db = store.open_read_only(path)
            try:
                last = ingest.last_ingest(db)
            finally:
                db.close()
            updated = last.isoformat() if last else None
        except sqlite3.Error:
            updated = None
    body = {"version": version(), "updated": updated, "updating": updating}
    return Answer(HTTPStatus.OK, "application/json", json.dumps(body).encode())


def respond(path: str, query: str) -> Answer:
    """The page, section or JSON for an address, from the store as it is."""
    argv, as_json, section = argv_of(path, query)
    try:
        args = cli.parser().parse_args([*argv, "--no-ingest"])
    except SystemExit:
        raise Refused(HTTPStatus.BAD_REQUEST, f"bad query: {' '.join(argv)}")
    try:
        result = cli.QUERIES[args.command][1](args)
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
        result.query, found, result.fields, command if section else None
    )
    if not section:
        since = getattr(args, "since", None)
        shown = page.page(
            "/" + "/".join(p for p in path.split("/") if p),
            str(since) if since else None,
            shown,
            " ".join(argv[:2]),
            command,
            version(),
        )
    return Answer(HTTPStatus.OK, "text/html; charset=utf-8", shown.encode())


def low_priority() -> tuple[list[str], int]:
    """The command prefix and process creation flags that run a command at background
    priority, as the justfile's `low` does: `taskpolicy -c utility` on macOS, below-normal
    priority on Windows, and `nice` elsewhere."""
    if sys.platform == "win32":
        return [], int(getattr(subprocess, "BELOW_NORMAL_PRIORITY_CLASS", 0))
    if sys.platform == "darwin":
        return ["taskpolicy", "-c", "utility"], 0
    return ["nice", "-n", "10"], 0


class Answers:
    """Each address's answer, kept until the store's next ingest. When an ingest lands,
    `warm` computes again the last `WARM` addresses asked for, so a page that was open
    answers at once after the update too."""

    WARM = 16

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.version: int | None = None
        self.warmed: int | None = None
        self.found: dict[tuple[str, str], Answer] = {}
        # The addresses asked for, oldest first, as an ordered set.
        self.asked: dict[tuple[str, str], None] = {}

    def get(self, key: tuple[str, str], compute: Callable[[], Answer]) -> Answer:
        now = version()
        with self.lock:
            if now != self.version:
                self.version = now
                self.found = {}
            kept = self.found.get(key)
        if kept is not None:
            return kept
        answer = compute()
        if answer.status == HTTPStatus.OK:
            with self.lock:
                self.asked.pop(key, None)
                self.asked[key] = None
                while len(self.asked) > self.WARM:
                    del self.asked[next(iter(self.asked))]
                if self.version == now:
                    self.found[key] = answer
        return answer

    def warm(self) -> None:
        now = version()
        with self.lock:
            if now == self.warmed:
                return
            self.warmed = now
            asked = list(self.asked)
        for key in reversed(asked):
            self.get(key, lambda key=key: answer_of(*key))


def answer_of(path: str, query: str) -> Answer:
    try:
        return respond(path, query)
    except Refused as e:
        return Answer(e.status, "text/plain; charset=utf-8", str(e).encode())


class Refresher:
    """Runs `efficiency ingest` as a separate process at background priority whenever the
    page is open and the store is over `REFRESH_SECONDS` old, at most once per
    `REFRESH_SECONDS`, so a failing ingest is not started again at once."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.process: subprocess.Popen[bytes] | None = None
        self.started: float | None = None
        self.seen: float = time.monotonic()

    def viewed(self) -> None:
        self.seen = time.monotonic()

    def updating(self) -> bool:
        with self.lock:
            return self.process is not None and self.process.poll() is None

    def tick(self) -> None:
        with self.lock:
            if self.process is not None and self.process.poll() is None:
                return
            self.process = None
            now = time.monotonic()
            started = self.started
            recent = started is not None and now - started < REFRESH_SECONDS
            unviewed = now - self.seen > VIEWED_SECONDS
            if recent or unviewed or not ingest.stale(paths.store(), REFRESH_SECONDS):
                return
            prefix, flags = low_priority()
            self.started = time.monotonic()
            self.process = subprocess.Popen(
                [*prefix, sys.executable, "-m", "beryl_efficiency", "ingest"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=flags,
            )


class Handler(BaseHTTPRequestHandler):
    answers: Answers = Answers()
    refresher: Refresher | None = None

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
        refresher = self.refresher
        if refresher is not None:
            refresher.viewed()
        if found.path == "/status.json":
            self.send(status(refresher is not None and refresher.updating()))
            return
        key = (found.path, found.query)
        self.send(self.answers.get(key, lambda: answer_of(*key)))

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
    """A server on 127.0.0.1 at `port` (0 for any free port). It answers each connection on
    its own thread: a browser opens connections it sends nothing on until it needs them, and
    a server that answered one connection at a time would wait on such a connection while the
    page's own requests, such as its trends, waited behind it. Unless `no_ingest`, a thread
    beside it keeps the store fresh (`Refresher`) and computes again the answers an ingest
    made old (`Answers.warm`)."""

    class Bound(Handler):
        pass

    Bound.answers = Answers()
    Bound.refresher = None if no_ingest else Refresher()
    refresher: Refresher | None = Bound.refresher
    answers: Answers = Bound.answers
    if refresher is not None:
        running: Refresher = refresher

        def keep_fresh() -> None:
            while True:
                running.tick()
                answers.warm()
                time.sleep(POLL_SECONDS)

        threading.Thread(target=keep_fresh, daemon=True).start()
    return ThreadingHTTPServer((HOST, port), Bound)


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
