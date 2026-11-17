"""`just b3d match motion`: how each moving element's path, size and opacity differ between the
web interface and Beryl 3D, frame by frame over a span of a recording (docs/beryl-3d.md,
"Matching the web interface's look"). `timeline-parity` holds each step's start, end and verb
equal; this measures what happens in between, which is what makes an animation feel different.

1. `b3d tree --records --at <from> --to <to>` plays the recording in Beryl 3D on its stepped
   clock and prints every element's record in each 60 Hz frame of the span. A card on the 3D
   table has its projected screen rectangle as its box.
2. motion.mjs opens the web interface's page of the same view on Playwright's paused clock and
   steps it to the same times after the view's arrival, reading every element's box and
   opacity at each (snapshot.py's moment, frame by frame).
3. Each frame is paired by the matcher (joined.py), and each Beryl 3D element keeps the web
   element it was paired with most often (tracks.py).
4. For each pair where either side moves, it prints the largest difference of the boxes'
   centers, of their sizes and of opacity, each with the frame it falls in, worst first, and
   writes a plot of both tracks (plot.py) into the span's directory under the target's
   snapshot directory: `plots/<element>.svg` and `.png`, `motion.txt` and `motion.json`.

The span must lie within one view, because the web page opens at a view and plays it alone;
frames after the next view arrives are left out, and the report says so. As in `snapshot`, the
web page's loading frames make the start of its animation approximate.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

from ... import paths as repo_paths
from ... import shots
from .. import compare
from . import plot, snapshot, tracks
from .target import UNSAFE, Target, target


def motion_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "recording", help="a recording `just b3d list` names, or a test's name"
    )
    parser.add_argument(
        "--from", dest="start", required=True, help="the span's first moment"
    )
    parser.add_argument(
        "--to", dest="end", required=True, help="the span's last moment"
    )
    parser.add_argument("--seat", choices=("you", "opponent"), default=None)
    parser.add_argument("--layout", choices=("desktop", "phone"), default=None)
    parser.add_argument("--size", default=None, help="<width>x<height> in points")
    parser.add_argument("--scale", default=None, help="pixels a point (default 2)")
    parser.add_argument(
        "--only",
        metavar="LOCATOR",
        default=None,
        help="that Beryl 3D element alone, as `tree` reads a locator, moving or not",
    )
    parser.add_argument(
        "--art", action="store_true", help="draw the app's card scans on both sides"
    )


@dataclass(frozen=True)
class Pair:
    """A Beryl 3D element and its web partner over the span, with their tracks; `key` is the
    Beryl 3D element's latest locator."""

    key: str
    web: tracks.Track
    b3d: tracks.Track
    difference: tracks.Difference
    plot: Path | None = None


def events(messages: Sequence[dict[str, object]]) -> list[dict[str, object]]:
    return [m for m in messages if m.get("kind") == "event"]


def released_for(messages: Sequence[dict[str, object]], views: int) -> int:
    """How many of the fixture's events the page must have been handed to have its first
    `views` views (`compare.web_at`, which reads 0 as all of them)."""
    return 0 if views == 0 else compare.web_at(list(messages), views)


def plays_on(event: dict[str, object]) -> bool:
    """Whether the fixture transport's `advance` hands the page the event after `event` too
    (`playsOn` in ui/src/ipc.ts): a view whose status is thinking or playing."""
    if event.get("event") != "game-update":
        return False
    payload = event.get("payload")
    view = payload.get("view") if isinstance(payload, dict) else None
    status = view.get("status") if isinstance(view, dict) else None
    return isinstance(status, dict) and status.get("kind") in ("thinking", "playing")


def schedule(
    messages: Sequence[dict[str, object]],
    given: Sequence[float],
    start: float,
    end: float,
) -> tuple[int, list[list[float]], list[str]]:
    """How the web page is handed the views Beryl 3D was given at the times `given`, for a span
    whose first batch of views arrived at `start` and which ends at `end`: the events the page
    opens with (every view given before `start`), each `[ms after start, advances]` that
    brings it up to the views Beryl 3D had by then, and a note for each time the page gets
    more views than Beryl 3D had, since `advance` hands over a playing view's successors too.
    """
    listed = events(messages)
    released = released_for(messages, sum(1 for at in given if at < start))
    opened = released
    out: list[list[float]] = []
    notes: list[str] = []
    for at in sorted({at for at in given if start <= at <= end}):
        wanted = released_for(messages, sum(1 for t in given if t <= at))
        count = 0
        while released < wanted and released < len(listed):
            while True:
                last = listed[released]
                released += 1
                if released >= len(listed) or not plays_on(last):
                    break
            count += 1
        if count:
            out.append([at - start, count])
        if released > wanted:
            notes.append(
                f"note: at {at:.0f}ms the web page was handed {released - wanted} more events "
                "than Beryl 3D's views, since a playing view brings the next with it"
            )
    return opened, out, notes


def span_frames(printed: str) -> list[dict[str, object]]:
    return [json.loads(line) for line in printed.splitlines() if line.startswith("{")]


def elements(frame: dict[str, object]) -> list[dict[str, object]]:
    found = frame["elements"]
    assert isinstance(found, list)
    return found


def chosen_track(
    b3d: Sequence[Sequence[dict[str, object]]], locator: str
) -> tuple[object | None, str]:
    """The track of the one Beryl 3D element `locator` names in the span (after
    `tracks.name_tracks`), as `tree` reads a locator: the element whose locator equals it in
    some frame, else the only one whose locator starts with it. None and why if not one.
    """
    named: dict[str, object] = {}
    for frame in b3d:
        for element in frame:
            if isinstance(element.get("key"), str):
                named[str(element["key"])] = element.get("track")
    found = {named[key] for key in named if key == locator} or {
        named[key] for key in named if key.startswith(locator)
    }
    if len(found) == 1:
        return found.pop(), ""
    some = ", ".join(sorted(key for key in named if key.startswith(locator))[:5])
    return None, f"--only: '{locator}' names {len(found)} Beryl 3D elements" + (
        f" ({some})" if some else "; `just b3d tree --at <moment>` lists them"
    )


def worst_text(
    label: str, worst: tracks.Worst | None, unit: str, times: list[float]
) -> str:
    if worst is None:
        return f"{label} -"
    return (
        f"{label} {worst.amount:.{3 if unit == '' else 1}f}{unit} "
        f"@#{worst.frame} {times[worst.frame]:.1f}ms"
    )


def pair_line(pair: Pair, times: list[float]) -> str:
    d = pair.difference
    line = "  ".join(
        [
            pair.key,
            worst_text("position", d.position, "pt", times),
            worst_text("size", d.size, "pt", times),
            worst_text("opacity", d.opacity, "", times),
        ]
    )
    web, b3d = tracks.present(pair.web), tracks.present(pair.b3d)
    if web != b3d:
        line += f"  (on screen: web frames {web}, Beryl 3D {b3d})"
    return line


def ranked(pair: Pair) -> tuple[float, float, float]:
    d = pair.difference
    return (
        -(d.position.amount if d.position else 0.0),
        -(d.size.amount if d.size else 0.0),
        -(d.opacity.amount if d.opacity else 0.0),
    )


def render_plots(plots: list[tuple[Path, Path]]) -> str | None:
    """Writes each SVG's PNG with plot.mjs; None when it worked, else why not."""
    if not plots:
        return None
    job = plots[0][0].parent / "plot-job.json"
    job.write_text(json.dumps({"plots": [[str(s), str(p)] for s, p in plots]}))
    try:
        done = subprocess.run(
            ["node", str(snapshot.HERE / "plot.mjs"), str(job)],
            cwd=repo_paths.UI,
            env=shots.browser_env(),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        job.unlink(missing_ok=True)
    if done.returncode != 0:
        return f"plot.mjs failed:\n{done.stdout}{done.stderr}"
    return None


def compared(
    web_frames: Sequence[Sequence[dict[str, object]]],
    b3d_frames: Sequence[Sequence[dict[str, object]]],
    latest: dict[object, str],
    only: object | None,
) -> tuple[list[Pair], list[str], list[str]]:
    """The pairs to report, worst first: each pair where either side moves, or the one whose
    Beryl 3D track is `only`, moving or not. Then the moving Beryl 3D elements (by `latest`, each
    track's locator) and web elements left without a partner."""
    partners = tracks.partners(web_frames, b3d_frames)
    web_tracks = tracks.tracks(web_frames)
    pairs: list[Pair] = []
    lonely_b3d: list[str] = []
    for name, track in tracks.tracks(b3d_frames).items():
        if only is not None and name != only:
            continue
        partner = partners.get(name)
        if partner is None:
            if only is not None or tracks.moves(track):
                lonely_b3d.append(latest[name])
            continue
        web = web_tracks[partner]
        if only is None and not (tracks.moves(track) or tracks.moves(web)):
            continue
        pairs.append(Pair(latest[name], web, track, tracks.difference(web, track)))
    if only is not None:
        return pairs, lonely_b3d, []
    about = {
        e.get("track"): f'{e.get("role")} "{e.get("name") or ""}"'
        for frame in web_frames
        for e in frame
    }
    partnered = set(partners.values())
    lonely_web = [
        f"{about[name]} (frames {tracks.present(track)})"
        for name, track in web_tracks.items()
        if name not in partnered and tracks.moves(track)
    ]
    return sorted(pairs, key=ranked), lonely_b3d, lonely_web


def run(args: argparse.Namespace) -> int:
    t: Target = target(
        argparse.Namespace(**{**vars(args), "at": args.start, "only": None})
    )
    printed = compare.b3d(
        [
            "tree",
            t.recording,
            *t.common(),
            "--at",
            args.start,
            "--to",
            args.end,
            "--records",
        ]
    )
    if printed is None:
        return 1
    frames = span_frames(printed)
    view = int(str(frames[0]["view"]))
    given = frames[0].get("view_given_ms")
    if not isinstance(given, int):
        print(
            f"match motion: no view has arrived by {args.start}; start the span later",
            file=sys.stderr,
        )
        return 1
    notes: list[str] = []
    kept = [f for f in frames if f["view"] == view]
    if len(kept) < len(frames):
        notes.append(
            f"note: view {view + 1} arrives at frame {len(kept)}; the {len(frames) - len(kept)}"
            " frames after it are left out, since the web page plays one view"
        )
    frames = kept
    times = [float(str(f["t_ms"])) for f in frames]
    out = t.directory() / UNSAFE.sub("_", f"motion-to-{args.end}")
    out.mkdir(parents=True, exist_ok=True)

    messages = snapshot.exported(t, build=False)
    if messages is None:
        return 1
    views_given = frames[-1].get("views_given_ms")
    assert isinstance(views_given, list)
    opened, advances, more = schedule(
        messages, [float(at) for at in views_given], float(given), times[-1]
    )
    notes += more
    job = snapshot.page_job(t, messages, opened, bool(args.art), out)
    job |= {
        "times_ms": [time - given for time in times],
        "advances": advances,
        "json": str(out / "web.json"),
    }
    lines, failed = snapshot.run_page("motion.mjs", job, out, None)
    if failed:
        print(f"match motion: {failed}", file=sys.stderr)
        return 1
    notes += [f"warning: web {line}" for line in lines]
    web_frames = [
        f["elements"] for f in json.loads((out / "web.json").read_text())["frames"]
    ]
    b3d_frames = [elements(f) for f in frames]

    latest = tracks.name_tracks(b3d_frames)
    only = None
    if args.only is not None:
        only, why = chosen_track(b3d_frames, args.only)
        if only is None:
            print(f"match motion: {why}", file=sys.stderr)
            return 1
    pairs, lonely_b3d, lonely_web = compared(web_frames, b3d_frames, latest, only)

    plots_dir = out / "plots"
    plots_dir.mkdir(exist_ok=True)
    for old in plots_dir.iterdir():
        old.unlink()
    rendered: list[tuple[Path, Path]] = []
    from_start = [time - times[0] for time in times]
    for n, pair in enumerate(pairs):
        stem = f"{n + 1:02d}-" + UNSAFE.sub("_", pair.key).strip("_")[:60]
        svg = plots_dir / f"{stem}.svg"
        svg.write_text(plot.svg(pair.key, from_start, pair.web, pair.b3d))
        rendered.append((svg, plots_dir / f"{stem}.png"))
        pairs[n] = Pair(**{**vars(pair), "plot": plots_dir / f"{stem}.png"})
    failed = render_plots(rendered)
    if failed:
        notes.append(f"warning: the plots stayed SVG: {failed}")

    report = [
        f"just b3d match motion {t.recording} --from {args.start} --to {args.end} "
        + " ".join(t.common())
        + (f" --only '{args.only}'" if args.only else ""),
        f"target {t.test_name()} ({t.layout}), view {view}, {len(frames)} frames from "
        f"{times[0]:.1f}ms to {times[-1]:.1f}ms ({times[0] - given:.1f}ms to "
        f"{times[-1] - given:.1f}ms after the view arrived)",
        *notes,
        f"{len(pairs)} {'pair' if only else 'moving pairs'}, worst position first:",
    ]
    for pair in pairs:
        report.append(pair_line(pair, times))
        if pair.plot is not None:
            report.append(
                f"  plot: {pair.plot.with_suffix('.svg') if failed else pair.plot}"
            )
    if lonely_b3d:
        report.append(f"moving in Beryl 3D with no web partner ({len(lonely_b3d)}):")
        report += [f"  {key}" for key in lonely_b3d]
    if lonely_web:
        report.append(
            f"moving on the web with no Beryl 3D partner ({len(lonely_web)}):"
        )
        report += [f"  {what}" for what in lonely_web[:20]]
        if len(lonely_web) > 20:
            report.append(f"  and {len(lonely_web) - 20} more")
    text = "\n".join(report) + "\n"
    (out / "motion.txt").write_text(text)
    (out / "motion.json").write_text(
        json.dumps(
            {
                "target": t.test_name(),
                "times_ms": times,
                "pairs": [
                    {
                        "key": p.key,
                        "position": (
                            vars(p.difference.position)
                            if p.difference.position
                            else None
                        ),
                        "size": vars(p.difference.size) if p.difference.size else None,
                        "opacity": (
                            vars(p.difference.opacity) if p.difference.opacity else None
                        ),
                        "plot": str(p.plot) if p.plot else None,
                    }
                    for p in pairs
                ],
                "lonely": {"b3d": lonely_b3d, "web": lonely_web},
            },
            indent=1,
        )
        + "\n"
    )
    print(text, end="")
    print(out / "motion.txt")
    return 0
