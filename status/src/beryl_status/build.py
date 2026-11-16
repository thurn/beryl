"""Building the site: every page is rendered, and a file is written only when its bytes
changed, so a rebuild after one bead changes rewrites a handful of files and leaves every
other page, and its cached copy in browsers, as it was.

The site's layout:
- `index.html`: the home page;
- `beads/<id>.html`: a bead's page, which servers answer at `beads/<id>`;
- `changelog/<id>.html`: a bead's changelog as a fragment, for pages whose changelog was not
  current when they were built;
- `assets/`: cirrus's content-hashed CSS and JS; `sw.js`: its service worker;
- `404.html`, `.nojekyll` (GitHub Pages serves the files as they are) and `version.json`,
  written last, which open pages poll to learn of a rebuild."""

from __future__ import annotations

import hashlib
import json
import os
import tempfile
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from cirrus.assets import Assets

from . import render
from .beads import Bead, Index
from .changelog import Changelog, shown

# Looks up a bead's cached changelog: whether it is current, and the copy itself, which may
# be older than the bead.
Lookup = Callable[[Bead], tuple[bool, Changelog | None]]


@dataclass
class PageCache:
    """What the last build wrote for each bead: a fingerprint of everything its page shows,
    so a rebuild skips rendering, reading and writing a page whose inputs did not change.
    It lives in memory, in the process that rebuilds (the server); a process that builds once
    starts empty and compares bytes on disk instead."""

    pages: dict[str, int] = field(default_factory=dict)
    fragments: dict[str, int] = field(default_factory=dict)


def summary(index: Index, bead_id: str) -> tuple[object, ...]:
    """What a page shows of a bead it links to: its row, hovercard and progress."""
    bead = index.beads[bead_id]
    children = index.children.get(bead_id, ())
    closed = sum(1 for i in children if index.beads[i].closed)
    working = sum(1 for i in children if index.beads[i].status == "in_progress")
    return (
        bead.title,
        bead.status,
        bead.priority,
        bead.issue_type,
        len(children),
        closed,
        working,
    )


def fingerprint(bead: Bead, index: Index, current: bool, log: Changelog | None) -> int:
    """A hash of every input of the bead's page: the bead, its changelog, and the summary of
    every bead the page links to, structurally or by a mention in its text."""
    linked = sorted(render.references(bead, index, log))
    return hash(
        (
            bead,
            current,
            log,
            index.parent.get(bead.id),
            tuple((i, summary(index, i)) for i in linked),
        )
    )


@dataclass
class Built:
    """What a build did: how many files it wrote and how many pages the site has."""

    written: int
    pages: int
    version: str


def write_if_changed(path: Path, text: str) -> bool:
    """Writes `text` to `path` in one step unless the file already holds it."""
    data = text.encode("utf-8")
    try:
        if path.read_bytes() == data:
            return False
    except OSError:
        pass
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    with os.fdopen(handle, "wb") as out:
        out.write(data)
    os.replace(temporary, path)
    return True


def version_of(export: str, assets: Assets) -> str:
    """The site's version: the export and the code's assets together, so a change to either
    tells open pages to refresh."""
    digest = hashlib.sha256(export.encode("utf-8"))
    for path in sorted(assets.paths.values()):
        digest.update(path.encode("utf-8"))
    return digest.hexdigest()[:16]


def build(
    site: Path,
    index: Index,
    version: str,
    lookup: Lookup,
    base: str = "/",
    cache: PageCache | None = None,
) -> Built:
    """Writes the site for `index` into `site`. `base` is the site's root as an absolute path,
    which only the 404 page uses."""
    assets = Assets.load()
    written = len(assets.write(site))
    written += write_if_changed(site / ".nojekyll", "")
    written += write_if_changed(site / "index.html", render.home(index, assets))
    written += write_if_changed(site / "404.html", render.not_found(assets, base))
    known = cache if cache is not None else PageCache()
    keep: set[str] = set()
    for bead in index.beads.values():
        current, log = lookup(bead)
        keep.add(bead.id)
        mark = fingerprint(bead, index, current, log)
        if known.pages.get(bead.id) != mark:
            page = render.bead_page(
                bead, index, assets, shown(bead, log) if current else None
            )
            written += write_if_changed(site / "beads" / f"{bead.id}.html", page)
            known.pages[bead.id] = mark
        if log is not None and known.fragments.get(bead.id) != mark:
            fragment = render.changelog_fragment(shown(bead, log) or log, index)
            written += write_if_changed(
                site / "changelog" / f"{bead.id}.html", fragment
            )
            known.fragments[bead.id] = mark
    for folder in ("beads", "changelog"):
        for stale in (site / folder).glob("*.html"):
            if stale.stem not in keep:
                stale.unlink()
                known.pages.pop(stale.stem, None)
                known.fragments.pop(stale.stem, None)
                written += 1
    built = datetime.now(timezone.utc).isoformat(timespec="seconds")
    version_file = site / "version.json"
    previous = read_version(version_file)
    if previous != version:
        write_if_changed(
            version_file, json.dumps({"version": version, "built": built}) + "\n"
        )
        written += 1
    return Built(written, len(index.beads) + 1, version)


def read_version(path: Path) -> str | None:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    value = raw.get("version") if isinstance(raw, dict) else None
    return value if isinstance(value, str) else None


def write_fragment(site: Path, bead: Bead, log: Changelog, index: Index) -> None:
    """Writes one bead's changelog fragment, as the server does when a page asks for one it
    has just fetched."""
    fragment = render.changelog_fragment(shown(bead, log) or log, index)
    write_if_changed(site / "changelog" / f"{bead.id}.html", fragment)
