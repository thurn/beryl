"""`coverage`: which source lines each browser check runs (plan K19; docs/project-50k.md, Target
architecture, Checks, "Selection"), for choosing the checks a diff can break.

It plays every ui-test check and every shot in Chromium, one page each, with the scripts' own
`coverage` hook (ui/scripts/capture.mjs) recording V8's block coverage of the modules under
ui/src and the CSS rules each page used. Vite's dev server serves each module, and each
stylesheet a module imports, with an inline source map, through which this maps the byte ranges
that ran to the lines of ui/src's files: JavaScript and CSS both at line granularity. Each check
also names the fixtures its page fetched, and the file each fixture's recording's hash, for
changes to the Rust that records them (selection.py).

By default it measures origin/staging in a detached worktree of its own, which it removes after;
`--commit` names another commit, and `--here` measures this checkout as it is. The result goes to
`$BERYL_CACHE/ci/coverage/<commit>.json`, keyed by the run log's check ids (`ui-test:<title>`,
`verb:<verb>/<seat>/<layout>`, `shots:<name>`, runlog.py), with `latest.json` naming the newest
clean one. A coverage run is not a check: it logs no results and fails on no check's failure,
but lists the checks that failed.
"""

from __future__ import annotations

import argparse
import base64
import bisect
import json
import os
import posixpath
import re
import shutil
import subprocess
import sys
import tempfile
import time
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from . import paths, runlog, selection, shots, system, warm

FORMAT = 1
# The browser with a coverage API: Playwright measures coverage only in Chromium.
BROWSER = "chromium"
# The inline source map Vite appends to a module (`//#`) or a stylesheet (`/*#`).
INLINE_MAP: re.Pattern[str] = re.compile(
    r"[#@] sourceMappingURL=data:application/json;(?:charset=utf-8;)?base64,"
    r"([A-Za-z0-9+/=]+)"
)
BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
DIGITS: dict[str, int] = {c: i for i, c in enumerate(BASE64)}


def folder() -> Path:
    """Where coverage files go: `$BERYL_CACHE/ci/coverage`."""
    path = paths.cache_root() / "ci" / "coverage"
    path.mkdir(parents=True, exist_ok=True)
    return path


def vlq(field: str) -> list[int]:
    """The numbers a source map's base64 VLQ field encodes."""
    values: list[int] = []
    value = shift = 0
    for char in field:
        digit = DIGITS[char]
        value += (digit & 31) << shift
        if digit & 32:
            shift += 5
            continue
        values.append(-(value >> 1) if value & 1 else value >> 1)
        value = shift = 0
    return values


@dataclass(frozen=True)
class Segment:
    """One mapping of a source map: generated (line, column) to a source's line, all 0-based."""

    line: int
    column: int
    source: int
    source_line: int


def segments(mappings: str) -> list[Segment]:
    """A source map's `mappings`, decoded. Segments with no source are left out. The source
    index and line are relative to the previous segment's across lines; the column only within
    a line (source map revision 3)."""
    found: list[Segment] = []
    source = source_line = 0
    for line, group in enumerate(mappings.split(";")):
        column = 0
        for field in group.split(","):
            if not field:
                continue
            values = vlq(field)
            column += values[0]
            if len(values) < 4:
                continue
            source += values[1]
            source_line += values[2]
            found.append(Segment(line, column, source, source_line))
    return found


def line_starts(text: str) -> list[int]:
    """Each line's offset in `text`, in UTF-16 code units: the unit of V8's and the DevTools
    protocol's offsets and of source maps' columns."""
    starts = [0]
    for line in text.split("\n")[:-1]:
        starts.append(starts[-1] + len(line.encode("utf-16-le")) // 2 + 1)
    return starts


def flatten(ranges: Iterable[Sequence[int]]) -> list[tuple[int, int, int]]:
    """Nested `[start, end, count]` ranges, as V8's block coverage reports them (a function's
    range, then the blocks inside it that ran a different number of times), as disjoint ones in
    order, each with the count of the innermost range around it."""
    ordered = sorted(ranges, key=lambda r: (r[0], -r[1]))
    flat: list[tuple[int, int, int]] = []
    open_: list[tuple[int, int]] = []  # (end, count), innermost last
    at = 0

    def emit(upto: int) -> None:
        nonlocal at
        if open_ and upto > at:
            flat.append((at, upto, open_[-1][1]))
        at = max(at, upto)

    for start, end, count in ordered:
        while open_ and open_[-1][0] <= start:
            emit(open_[-1][0])
            open_.pop()
        emit(start)
        open_.append((end, count))
    while open_:
        emit(open_[-1][0])
        open_.pop()
    return flat


class Mapped:
    """A script or stylesheet as the page ran it, with its inline source map read: the offset
    of each mapped segment, in order, and the (file, line) each maps to. Files are relative to
    the checkout, and only those in it outside node_modules count."""

    def __init__(self, text: str, base: Path, root: Path) -> None:
        self.offsets: list[int] = []
        self.lines: list[tuple[str, int]] = []
        maps = INLINE_MAP.findall(text)
        if not maps:
            return
        source_map = json.loads(base64.b64decode(maps[-1]))
        source_root = source_map.get("sourceRoot") or ""
        files: list[str | None] = []
        for source in source_map.get("sources", []):
            path = Path(os.path.normpath(base / source_root / (source or "")))
            try:
                relative = path.relative_to(root).as_posix()
            except ValueError:
                relative = None
            files.append(
                None if relative is None or "node_modules" in relative else relative
            )
        starts = line_starts(text)
        for s in sorted(
            segments(source_map["mappings"]), key=lambda s: (s.line, s.column)
        ):
            file = files[s.source] if 0 <= s.source < len(files) else None
            if file is None or s.line >= len(starts):
                continue
            self.offsets.append(starts[s.line] + s.column)
            self.lines.append((file, s.source_line + 1))

    def covered(self, ranges: Iterable[Sequence[int]]) -> set[tuple[str, int]]:
        """The (file, line) pairs a segment of which starts where a range counts more than 0."""
        found: set[tuple[str, int]] = set()
        for start, end, count in flatten(ranges):
            if count > 0:
                lo = bisect.bisect_left(self.offsets, start)
                hi = bisect.bisect_left(self.offsets, end)
                found.update(self.lines[lo:hi])
        return found


def source_base(source: dict[str, str], root: Path) -> Path:
    """The folder a saved source's map names its sources from: a module's on the dev server,
    which serves ui/ at its root (`url`), or a stylesheet's on disk (`file`)."""
    if "url" in source:
        return root / "ui" / posixpath.dirname(source["url"]).lstrip("/")
    return Path(source["file"]).parent


def spans(lines: Iterable[int]) -> list[list[int]]:
    """Line numbers as sorted inclusive `[first, last]` runs."""
    runs: list[list[int]] = []
    for line in sorted(set(lines)):
        if runs and runs[-1][1] == line - 1:
            runs[-1][1] = line
        else:
            runs.append([line, line])
    return runs


def build(raw: Path, root: Path) -> dict[str, dict[str, object]]:
    """Each check's entry from the records the scripts wrote into `raw`: its outcome, seconds,
    fixtures and the lines of each file it ran, as `spans`; `files` is null when it ran no page
    (a check of ui/src's code in Node) or its page could not report coverage, so that no change
    can rule it out. A check recorded twice keeps its last record."""
    mapped: dict[str, Mapped] = {}

    def load(name: str) -> Mapped:
        if name not in mapped:
            source = json.loads((raw / "sources" / name).read_text())
            mapped[name] = Mapped(source["text"], source_base(source, root), root)
        return mapped[name]

    checks: dict[str, dict[str, object]] = {}
    for records in sorted(raw.glob("checks-*.jsonl")):
        for line in records.read_text().splitlines():
            record = json.loads(line)
            entry: dict[str, object] = {
                "outcome": record["outcome"],
                "seconds": record["seconds"],
                "fixtures": record.get("fixtures", []),
                "files": None,
            }
            if "js" in record:
                ran: dict[str, set[int]] = {}
                for part in [*record["js"], *record["css"]]:
                    for file, number in load(part["source"]).covered(part["ranges"]):
                        ran.setdefault(file, set()).add(number)
                entry["files"] = {f: spans(ran[f]) for f in sorted(ran)}
            checks[record["check"]] = entry
    return checks


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=paths.REPO,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def write(
    checks: dict[str, dict[str, object]],
    commit: str,
    dirty: bool,
    lanes: dict[str, int],
    seconds: float,
    recordings: dict[str, str] | None = None,
) -> Path:
    """Writes the coverage file for `commit`, and points latest.json at it if the tree was
    clean. Returns its path. `recordings` hashes each fixture as recorded (selection.hashes),
    against which the fast check finds the fixtures a Rust change re-recorded otherwise.
    """
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
    failed = sorted(c for c, e in checks.items() if e["outcome"] != "pass")
    document: dict[str, object] = {
        "format": FORMAT,
        "commit": commit,
        "dirty": dirty,
        "time": now,
        "seconds": round(seconds, 1),
        "lanes": lanes,
        "failed": failed,
        "checks": checks,
    }
    if recordings is not None:
        document["recordings"] = recordings
    name = f"{commit}{'-dirty' if dirty else ''}.json"
    path = folder() / name
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(document, separators=(",", ":")))
    tmp.replace(path)
    if not dirty:
        latest = folder() / "latest.json"
        latest.write_text(
            json.dumps({"commit": commit, "file": name, "time": now}, indent=2) + "\n"
        )
    return path


def measure_here() -> int:
    """Measures this checkout as it is and writes its coverage file."""
    started = time.monotonic()
    commit = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain", "--untracked-files=no"))
    with tempfile.TemporaryDirectory(prefix="beryl-coverage-") as tmp:
        raw, recorded, out = (Path(tmp) / d for d in ("raw", "fixtures", "shots"))
        raw.mkdir()
        out.mkdir()
        try:
            env = shots.browser_env(BROWSER)
        except shots.UiError as e:
            print(f"coverage: {e}", file=sys.stderr)
            return 1
        env["BERYL_COVERAGE"] = str(raw)
        # Not a check: nothing reaches the run log.
        env.pop(runlog.RESULTS_ENV, None)
        # ui-test's fixtures, and those of shots.json, which its pages fetch from ui/public.
        if (
            shots.record_fixtures(recorded, shots.ui_test_fixtures())
            or shots.record_fixtures()
        ):
            print("coverage: recording the fixtures failed", file=sys.stderr)
            return 1
        recordings = selection.hashes(shots.RECORDED) | selection.hashes(recorded)
        lanes: dict[str, int] = {}
        try:
            with shots.served_ui(env) as url:
                scripts = [
                    ("ui-test", ["scripts/interactions.mjs", url, str(recorded)]),
                    ("ui-test", ["scripts/verbs.mjs", url, str(recorded)]),
                    ("shots", ["scripts/shots.mjs", url, str(out), "shots.json"]),
                ]
                for lane, script in scripts:
                    print(f"coverage: {lane} {script[0]}", file=sys.stderr, flush=True)
                    code = subprocess.run(
                        ["node", *script],
                        cwd=paths.UI,
                        env=env,
                        stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL,
                        check=False,
                    ).returncode
                    lanes[lane] = lanes.get(lane, 0) or code
        except shots.UiError as e:
            print(f"coverage: {e}", file=sys.stderr)
            return 1
        checks = build(raw, paths.REPO)
    path = write(checks, commit, dirty, lanes, time.monotonic() - started, recordings)
    failed = sum(e["outcome"] != "pass" for e in checks.values())
    print(
        f"coverage: {len(checks)} checks ({failed} failed) in "
        f"{time.monotonic() - started:.0f} s: {path} "
        f"({path.stat().st_size / 1e6:.1f} MB)"
    )
    return 0


def measure(commit: str) -> int:
    """Measures `commit` in a detached worktree of its own, removed after, by that commit's own
    `by coverage --here`."""
    with system.file_lock(folder() / ".lock"):
        if commit.startswith("origin/"):
            subprocess.run(
                ["git", "fetch", "--quiet", "origin", commit.removeprefix("origin/")],
                cwd=paths.REPO,
                check=False,
            )
        sha = git("rev-parse", "--verify", f"{commit}^{{commit}}")
        tree = folder() / "trees" / sha[:12]
        remove_tree(tree)
        git("worktree", "add", "--detach", "--quiet", str(tree), sha)
        try:
            warm.ensure(tree)
            env = {**os.environ, "PYTHONPATH": str(tree / "tools" / "src")}
            return subprocess.run(
                [sys.executable, "-m", "beryl_tools.cli", "coverage", "--here"],
                cwd=tree,
                env=env,
                stdin=subprocess.DEVNULL,
                check=False,
            ).returncode
        finally:
            remove_tree(tree)


def remove_tree(tree: Path) -> None:
    subprocess.run(
        ["git", "worktree", "remove", "--force", str(tree)],
        cwd=paths.REPO,
        capture_output=True,
        check=False,
    )
    shutil.rmtree(tree, ignore_errors=True)
    subprocess.run(
        ["git", "worktree", "prune"], cwd=paths.REPO, capture_output=True, check=False
    )


def coverage_command(args: argparse.Namespace) -> int:
    return measure_here() if args.here else measure(args.commit)


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "coverage", help="record the source lines each browser check runs"
    )
    p.add_argument(
        "--commit",
        default="origin/staging",
        help="the commit to measure, in a worktree of its own (origin/staging)",
    )
    p.add_argument(
        "--here", action="store_true", help="measure this checkout as it is instead"
    )
    p.set_defaults(func=coverage_command)
