"""`just b3d timeline-parity`: the web interface and Beryl 3D play every recording on the same
timeline (docs/beryl-3d.md, 'Timeline parity').

It exports the recordings (`just b3d export`), plays them with `ui/scripts/timeline.mjs` (the
web interface's timeline), prints Beryl 3D's own (`just b3d timeline`) and compares the two
recording by recording and seat by seat. Two timelines agree when they hold the same steps
with the same view, beat, step, verb, case, effects, sounds and `own`, and every start and
end is within [`TOLERANCE_MS`] of the other's, or the web's step is later only because of
where the web interface lays cards out ([`laid_out_later`]).

`data/beryl3d/timeline-parity.json` lists the recordings that differ and by how many steps. A
step counts once however many of its fields differ, and a step that only one side has counts
once. A recording that differs and is not listed fails, and so does one that differs by more
steps than its entry. `--update` rewrites the entries to what was measured and refuses to
raise or add one unless `--raise` is given as well; a commit that raises one says why.

The web side's lines are kept in `$BERYL_CACHE/b3d/web-timelines-<hash>.jsonl`. The hash covers
the recordings pack, the exported recordings and every file the web timeline imports, followed
from `ui/scripts/timeline.mjs` through relative imports ([`web_sources`]), so a run that finds
the file does not start `node`. Writing a new one deletes those older than [`WEB_CACHE_DAYS`].

A run leaves nothing else behind: it deletes the exported recordings and Beryl 3D's timeline file
when it is done. Both used to stay, about 400 MB a run in the temporary folder and 12 MB in
`$BERYL_CACHE/b3d/out`, and after some 700 runs the disk was full.

`--fixtures` compares only the recorded fixtures, which takes about 25 to 60 s depending on
machine load; without it the command compares every recording, since Beryl 3D's own
`timeline --all` is most of the time.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import json
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Any

from .. import paths as repo_paths
from . import paths

ALLOWLIST: Path = paths.DATA / "timeline-parity.json"
# How long a cached web timeline is kept after it was written. Worktrees on different commits
# share the cache, so a file another worktree still uses is kept for a while.
WEB_CACHE_DAYS = 2
# Two starts or two ends this far apart, in milliseconds, still agree.
TOLERANCE_MS = 1.0
# The most differing recordings a run prints before it only counts the rest.
MOST_PRINTED = 20
# The fields of a step that must be equal, the start and end aside.
EXACT: tuple[str, ...] = ("verb", "case", "effects", "sounds")
# The web timeline's script, under `ui/`, from which [`web_sources`] follows its imports.
WEB_ENTRY = "scripts/timeline.mjs"
# A module specifier in an `import ... from`, `export ... from`, `import "..."` or `import(...)`.
IMPORT: re.Pattern[str] = re.compile(
    r"""(?:\bfrom|\bimport)\s*\(?\s*["']([^"']+)["']"""
)
# Extensions an import with none may name, in the order timeline.mjs's resolve hook tries them.
IMPLIED = (".ts", ".tsx")
# Modules whose imports are not followed: timeline.mjs serves a `.tsx` file as its literal
# constants alone, so what it imports is never loaded.
UNFOLLOWED = (".tsx", ".json")

# A step as printed on either side, by its keys.
Step = dict[str, Any]
# Where a step is: its view, beat, step and whether the client made it itself ([`place`]).
Place = tuple[int, int, int, bool]
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


def place(step: Step) -> Place:
    """Where a step is: its view, beat, step and whether the client made it itself."""
    return (step["view"], step["beat"], step["step"], bool(step.get("own", False)))


def agree(a: Step, b: Step) -> bool:
    if any(a.get(k) != b.get(k) for k in EXACT):
        return False
    return all(abs(a[k] - b[k]) <= TOLERANCE_MS for k in ("start", "end"))


# How long the web interface holds a strike back at a time while its way would cross an earlier
# strike's (`LOOK.wait` in ui/src/stage/enter.ts).
STRIKE_WAIT_MS = 25.0
# How long, as a share of a `vanish` step, the web interface takes to close up what the card
# that left made room for (`CLOSES` in ui/src/stage/tracks.ts).
CLOSES = 0.4
# The stage's timing conventions, whose `closes` say how long a close-up after a cut takes.
UI_CONVENTIONS: Path = repo_paths.REPO / "data" / "ui-conventions.json"


@cache
def _cut_closes() -> float:
    """The longest close-up the session sends for a `vanish` step that takes no time (a cut,
    such as a finished ability), from `closes` in `data/ui-conventions.json`'s verbs."""
    verbs = json.loads(UI_CONVENTIONS.read_text())["verbs"]
    return max(
        (float(ms) for verb in verbs for ms in verb.get("closes", {}).values()),
        default=0.0,
    )


def _closes(step: Step) -> float:
    """How long the web interface takes, after the `vanish` step `step` ends, to close up what
    its card made room for (`roomIn` and `Timing.closes` in ui/src/stage/tracks.ts)."""
    length = step["end"] - step["start"]
    return float(round(CLOSES * length)) if length > TOLERANCE_MS else _cut_closes()


@dataclass
class _Script:
    """How far [`laid_out_later`] has got through the steps of one script, on one side of
    `own`: how much later the web's steps are than Beryl 3D's, the beat it is in, on each
    side the latest end of the steps of the beats before that one and of every step so far,
    and the longest close-up after a web `vanish` step of the beat before and of this beat
    (None where it has none).
    """

    shift: float = 0.0
    beat: int | None = None
    before: tuple[float, float] | None = None
    so_far: tuple[float, float] = (float("-inf"), float("-inf"))
    closed_before: float | None = None
    closes: float | None = None


def _strike_wait(grown: float) -> bool:
    """Whether `grown` milliseconds is a whole number of the web's strike waits, more than none."""
    waits = round(grown / STRIKE_WAIT_MS)
    return waits > 0 and abs(grown - waits * STRIKE_WAIT_MS) <= TOLERANCE_MS


def laid_out_later(
    by_web: dict[Place, Step], by_native: dict[Place, Step]
) -> set[Place]:
    """The places of the steps that differ only because the web interface plays them later for
    a reason of its own layout, which docs/beryl-3d.md ('Timeline parity') says not to copy:

    - **A beat waits for cards closing up.** The web's beat lasts until the cards that close up
      after a card left are at their new places (`closedBy` in ui/src/stage/tracks.ts), which
      depends on where its layout puts them. Beryl 3D lays its table out differently, so its
      close-up can end sooner. Accepted: the first step of a beat (not the first beat of its
      script) begins later on the web, as long as the beat before holds a `vanish` step on the
      web, the web's beat begins no sooner than every earlier step of its script on the web has
      ended, and it waits longer after that than Beryl 3D's waits after its own by no more than
      the longest close-up after one of those `vanish` steps ([`_closes`]).
    - **A strike is held back.** The web holds a strike back, 25 ms at a time, while its way
      across the table would cross an earlier strike's still on its way out or back, and every
      later step of the beat as long (`awaited` in ui/src/stage/enter.ts). Beryl 3D does not
      hold strikes back. Accepted: a strike that is not the first step of its beat begins a
      whole number of those waits later on the web than the step before it in its beat.

    Either way, the steps after it are accepted while they are later on the web by the same
    amount, and so is the first step of a later beat that both sides begin equally long after
    their own earlier steps ended, which happens when the later steps the web was held for end
    the beat. Every accepted step has the same verb, case, effects, sounds and length on both
    sides. A step that is not accepted leaves the accepted shift as it was, so a real
    difference that moves every later step still counts each of them.

    Within that bound this cannot tell a close-up from any other reason the first step of a
    beat began later on the web, since neither timeline says why a beat ended when it did: a
    Beryl 3D beat that begins up to a close-up too early after one where a card left is
    accepted too.
    """
    accepted: set[Place] = set()
    scripts: dict[tuple[int, bool], _Script] = {}
    for at in sorted(by_web.keys() | by_native.keys()):
        view, beat, _, own = at
        script = scripts.setdefault((view, own), _Script())
        first = script.beat != beat
        if first:
            if script.beat is not None:
                script.before = script.so_far
            script.beat = beat
            script.closed_before, script.closes = script.closes, None
        a, b = by_web.get(at), by_native.get(at)
        if a is not None and b is not None:
            shift = a["start"] - b["start"]
            if agree(a, b) or _later(a, b, script, first):
                if not agree(a, b):
                    accepted.add(at)
                script.shift = shift
        if a is not None and a["verb"] == "vanish":
            script.closes = max(script.closes or 0.0, _closes(a))
        web_end, native_end = script.so_far
        script.so_far = (
            web_end if a is None else max(web_end, a["end"]),
            native_end if b is None else max(native_end, b["end"]),
        )
    return accepted


def _later(a: Step, b: Step, script: _Script, first: bool) -> bool:
    """Whether the web's step `a` differs from Beryl 3D's `b` only in being later, for one of
    the reasons [`laid_out_later`] accepts."""
    if any(a.get(k) != b.get(k) for k in EXACT):
        return False
    if abs((a["end"] - a["start"]) - (b["end"] - b["start"])) > TOLERANCE_MS:
        return False
    grown = (a["start"] - b["start"]) - script.shift
    if abs(grown) <= TOLERANCE_MS:
        return True
    if not first:
        return a["verb"] == "strike" and _strike_wait(grown)
    before, closes = script.before, script.closed_before
    if before is None:
        return False
    web_gap = a["start"] - before[0]
    native_gap = b["start"] - before[1]
    if abs(web_gap - native_gap) <= TOLERANCE_MS:
        return True
    return (
        closes is not None
        and web_gap >= -TOLERANCE_MS
        and native_gap < web_gap <= native_gap + closes + TOLERANCE_MS
    )


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
    later = laid_out_later(by_web, by_native)
    count = 0
    first: tuple[Step | None, Step | None] | None = None
    # In the order the steps begin on either side, the web's first.
    for at in [*by_web, *(p for p in by_native if p not in by_web)]:
        a, b = by_web.get(at), by_native.get(at)
        if at in later or (a is not None and b is not None and agree(a, b)):
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


def web_sources(ui: Path) -> list[Path]:
    """Every file the web timeline loads from the repository: `ui/scripts/timeline.mjs` and
    the files its relative imports reach, transitively, including JSON data outside `ui/`
    (the stage's timings import `data/ui-conventions.json`). Packages under `node_modules`
    are covered by `ui/package-lock.json`, which is included too. Sorted, without repeats.
    """
    found: set[Path] = set()
    waiting = [ui / WEB_ENTRY]
    while waiting:
        file = waiting.pop().resolve()
        if file in found or not file.is_file():
            continue
        found.add(file)
        if file.suffix in UNFOLLOWED:
            continue
        for specifier in IMPORT.findall(file.read_text()):
            if not specifier.startswith(("./", "../")):
                continue
            target = file.parent / specifier
            if target.suffix:
                waiting.append(target)
            else:
                waiting += [target.with_name(target.name + e) for e in IMPLIED]
    lock = ui / "package-lock.json"
    if lock.is_file():
        found.add(lock.resolve())
    return sorted(found)


def web_hash(pack: str, ui: Path, exported: Path) -> str:
    """The hash that names the web timelines' cache file: the pack's name, the content of
    every recording exported to `exported` and of every file the web timeline loads
    ([`web_sources`]). The exported files count because `b3d export` adds messages of its own
    to the pack's, so a change to the exporter changes what the web timeline prints."""
    digest = hashlib.sha256(pack.encode())
    for file in sorted(exported.glob("*.json")):
        digest.update(file.name.encode())
        digest.update(file.read_bytes())
    root = ui.resolve().parent
    for file in web_sources(ui):
        digest.update(str(file.relative_to(root)).encode())
        digest.update(file.read_bytes())
    return digest.hexdigest()[:16]


def out(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command, capture_output=True, text=True, check=False, **kwargs
    )


def b3d(arguments: list[str]) -> str:
    """Runs the `b3d` binary built by `pack_name` and returns its output.
    Building `beryl_inspect` on its own resolves a different set of Cargo features and
    leaves a second build in the target directory."""
    from . import cli

    line = [str(cli.program("b3d")), *arguments]
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
    """Builds the shared binaries and returns the recordings pack's name."""
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
    # Not at the top: Ignite's adapter imports this module without it.
    from .. import deps

    deps.ensure_node_modules(repo_paths.REPO)
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
    # Written beside the cache file and renamed onto it, so a run that starts meanwhile (two
    # runs of `timeline-parity` at once share this cache) finds no file or the whole one, never
    # the part written so far.
    partial = cached.with_name(f"{cached.name}.{os.getpid()}.tmp")
    partial.write_text(done.stdout)
    os.replace(partial, cached)
    prune_web_cache(cached.parent)
    return done.stdout.splitlines()


def prune_web_cache(directory: Path, now: float | None = None) -> None:
    """Deletes the cached web timelines not written within [`WEB_CACHE_DAYS`], with any partial
    file a killed run left. Each change to the web timeline's sources writes a new file, so
    without this they pile up."""
    oldest = (time.time() if now is None else now) - WEB_CACHE_DAYS * 86400
    for file in directory.glob("web-timelines-*"):
        try:
            written = file.stat().st_mtime
        except FileNotFoundError:
            # Another run renamed or pruned it after the listing.
            continue
        if written < oldest:
            file.unlink(missing_ok=True)


def native_lines() -> list[str]:
    """Beryl 3D's timeline of every recording and seat, from its own file. One run for all is
    faster than a run for each recording, since each start of the program is checked by macOS
    before it runs."""
    printed = b3d(["timeline", "--all"]).splitlines()
    # The first line is the command that prints it again, the second the file, which is
    # deleted once read: every run writes a new one.
    file = Path(printed[1])
    lines = file.read_text().splitlines()
    file.unlink()
    return lines


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
    try:
        return parity(args, pack, directory)
    finally:
        shutil.rmtree(directory, ignore_errors=True)


def parity(args: argparse.Namespace, pack: str, directory: Path) -> int:
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
    # The exported recordings are deleted when the run ends, so the command exports them again.
    again = shlex.quote(str(Path(tempfile.gettempdir()) / "b3d-recordings"))
    web_command = f"just b3d export {again} && node ui/scripts/timeline.mjs {again}"
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
