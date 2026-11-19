"""The `b3d-timeline-drift` exploration family of `ignite/checks/beryl3d.toml`, run through the
adapter `beryl3d.py` (docs/beryl-3d.md, "Timeline parity").

The `b3d-timeline` regression family compares Beryl 3D's timeline of each recorded fixture with
a committed copy of the web interface's, `data/beryl3d/timeline/web/<fixture>.jsonl`, so that a
change to the web interface runs none of Beryl 3D's checks (decision C17). This family finds
when that copy no longer matches the web interface: one check per fixture plays the recording
through `ui/scripts/timeline.mjs` and fails when any step of either seat differs from the copy,
naming the fixture, the first step that differs and the command that rewrites the copy. Ignite
files a ticket for each failure. Its `next` command yields every fixture with a digest of the
web timeline's sources, the recordings pack and the fixture's copy, so a fixture is played again
only after one of them changed.
"""

from __future__ import annotations

import hashlib
import json
import shutil
import tempfile
from collections.abc import Sequence
from pathlib import Path

from .. import paths
from ..beryl3d import timeline
from . import Result
from .beryl3d import b3d_text, fixtures, pack_name
from .rust import asked

TIMELINE_DRIFT = "b3d-timeline-drift"
# The longest a failure's message grows, in characters.
MESSAGE_LIMIT = 1500


def relative(file: Path) -> str:
    return file.relative_to(paths.REPO).as_posix()


def sources_digest() -> str:
    """A digest of every file the web timeline loads (`timeline.web_sources`)."""
    digest = hashlib.sha256()
    for file in timeline.web_sources(paths.REPO / "ui"):
        digest.update(relative(file).encode())
        digest.update(b"\0")
        digest.update(file.read_bytes())
    return digest.hexdigest()


def next_timeline_drift() -> list[dict[str, str]]:
    """Every fixture as a check, keyed by the web timeline's sources, the recordings pack and
    the fixture's committed copy, so that it is played again when any of them changed.
    """
    shared = [sources_digest(), pack_name()]
    out: list[dict[str, str]] = []
    for name in fixtures():
        copy = timeline.copy_path(name)
        digest = hashlib.sha256()
        for part in (*shared, copy.read_text() if copy.is_file() else ""):
            digest.update(part.encode())
            digest.update(b"\0")
        out.append({"check": f"{TIMELINE_DRIFT}:{name}", "digest": digest.hexdigest()})
    return out


def drift_one(name: str, live: timeline.Timelines) -> Result:
    """Whether the live web timeline of the fixture `name` (every recording's, in `live`) is
    its committed copy."""
    check = f"{TIMELINE_DRIFT}:{name}"
    copy = timeline.copy_shown(name)
    refresh = (
        "`just b3d timeline-parity --fixtures --refresh-web` rewrites the copy if the web "
        "change was meant, and `ignite test b3d-timeline` then says whether Beryl 3D still "
        "agrees with it"
    )
    committed = timeline.load_copy(name)
    if committed is None:
        return Result(check, "fail", f"{copy} is missing; {refresh}.")
    keys = sorted(
        k for k in live.keys() | committed.keys() if k.rsplit("/", 1)[0] == name
    )
    lines: list[str] = []
    for key in keys:
        found = timeline.drifted(live.get(key, []), committed.get(key, []))
        if found is None:
            continue
        lines += [
            f"{key}: {found.count} step(s) of the web interface's timeline differ from {copy}; "
            "the first:",
            f"  web now:   {json.dumps(found.web) if found.web else 'no such step'}",
            f"  committed: {json.dumps(found.native) if found.native else 'no such step'}",
        ]
    if not lines:
        return Result(check, "pass")
    lines.append(f"Reproduce: `just b3d timeline-parity {name} --fixtures`; {refresh}.")
    text = "\n".join(lines)
    if len(text) > MESSAGE_LIMIT:
        text = text[: MESSAGE_LIMIT - 1] + "…"
    return Result(check, "fail", text)


def run_timeline_drift(checks: Sequence[str]) -> list[Result]:
    """Plays every recording through the live web interface's timeline (`timeline.web_lines`,
    which keeps it in the cache by the sources' digest) and compares each wanted fixture's with
    its committed copy."""
    wanted = asked(TIMELINE_DRIFT, list(checks))
    if not wanted:
        return []
    directory = Path(tempfile.mkdtemp(prefix="b3d-timeline-drift-"))
    try:
        b3d_text(["export", str(directory)])
        live = timeline.parse(timeline.web_lines(pack_name(), directory))
    finally:
        shutil.rmtree(directory, ignore_errors=True)
    return [drift_one(name, live) for name in wanted]
