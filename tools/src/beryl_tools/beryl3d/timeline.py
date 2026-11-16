"""`just b3d timeline-parity`: the web interface and Beryl 3D play every recording on the same
timeline (docs/beryl-3d.md, 'Timeline parity').

It exports the recordings (`just b3d export`), plays them with `ui/scripts/timeline.mjs` (the
web interface's timeline), prints Beryl 3D's own (`just b3d timeline`) and compares the two
recording by recording and seat by seat. Two timelines agree when they hold the same steps
with the same view, beat, step, verb, case, effects and `own`, and every start and end is
within [`TOLERANCE_MS`] of the other's.

`data/beryl3d/timeline-parity.json` lists the recordings that differ and by how many steps. A
step counts once however many of its fields differ, and a step that only one side has counts
once. A recording that differs and is not listed fails, and so does one that differs by more
steps than its entry. `--update` rewrites the entries to what was measured and refuses to
raise or add one unless `--raise` is given as well; a commit that raises one says why.

The web side's lines are kept in `$BERYL_CACHE/b3d/web-timelines-<hash>.jsonl`. The hash covers
the recordings pack, the exported recordings and every source file the web timeline reads, so a
run that finds the file does not start `node`.

Measured with `--fixtures` (the lane's step) on 2026-11-15 at load averages of 20 to 60, five
runs each: with the web lines cached, 24.8, 26.5, 27.3, 57.3 and 59.4 s (median 27.3 s); with
them recomputed, 21.1, 27.5, 38.5, 49.1 and 57.3 s (median 38.5 s, as the load moved). Beryl
3D's own `timeline --all` is most of it, so the step always compares the fixtures only and the
sweep compares everything.

It never runs for a change to `ui/` (the `beryl3d` lane's paths are Beryl 3D's alone): when the
web interface's timeline moves, the sweep names the recordings that now differ, and Beryl 3D
follows.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shlex
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import paths as repo_paths
from .. import warm
from . import paths

ALLOWLIST: Path = paths.DATA / "timeline-parity.json"
# Two starts or two ends this far apart, in milliseconds, still agree.
TOLERANCE_MS = 1.0
# The most differing recordings a run prints before it only counts the rest.
MOST_PRINTED = 20
# The fields of a step that must be equal, the start and end aside.
EXACT: tuple[str, ...] = ("verb", "case", "effects")
# The directories and file the web timeline's output depends on, under `ui/`, besides the
# recordings pack.
WEB_SOURCES: tuple[str, ...] = ("src/stage", "src/game", "scripts/timeline.mjs")

# A step as printed on either side, by its keys.
Step = dict[str, Any]
# The steps of each recording and seat, keyed `<recording>/<seat>`.
Timelines = dict[str, list[Step]]


def key_of(recording: str, seat: str) -> str:
    return f"{recording}/{seat}"


def parse(lines: list[str]) -> Timelines:
    """The steps in `lines`, each a line of JSON, by recording and seat."""
    found: Timelines = {}
    for line in lines:
        if not line.startswith("{"):
            continue
        step = json.loads(line)
        found.setdefault(key_of(step["recording"], step["seat"]), []).append(step)
    return found


def place(step: Step) -> tuple[int, int, int, bool]:
    """Where a step is: its view, beat, step and whether the client made it itself."""
    return (step["view"], step["beat"], step["step"], bool(step.get("own", False)))


def agree(a: Step, b: Step) -> bool:
    if any(a.get(k) != b.get(k) for k in EXACT):
        return False
    return all(abs(a[k] - b[k]) <= TOLERANCE_MS for k in ("start", "end"))


@dataclass(frozen=True)
class Difference:
    """The first step of one recording and seat that differs, as each side printed it."""

    key: str
    # How many steps differ.
    count: int
    web: Step | None
    native: Step | None


def compare(web: list[Step], native: list[Step]) -> Difference | None:
    """How the two timelines of one recording and seat differ, or None when they agree."""
    by_web = {place(s): s for s in web}
    by_native = {place(s): s for s in native}
    count = 0
    first: tuple[Step | None, Step | None] | None = None
    # In the order the steps begin on either side, the web's first.
    for at in [*by_web, *(p for p in by_native if p not in by_web)]:
        a, b = by_web.get(at), by_native.get(at)
        if a is not None and b is not None and agree(a, b):
            continue
        count += 1
        first = first or (a, b)
    if first is None:
        return None
    return Difference("", count, first[0], first[1])


def differences(
    web: Timelines, native: Timelines, scope: set[str]
) -> dict[str, Difference]:
    """The recordings and seats of `scope` whose timelines differ, by key."""
    found: dict[str, Difference] = {}
    for key in sorted(scope):
        found_one = compare(web.get(key, []), native.get(key, []))
        if found_one is not None:
            found[key] = Difference(
                key, found_one.count, found_one.web, found_one.native
            )
    return found


def load_allowlist(path: Path = ALLOWLIST) -> dict[str, int]:
    if not path.exists():
        return {}
    return {k: int(v) for k, v in json.loads(path.read_text())["recordings"].items()}


def write_allowlist(entries: dict[str, int], path: Path = ALLOWLIST) -> None:
    body = {"recordings": dict(sorted(entries.items()))}
    path.write_text(json.dumps(body, indent=2) + "\n")


def over_limit(
    found: dict[str, Difference], allowed: dict[str, int]
) -> list[Difference]:
    """The differing recordings that are not listed, or differ by more steps than listed."""
    return [d for k, d in found.items() if d.count > allowed.get(k, 0)]


def lowered(
    found: dict[str, Difference],
    allowed: dict[str, int],
    scope: set[str],
    raise_: bool,
) -> tuple[dict[str, int], list[str]]:
    """The allowlist after `--update`: the entries of `scope` set to what was measured, those
    outside it kept. Returns it with the keys it would have had to raise or add, which it
    leaves as they were unless `raise_`."""
    entries = {k: v for k, v in allowed.items() if k not in scope}
    refused: list[str] = []
    for key in sorted(scope):
        measured = found[key].count if key in found else 0
        before = allowed.get(key, 0)
        if measured > before and not raise_:
            refused.append(key)
            if before:
                entries[key] = before
        elif measured:
            entries[key] = measured
    return entries, refused


def web_hash(pack: str, ui: Path, exported: Path) -> str:
    """The hash that names the web timelines' cache file: the pack's name, the content of
    every recording exported to `exported` and of every file under the web timeline's
    sources. The exported files count because `b3d export` adds messages of its own to the
    pack's, so a change to the exporter changes what the web timeline prints."""
    digest = hashlib.sha256(pack.encode())
    for file in sorted(exported.glob("*.json")):
        digest.update(file.name.encode())
        digest.update(file.read_bytes())
    for source in WEB_SOURCES:
        root = ui / source
        files = (
            [root]
            if root.is_file()
            else sorted(p for p in root.rglob("*") if p.is_file())
        )
        for file in files:
            digest.update(str(file.relative_to(ui)).encode())
            digest.update(file.read_bytes())
    return digest.hexdigest()[:16]


def out(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command, capture_output=True, text=True, check=False, **kwargs
    )


def b3d(arguments: list[str]) -> str:
    """The standard output of the `b3d` program run with `arguments`, which is built first.
    When it says the recordings pack is missing, the pack is built and it runs again."""
    from . import cli

    built = cli.build(["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"])
    if built != 0:
        raise SystemExit(built)
    line = [str(cli.linked("b3d")), *arguments]
    done = out(line)
    if done.returncode == cli.MISSING_PACK:
        if cli.record() != 0:
            raise SystemExit(1)
        done = out(line)
    if done.returncode != 0:
        sys.stderr.write(done.stderr)
        raise SystemExit(done.returncode)
    return done.stdout


def pack_name() -> str:
    """The name of the recordings pack for the present sources, built if it is missing."""
    from . import cli

    if cli.shared_build() != 0:
        raise SystemExit(1)
    done = out([str(cli.program("record"))], cwd=paths.WORKSPACE)
    if done.returncode != 0:
        sys.stderr.write(done.stderr)
        raise SystemExit(done.returncode)
    return Path(done.stdout.strip().splitlines()[-1]).name


def export(directory: Path) -> list[tuple[str, str]]:
    """Writes the recordings to `directory` and returns the recording and seat of each."""
    b3d(["export", str(directory)])
    found: list[tuple[str, str]] = []
    for file in sorted(directory.glob("*.json")):
        stem, seat = file.name.removesuffix(".json").rsplit(".", 1)
        found.append((stem.replace("__", "/"), seat))
    return found


def web_lines(pack: str, directory: Path) -> list[str]:
    """The web interface's timeline of every recording exported to `directory`, from the cache
    or by playing it with `node`."""
    ui = repo_paths.REPO / "ui"
    cached = paths.cache() / f"web-timelines-{web_hash(pack, ui, directory)}.jsonl"
    if cached.exists():
        return cached.read_text().splitlines()
    warm.ensure_node_modules(repo_paths.REPO)
    done = subprocess.run(
        ["node", "scripts/timeline.mjs", str(directory)],
        cwd=ui,
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        sys.stderr.write(done.stderr)
        raise SystemExit(done.returncode)
    cached.write_text(done.stdout)
    return done.stdout.splitlines()


def native_lines() -> list[str]:
    """Beryl 3D's timeline of every recording and seat, from its own file. One run for all is
    faster than a run for each recording, since each start of the program is checked by macOS
    before it runs."""
    printed = b3d(["timeline", "--all"]).splitlines()
    # The first line is the command that prints it again, the second the file.
    return Path(printed[1]).read_text().splitlines()


def fixture(name: str) -> bool:
    """Whether a recording is a recorded fixture, which has no `/` in its name, rather than a
    card spec moment."""
    return "/" not in name


def report(found: list[Difference], web_command: str) -> str:
    lines: list[str] = []
    for d in found[:MOST_PRINTED]:
        recording, seat = d.key.rsplit("/", 1)
        lines += [
            f"{d.key}: {d.count} step(s) differ; the first, as each side printed it:",
            f"  web:    {json.dumps(d.web) if d.web else 'no such step'}",
            f"  beryl3d: {json.dumps(d.native) if d.native else 'no such step'}",
            f"  just b3d timeline {recording} --seat {seat}",
            f"  {web_command}",
        ]
    if len(found) > MOST_PRINTED:
        lines.append(f"... and {len(found) - MOST_PRINTED} more recordings differ.")
    return "\n".join(lines)


def run_parity(args: argparse.Namespace) -> int:
    pack = pack_name()
    directory = Path(tempfile.mkdtemp(prefix="b3d-parity-"))
    names = export(directory)
    web_all = web_lines(pack, directory)
    selected = [
        (n, s)
        for n, s in names
        if n.startswith(args.prefix) and (not args.fixtures or fixture(n))
    ]
    scope = {key_of(n, s) for n, s in selected}
    web = parse(web_all)
    native = parse(native_lines())
    found = differences(web, native, scope)
    allowed = load_allowlist()
    web_command = f"node ui/scripts/timeline.mjs {shlex.quote(str(directory))}"
    if args.update:
        # An entry for a recording that no longer exists (a renamed spec moment) goes.
        exported = {key_of(n, s) for n, s in names}
        allowed = {k: v for k, v in allowed.items() if k in exported}
        entries, refused = lowered(found, allowed, scope, args.raise_)
        write_allowlist(entries)
        print(f"wrote {ALLOWLIST.name}: {len(entries)} recordings listed")
        if refused:
            print(
                f"left alone, since they would rise (use --raise): {', '.join(refused)}"
            )
        return 1 if refused else 0
    bad = over_limit(found, allowed)
    if bad:
        print(report(bad, web_command))
        print(
            f"{len(bad)} of {len(scope)} recordings and seats differ more than listed."
        )
        return 1
    print(
        f"timeline parity: {len(scope)} recordings and seats, {len(found)} listed as differing"
    )
    return 0


def parity_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "prefix", nargs="?", default="", help="only recordings starting so"
    )
    parser.add_argument(
        "--fixtures",
        action="store_true",
        help="only recorded fixtures, not spec moments",
    )
    parser.add_argument("--update", action="store_true", help="lower the allowlist")
    parser.add_argument(
        "--raise",
        dest="raise_",
        action="store_true",
        help="with --update, allow raising",
    )
