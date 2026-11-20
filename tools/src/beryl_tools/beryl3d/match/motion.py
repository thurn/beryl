"""`just b3d match motion`: how each moving element's path, size and opacity differ between the
web interface and Beryl 3D, frame by frame over a span of a recording (docs/beryl-3d.md,
"Matching the web interface's look"). `timeline-parity` holds each step's start, end and verb
equal; this measures what happens in between, which is what makes an animation feel different.

1. `b3d tree --records --at <from> --to <to> --no-catch-up` plays the recording in Beryl 3D on
   its stepped clock, every script at real time as the web interface plays it (Beryl 3D
   otherwise plays faster while many views wait), and prints every element's record in each
   60 Hz frame of the span. A card on the 3D
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

The web page is handed the views Beryl 3D was still to play live, at the times Beryl 3D was given
them (clock.py), so that both clients play the same script at the same time. Frames after a
view given later than the span's first frame are left out, and the report says so.

`--all` measures a span of every verb, effect and mark and ranks them (motion_census.py).
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
from . import clock, joined, plot, snapshot, tracks
from .target import UNSAFE, Target, target


def motion_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "recording",
        nargs="?",
        help="a recording `just b3d list` names, or a test's name (left out with --all)",
    )
    parser.add_argument("--from", dest="start", help="the span's first moment")
    parser.add_argument("--to", dest="end", help="the span's last moment")
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
    parser.add_argument(
        "--all",
        action="store_true",
        help="rank every verb, effect and mark of data/beryl3d/match/motion-census.json "
        "by how far its motion is from the web's, worst first",
    )
    parser.add_argument(
        "--refresh-census",
        action="store_true",
        help="with --all, find each item's span again from the timelines and rewrite "
        "motion-census.json",
    )
    parser.add_argument(
        "--fresh",
        action="store_true",
        help="with --all, measure every span again even if nothing changed",
    )


@dataclass(frozen=True)
class Lonely:
    """An element that moves on one side with no partner on the other: how the report names
    it, its track, and the effect's name if it is an effect (role `effect`)."""

    key: str
    track: tracks.Track
    effect: str = ""


@dataclass(frozen=True)
class Pair:
    """A Beryl 3D element and its web partner over the span, with their tracks; `key` is the
    Beryl 3D element's latest locator, and `effect` the effect's name if they are an effect.
    """

    key: str
    web: tracks.Track
    b3d: tracks.Track
    difference: tracks.Difference
    plot: Path | None = None
    effect: str = ""


def effect_names(frames: Sequence[Sequence[dict[str, object]]]) -> dict[object, str]:
    """The effect's name of each track whose element is an effect (role `effect`)."""
    return {
        e.get("track"): str(e.get("name") or "")
        for frame in frames
        for e in frame
        if e.get("role") == "effect"
    }


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
) -> tuple[list[Pair], list[Lonely], list[Lonely]]:
    """The pairs to report, worst first: each pair where either side moves, or the one whose
    Beryl 3D track is `only`, moving or not. Then the moving Beryl 3D elements (by `latest`, each
    track's locator) and web elements left without a partner and not inside a partnered one,
    each with its track."""
    partners = tracks.partners(web_frames, b3d_frames)
    web_tracks = tracks.tracks(web_frames)
    b3d_effects, web_effects = effect_names(b3d_frames), effect_names(web_frames)
    pairs: list[Pair] = []
    lonely_b3d: list[Lonely] = []
    for name, track in tracks.tracks(b3d_frames).items():
        if only is not None and name != only:
            continue
        partner = partners.get(name)
        effect = b3d_effects.get(name, "")
        if partner is None:
            if only is not None or tracks.moves(track):
                lonely_b3d.append(Lonely(latest[name], track, effect))
            continue
        web = web_tracks[partner]
        if only is None and not (tracks.moves(track) or tracks.moves(web)):
            continue
        difference = tracks.difference(web, track)
        pairs.append(Pair(latest[name], web, track, difference, effect=effect))
    if only is not None:
        return pairs, lonely_b3d, []
    about = {
        e.get("track"): f'{e.get("role")} "{e.get("name") or ""}"'
        + (f" on {e.get('object')}" if e.get("role") == "effect" else "")
        for frame in web_frames
        for e in frame
    }
    partnered = set(partners.values())
    # An element inside a partnered one (a card's text panel on the web) moves with it, and
    # its partner's comparison already says how. An effect inside a card is drawn of its own,
    # and is measured as itself.
    carried = tracks.inside(web_frames, partnered) - set(web_effects)
    screens = [joined.screen_of(frame) for frame in web_frames]
    lonely_web = [
        Lonely(
            f"{about[name]} (frames {tracks.present(track)})",
            track,
            web_effects.get(name, ""),
        )
        for name, all_frames in web_tracks.items()
        if name not in partnered and name not in carried
        for track in [tracks.seen(all_frames, screens)]
        if tracks.moves(track)
    ]
    return sorted(pairs, key=ranked), lonely_b3d, lonely_web


@dataclass(frozen=True)
class Measured:
    """Both clients' tracks over one span, compared: the view the span lies in, the time both
    clocks count from (ms on Beryl 3D's clock, when the first view handed to the web page live
    arrived; clock.py), each frame's time, what the report should warn of, the
    span's directory, and `compared`'s pairs and lonely elements."""

    view: int
    given: float
    times: list[float]
    notes: list[str]
    out: Path
    pairs: list[Pair]
    lonely_b3d: list[Lonely]
    lonely_web: list[Lonely]


def measure(
    t: Target,
    end: str,
    only_locator: str | None,
    art: bool,
    serving: snapshot.Shared | None,
) -> Measured | str:
    """Plays the span from `t.at` to `end` in both clients and compares their tracks (steps 1
    to 3 of this module's list); why not, if it could not. With `serving`, both programs are
    built already and the web page is served from it."""
    built = serving is None
    printed = compare.b3d(
        ["tree", t.recording, *t.common(), "--at", t.at, "--to", end, "--records"],
        built,
    )
    if printed is None:
        return "b3d tree failed"
    frames = span_frames(printed)
    view = int(str(frames[0]["view"]))
    latest = frames[0].get("view_given_ms")
    if not isinstance(latest, int):
        return f"no view has arrived by {t.at}; start the span later"
    notes: list[str] = []
    kept = [f for f in frames if f["view"] == view]
    if len(kept) < len(frames):
        notes.append(
            f"note: view {view + 1} arrives at frame {len(kept)}; the {len(frames) - len(kept)}"
            " frames after it are left out, since the web page plays one view"
        )
    frames = kept
    times = [float(str(f["t_ms"])) for f in frames]
    out = t.directory() / UNSAFE.sub("_", f"motion-to-{end}")
    out.mkdir(parents=True, exist_ok=True)

    messages = snapshot.exported(t, build=False)
    if messages is None:
        return "b3d export failed"
    views_given = frames[-1].get("views_given_ms")
    assert isinstance(views_given, list)
    listed = [float(at) for at in views_given]
    given, opened, advances, more = clock.handover(
        messages,
        listed,
        clock.script_ends(t.recording, t.seat, built),
        times[0],
        times[-1],
        float(latest),
    )
    notes += more
    job = snapshot.page_job(t, messages, opened, art, out)
    job |= {
        "preroll_ms": clock.preroll(listed, given),
        "times_ms": [time - given for time in times],
        "advances": advances,
        "json": str(out / "web.json"),
    }
    lines, failed = snapshot.run_page("motion.mjs", job, out, serving)
    if failed:
        return failed
    notes += [f"warning: web {line}" for line in lines]
    web_frames = [
        f["elements"] for f in json.loads((out / "web.json").read_text())["frames"]
    ]
    b3d_frames = [elements(f) for f in frames]

    latest = tracks.name_tracks(b3d_frames)
    only = None
    if only_locator is not None:
        only, why = chosen_track(b3d_frames, only_locator)
        if only is None:
            return why
    pairs, lonely_b3d, lonely_web = compared(web_frames, b3d_frames, latest, only)
    return Measured(
        view, float(given), times, notes, out, pairs, lonely_b3d, lonely_web
    )


def command(t: Target, end: str, only: str | None = None) -> str:
    """The `just b3d match motion` command that measures the span from `t.at` to `end`."""
    options = t.options()
    return (
        f"just b3d match motion {t.recording} --from {t.at} --to {end} "
        + " ".join(options)
        + (f" --only '{only}'" if only else "")
    )


def run(args: argparse.Namespace) -> int:
    if args.all:
        from . import motion_census

        return motion_census.run_all(args)
    if args.recording is None or args.start is None or args.end is None:
        print(
            "match motion: give a recording, --from and --to, or --all", file=sys.stderr
        )
        return 2
    t: Target = target(
        argparse.Namespace(**{**vars(args), "at": args.start, "only": None})
    )
    found = measure(t, args.end, args.only, bool(args.art), None)
    if isinstance(found, str):
        print(f"match motion: {found}", file=sys.stderr)
        return 1
    m = found
    pairs, times, out, notes = m.pairs, m.times, m.out, list(m.notes)

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

    only = args.only
    lonely_b3d = [lonely.key for lonely in m.lonely_b3d]
    lonely_web = [lonely.key for lonely in m.lonely_web]
    report = [
        command(t, args.end, only),
        f"target {t.test_name()} ({t.layout}), view {m.view}, {len(times)} frames from "
        f"{times[0]:.1f}ms to {times[-1]:.1f}ms ({times[0] - m.given:.1f}ms to "
        f"{times[-1] - m.given:.1f}ms after the views both clocks count from arrived)",
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
