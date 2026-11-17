"""`just b3d perf`: the running app's frame timing of recorded fixtures, as `just ui-perf` times
the web interface's (docs/beryl-3d.md, "Driving the running app").

Each fixture of `ui/fixtures.json` that has a recording is replayed with no window for the
recording's length plus a second, and the frame log the app writes is read back. The log holds
one line for each *late* frame (a frame that missed the rate the host asked for) and a summary
of the whole run, so the client times here are over late frames only; `frames` and `late`
count every frame. Times are reported with the machine's load and never gated.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

from .. import checks, paths as repo_paths
from . import app, parity
from .command import Command

# How long past a recording's end the app keeps playing, so that its last frames are drawn.
EXTRA_S = 1
# One line of `b3d list`: a recording, its seat, its length and its view count.
LISTED: re.Pattern[str] = re.compile(r"^(\S+)\s+--seat \S+\s+(\d+)ms, \d+ views$")


def fixture_names() -> list[str]:
    """Every fixture `just ui-perf` can play: the definitions of `ui/fixtures.json`."""
    return [f["name"] for f in parity.read_json(parity.FIXTURES)["fixtures"]]


def recording_lengths(listing: str) -> dict[str, int]:
    """The longest length in milliseconds of each recording `b3d list` printed, over its seats."""
    lengths: dict[str, int] = {}
    for line in listing.splitlines():
        found = LISTED.match(line.strip())
        if found:
            name, ms = found.group(1), int(found.group(2))
            lengths[name] = max(ms, lengths.get(name, 0))
    return lengths


def run_seconds(length_ms: int) -> int:
    """`--for` in whole seconds: the recording's length rounded up, plus `EXTRA_S`."""
    return math.ceil(length_ms / 1000) + EXTRA_S


def read_log(path: Path) -> tuple[list[dict[str, Any]], dict[str, Any] | None]:
    """A frame log's late frames and its summary, which is None when the run ended before it
    wrote one."""
    late: list[dict[str, Any]] = []
    summary = None
    for text in path.read_text(encoding="utf-8").splitlines():
        line = json.loads(text)
        if "late" in line:
            late.append(line["late"])
        elif "summary" in line:
            summary = line["summary"]
    return late, summary


def percentile(values: list[float], share: float) -> float:
    """The nearest-rank percentile of `values`; 0 when there are none."""
    if not values:
        return 0.0
    ordered = sorted(values)
    return ordered[max(0, math.ceil(share * len(ordered)) - 1)]


def fixture_line(
    name: str, late: list[dict[str, Any]], summary: dict[str, Any] | None
) -> str:
    """One fixture's line: frames drawn, late frames, the worst and 95th-percentile client time
    of the late frames, the worst encode time and the worst wait for a drawable."""
    if summary is None:
        return f"{name:<28} no summary: the run ended before it wrote one"
    client = [frame["client_ms"] for frame in late]
    encoded = max((frame["encoded_ms"] for frame in late), default=0.0)
    return (
        f"{name:<28} {summary['frames']:>6} frames {summary['late']:>5} late"
        f"  client worst {max(client, default=0.0):6.2f} ms p95 {percentile(client, 0.95):6.2f} ms"
        f"  encode worst {encoded:6.2f} ms"
        f"  drawable wait worst {summary['worst_wait_ms']:6.1f} ms"
    )


def verb_lines(late: list[dict[str, Any]]) -> list[str]:
    """The late frames by the verb playing in them, most lateness first. A frame in no verb is
    `(none)`; one in several counts for each. Lateness is the gap past a frame at the rate the
    host asked for."""
    tally: dict[str, list[float]] = {}
    for frame in late:
        over = max(0.0, frame["since_last_ms"] - 1000.0 / frame["hz"])
        for verb in frame["verbs"] or ["(none)"]:
            entry = tally.setdefault(verb, [0.0, 0.0, 0.0])
            entry[0] += 1
            entry[1] += over
            entry[2] = max(entry[2], frame["client_ms"])
    ordered = sorted(tally.items(), key=lambda item: -item[1][1])
    return [
        f"  {verb:<26} {count:>5.0f} late, {over:8.1f} ms late in all, worst client {client:6.2f} ms"
        for verb, (count, over, client) in ordered
    ]


def load_line() -> str:
    one, five, fifteen = os.getloadavg()
    return f"load average {one:.2f} {five:.2f} {fifteen:.2f} (times are reported with it, never gated)"


def perf_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "fixtures",
        nargs="*",
        help="fixtures to play, by name; every fixture of ui/fixtures.json by default",
    )
    parser.add_argument(
        "--verbs", action="store_true", help="the late frames by the verb playing"
    )
    parser.add_argument(
        "--window",
        choices=app.WINDOWS,
        default="none",
        help="none (draws no frames to a window), or background to measure presentation",
    )
    parser.add_argument("--no-build", action="store_true")


def play(name: str, seconds: int, window: str) -> Path | None:
    """Plays `name` and gives the frame log the run wrote, or None when the run wrote none."""
    folder, checkout = app.frames_dir(), repo_paths.REPO.resolve()
    before = app.newest_log(folder, checkout)
    # Parsed by `just b3d app`'s own parser, so every option it adds has its default here.
    parser = argparse.ArgumentParser()
    app.app_arguments(parser)
    given = ["--fixture", name, "--window", window, "--for", f"{seconds}s"]
    app.run_app(parser.parse_args([*given, "--no-build"]))
    after = app.newest_log(folder, checkout)
    return after if after != before else None


def run_perf(args: argparse.Namespace) -> int:
    if not args.no_build and app.build() != 0:
        return checks.CANNOT_RUN
    listed = subprocess.run(
        ["just", "b3d", "list", "--full"], capture_output=True, text=True, check=False
    )
    lengths = recording_lengths(listed.stdout)
    names = args.fixtures or [n for n in fixture_names() if n in lengths]
    unknown = [n for n in names if n not in lengths]
    if unknown:
        print(f"perf: no recording named {', '.join(unknown)}", file=sys.stderr)
        return 2
    for name in names:
        log = play(name, run_seconds(lengths[name]), args.window)
        if log is None:
            print(f"{name:<28} the app wrote no frame log")
            continue
        late, summary = read_log(log)
        print(fixture_line(name, late, summary))
        if args.verbs:
            print("\n".join(verb_lines(late)))
    print(load_line())
    return 0


COMMANDS: tuple[Command, ...] = (
    Command(
        "perf",
        "frame timing of recorded fixtures played in the app (`just ui-perf`'s equivalent)",
        (
            ("just b3d perf verbs-zones", "play one fixture and print its frame line"),
            (
                "just b3d perf --verbs --window background",
                "every fixture, by verb, drawing to a window",
            ),
        ),
        run_perf,
        perf_arguments,
    ),
)
