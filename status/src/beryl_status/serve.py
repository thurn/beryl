"""`just status`: the built site on 127.0.0.1, kept current while it runs.

The server answers from the files as they are the moment it starts, so a page shows at once
with what the last build left, and a thread brings the site up to date behind it. Each file
carries an ETag, so a browser's revalidation costs a 304; content-hashed assets are cached
for a year. A changelog fragment that is missing or older than its bead is fetched from `bd`
when a page asks for it."""

from __future__ import annotations

import argparse
import hashlib
import io
import re
import threading
import time
from collections.abc import Sequence
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit

from . import paths
from .site import Site

HOST = "127.0.0.1"
# How often the watcher looks at the database's files, and how long `fill` rests when every
# changelog is current.
WATCH_SECONDS = 3.0
FILL_REST_SECONDS = 30.0
TYPES: dict[str, str] = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".json": "application/json",
}
HASHED: re.Pattern[str] = re.compile(r"\.[0-9a-f]{10}\.(css|js)$")
FRAGMENT: re.Pattern[str] = re.compile(r"^changelog/([A-Za-z0-9._-]+)\.html$")


def resolve(folder: Path, path: str) -> Path | None:
    """The file an address names: `/` is `index.html`, and an address without an extension is
    its `.html` file, as GitHub Pages serves them. None for anything outside the site.
    """
    relative = unquote(urlsplit(path).path).lstrip("/")
    if relative == "" or relative.endswith("/"):
        relative += "index.html"
    candidate = (folder / relative).resolve()
    root = folder.resolve()
    if root not in candidate.parents and candidate != root:
        return None
    if candidate.is_file():
        return candidate
    html = candidate.with_name(candidate.name + ".html")
    return html if html.is_file() else None


class Server(ThreadingHTTPServer):
    """A server on 127.0.0.1, one thread per connection, because a browser holds connections
    open that it sends nothing on. It holds the site its handlers answer from."""

    def __init__(self, site: Site, port: int) -> None:
        super().__init__((HOST, port), Handler)
        self.site = site


class Handler(BaseHTTPRequestHandler):
    @property
    def site(self) -> Site:
        server = self.server
        assert isinstance(server, Server)
        return server.site

    def do_GET(self) -> None:  # noqa: N802, the name http.server calls
        relative = unquote(urlsplit(self.path).path).lstrip("/")
        if (m := FRAGMENT.match(relative)) is not None:
            self.ensure_fragment(m.group(1))
        found = resolve(self.site.folder, self.path)
        if found is None:
            missing = self.site.folder / "404.html"
            body = missing.read_bytes() if missing.is_file() else b"Not found"
            self.reply(404, body, TYPES[".html"], "no-cache")
            return
        data = found.read_bytes()
        etag = '"' + hashlib.sha1(data).hexdigest()[:20] + '"'
        if found.name == "version.json":
            policy = "no-store"
        elif HASHED.search(found.name):
            policy = "public, max-age=31536000, immutable"
        else:
            policy = "no-cache"
        if self.headers.get("If-None-Match") == etag:
            self.send_response(304)
            self.send_header("ETag", etag)
            self.send_header("Cache-Control", policy)
            self.end_headers()
            return
        kind = TYPES.get(found.suffix, "application/octet-stream")
        self.reply(200, data, kind, policy, etag)

    def ensure_fragment(self, bead_id: str) -> None:
        bead = self.site.index.beads.get(bead_id)
        if bead is None:
            return
        try:
            self.site.changelog_now(bead_id)
        except RuntimeError as error:
            self.site.log(f"status: {error}")

    def reply(
        self, code: int, body: bytes, kind: str, policy: str, etag: str | None = None
    ) -> None:
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", policy)
        if etag:
            self.send_header("ETag", etag)
        self.end_headers()
        self.wfile.write(body)

    def do_HEAD(self) -> None:  # noqa: N802
        """GET without the body, for tools that check an address exists."""
        real = self.wfile
        self.wfile = io.BytesIO()
        try:
            self.do_GET()
        finally:
            self.wfile = real

    def refuse(self) -> None:
        self.reply(405, b"Read only", "text/plain", "no-store")

    def do_POST(self) -> None:  # noqa: N802
        self.refuse()

    def do_PUT(self) -> None:  # noqa: N802
        self.refuse()

    def do_DELETE(self) -> None:  # noqa: N802
        self.refuse()

    def log_message(self, format: str, *args: object) -> None:
        pass


def keep_current(site: Site) -> None:
    """Rebuilds when the database changes, and fills changelogs in the time between."""

    def watch() -> None:
        while True:
            try:
                site.refresh()
            except Exception as error:  # A failed export must not stop the watcher.
                site.log(f"status: refresh failed: {error}")
            time.sleep(WATCH_SECONDS)

    def fill() -> None:
        while True:
            if site.export_text:
                site.fill()
            time.sleep(FILL_REST_SECONDS)

    threading.Thread(target=watch, daemon=True).start()
    threading.Thread(target=fill, daemon=True).start()


def serve(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="just status", description="serve the bead status site on 127.0.0.1"
    )
    parser.add_argument("--port", type=int, default=paths.PORT)
    parser.add_argument(
        "--no-refresh",
        action="store_true",
        help="serve the site as last built, without reading the beads",
    )
    args = parser.parse_args(argv)
    site = Site(paths.main_checkout(), paths.site())
    site.folder.mkdir(parents=True, exist_ok=True)
    if not args.no_refresh:
        if not (site.folder / "index.html").is_file():
            site.refresh(
                force=True
            )  # The first run has nothing to show until it builds.
        keep_current(site)
    found = Server(site, int(args.port))
    print(f"status: http://{HOST}:{found.server_address[1]}/", flush=True)
    try:
        found.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        found.server_close()
    return 0
