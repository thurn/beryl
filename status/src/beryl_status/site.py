"""The site's state: the beads as last exported, and the work that keeps the built files
current. The server and `publish` share it.

- `refresh` runs `bd export` (about two seconds) only when the database's files changed since
  the last export, then rebuilds.
- `fill` fetches the changelogs that are not current, a bead at a time at background
  priority, open beads first and the most recently updated first, and rebuilds as it goes so
  pages pick up their changelogs."""

from __future__ import annotations

import threading
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from cirrus.assets import Assets

from . import build, changelog, paths
from .beads import Bead, Index, export, parse
from .changelog import Changelog

# How often `fill` rebuilds while it works: after this many changelogs or this many seconds,
# whichever comes first.
FILL_BATCH = 40
FILL_SECONDS = 20.0


def say(message: str) -> None:
    """A log line, flushed at once: the server's output is often a file, which Python would
    otherwise hold in its buffer."""
    print(message, flush=True)


def signature(main: Path) -> tuple[float, ...]:
    """The database files' modification times; a change means the beads may have changed."""
    found: list[float] = []
    for path in paths.store_files(main):
        try:
            found.append(path.stat().st_mtime)
        except OSError:
            found.append(0.0)
    return tuple(found)


# Each changelog file read, by bead id, with the modification time it was read at, so a
# rebuild reads only the files `fill` wrote since the last one.
READ: dict[str, tuple[int, Changelog | None]] = {}


def lookup(bead: Bead) -> tuple[bool, Changelog | None]:
    """Whether the bead's cached changelog is current, and the cached copy, current or not."""
    path = changelog.cache_file(bead.id)
    try:
        stamp = path.stat().st_mtime_ns
        known = READ.get(bead.id)
        if known is not None and known[0] == stamp:
            log = known[1]
        else:
            log = changelog.from_json(path.read_text(encoding="utf-8"))
            READ[bead.id] = (stamp, log)
    except OSError:
        return False, None
    if log is None:
        return False, None
    return log.updated_at == bead.updated_at, log


def fill_order(index: Index) -> list[Bead]:
    """The beads whose changelog is not current, in the order `fill` fetches them."""
    stale = [b for b in index.beads.values() if not lookup(b)[0]]
    stale.sort(key=lambda b: b.updated_at, reverse=True)
    stale.sort(key=lambda b: b.closed)
    return stale


@dataclass
class Site:
    """The beads and the built site. A lock keeps one rebuild at a time."""

    main: Path
    folder: Path
    base: str = "/"
    index: Index = field(default_factory=lambda: Index({}))
    export_text: str = ""
    seen: tuple[float, ...] = ()
    version: str = ""
    lock: threading.Lock = field(default_factory=threading.Lock)
    pages: build.PageCache = field(default_factory=build.PageCache)
    log: Callable[[str], None] = say

    def refresh(self, force: bool = False) -> bool:
        """Exports and rebuilds if the database changed (or `force`); whether it did."""
        now = signature(self.main)
        if not force and now == self.seen and self.export_text:
            return False
        text = export(self.main)
        self.seen = now
        if text == self.export_text and not force:
            return False
        self.export_text = text
        self.index = Index.build(parse(text.splitlines()))
        self.rebuild()
        return True

    def rebuild(self) -> build.Built:
        with self.lock:
            started = time.monotonic()
            self.version = build.version_of(self.export_text, Assets.load())
            done = build.build(
                self.folder, self.index, self.version, lookup, self.base, self.pages
            )
            self.log(
                f"status: built {done.pages} pages, wrote {done.written} files "
                f"in {time.monotonic() - started:.1f}s"
            )
            return done

    def changelog_now(self, bead_id: str) -> Changelog | None:
        """The bead's changelog, fetched now if it is not current, with its fragment written:
        what the server does when a page asks for a fragment that is missing or old."""
        bead = self.index.beads.get(bead_id)
        if bead is None:
            return None
        log = changelog.current(self.main, bead)
        build.write_fragment(self.folder, bead, log, self.index)
        return log

    def fill(
        self, budget: float | None = None, stop: Callable[[], bool] = lambda: False
    ) -> int:
        """Fetches changelogs that are not current until none is left, `budget` seconds pass
        or `stop` says so, rebuilding as it goes; how many it fetched."""
        started = time.monotonic()
        last_build = started
        fetched = 0
        pending = 0
        for bead in fill_order(self.index):
            if stop() or (budget is not None and time.monotonic() - started > budget):
                break
            if self.index.beads.get(bead.id) != bead:
                continue  # A refresh changed it; the next fill sees the new one.
            try:
                changelog.current(self.main, bead)
            except RuntimeError as error:
                self.log(f"status: {error}")
                continue
            fetched += 1
            pending += 1
            if pending >= FILL_BATCH or time.monotonic() - last_build > FILL_SECONDS:
                self.rebuild()
                pending = 0
                last_build = time.monotonic()
        if pending:
            self.rebuild()
        return fetched
