"""`by motion --film <verb> [case]`: a verb filmed for the independent motion review, which
runs when the user asks and gates nothing (the motion-review skill,
.agents/skills/motion-review/SKILL.md). Judgement is no check: nothing here passes or fails a
verb, and nothing is logged.

It writes, into the cache (`motion/<checkout>/film/<verb>/`), all a reviewer is given:

- The stage's takes of the verb (ui/scripts/verb-film.mjs): the verb's moments of the verb
  suite played in stepped stage time from both seats in both layouts, each take as sheets of
  the whole table, of 30 frames a second, and of every tick of its key 300 ms.
- Arena's instances of the verb (data/arena-motion.json), the first few of each case, cut from
  the recordings as `by video film` cuts them, and a contact sheet of each over each seat's
  take of its case on the desktop, at one scale of the table. An instance whose recording is not on this
  machine is left out and said so.
- `brief.txt`: the verb's prose (what it shows, what Arena does, Arena's path and each filmed
  instance's note), Beryl's values beside Arena's, every sheet's path, and for each step
  filmed on the desktop what the tracker reads of it (video_measure.py, video_shape.py: its
  easing, sizes, bow, turn and squash) and the stage's own pose of its subject tick by tick.
  The tracker's reading of each Arena instance stands beside, or why it cannot follow it.

Never commit the images."""

from __future__ import annotations

import argparse
import json
import math
import re
import shutil
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import (
    conventions,
    motion,
    paths,
    shots,
    suites,
    video,
    video_measure,
    video_shape,
)

FILM_SCRIPT = "ui/scripts/verb-film.mjs"
# Arena's instances filmed of each case, unless asked for more.
INSTANCES = 2
# Frames a second of an Arena instance on a contact sheet, as the stage's beside it.
ARENA_FPS = 15
# The longest span of an instance filmed, in seconds, and the widest its frames are kept.
LONGEST = 2.0
ARENA_WIDTH = 760
# Seconds filmed past an instance's motion as it was timed: where it comes to rest.
SETTLE = 0.4
# An instance's `command`: the recording, the time and the options it was taken with.
COMMAND: re.Pattern[str] = re.compile(
    r"just video (?:film|measure) (\S+) (\S+)((?:\s+--\w+\s+\S+)*)"
)
# A card's frame, high over wide (ui/src/stage: 63 by 88).
ASPECT = 88 / 63


@dataclass(frozen=True)
class Retake:
    """Where an Arena instance is in its recording: as its `command` took it, else as its own
    record says."""

    recording: str
    start: float
    span: float
    crop: str | None


def retake(instance: dict[str, Any]) -> Retake:
    # A command may take a motion in two cuts, the second after its first's span: the span
    # reaches at least past the motion as it was timed.
    timed = (instance.get("duration_ms") or 0) + (instance.get("delay_ms") or 0)
    least = max(float(instance.get("span") or 1), timed / 1000 + SETTLE)
    match = COMMAND.search(str(instance.get("command") or ""))
    if match:
        options = dict(re.findall(r"--(\w+)\s+(\S+)", match.group(3)))
        return Retake(
            match.group(1),
            video.parse_clock(match.group(2)),
            max(least, float(options.get("span", 0))),
            options.get("crop"),
        )
    crop = instance.get("crop")
    return Retake(
        str(instance["recording"]),
        video.parse_clock(str(instance["time"])),
        least,
        ",".join(str(p) for p in crop) if crop else None,
    )


def chosen(
    reference: dict[str, Any], verb: str, case: str | None, most: int
) -> list[dict[str, Any]]:
    """Arena's instances of `verb` to film: the first `most` of each case, or of `case`."""
    taken: Counter[str] = Counter()
    out: list[dict[str, Any]] = []
    for instance in motion.entries(reference).get(verb, {}).get("instances", []):
        name = str(instance.get("case"))
        if (case is None or name == case) and taken[name] < most:
            taken[name] += 1
            out.append(instance)
    return out


def arena_frames(take: Retake, folder: Path) -> dict[str, Any]:
    """Cuts an instance's frames into `folder`, ARENA_FPS a second: each frame's path and
    time, how wide and high they are, and the share of the table's width they show."""
    source, _ = video.resolve(take.recording)
    if not source.is_file():
        raise video.VideoError(
            f"{take.recording}: its recording is not on this machine"
        )
    _, _, table, tall = video.table_box(*video_measure.frame_size(source))
    shown, high = (
        (int(p) for p in take.crop.split(",")[2:]) if take.crop else (table, tall)
    )
    width = min(shown, ARENA_WIDTH)
    count = max(1, round(min(take.span, LONGEST) * ARENA_FPS))
    folder.mkdir(parents=True)
    filters = [*video.sampled(ARENA_FPS, take.crop), f"scale={width}:-2"]
    video.ffmpeg(
        *("-ss", f"{take.start:.3f}", "-t", f"{count / ARENA_FPS:.3f}"),
        *("-i", str(source), "-vf", ",".join(filters)),
        *("-q:v", "3", f"{folder}/%03d.jpg"),
    )
    return {
        "frames": [
            {"path": str(path), "at": video.clock(take.start + n / ARENA_FPS, 2)}
            for n, path in enumerate(sorted(folder.glob("*.jpg")))
        ],
        "width": width,
        "height": width * high / shown,
        "share": shown / table,
    }


def said(found: list[tuple[str, str]]) -> str:
    return "; ".join(f"{measure} {text}" for measure, text in found)


def arena_shape(take: Retake) -> str:
    """What the tracker reads of an Arena instance's path, or why it cannot follow it."""
    try:
        source, _ = video.resolve(take.recording)
        clip = video_measure.read_recording(
            source, take.start, take.span, take.crop, video_measure.WORK_WIDTH
        )
        [card] = video_measure.measure(clip)
    except video_measure.Unmeasurable as error:
        return f"cannot follow it ({str(error).split(':')[0]}): read the contact sheet"
    except (video.VideoError, OSError) as error:
        return f"not read ({str(error).splitlines()[0]})"
    if motion.distance(card.blobs[0], card.blobs[-1]) < card.rest.size:
        return "in place: nothing travels its own size"
    return said(
        video_shape.words(
            video_shape.card_bow(clip, card), video_shape.shape(clip, card)
        )
    )


def stage_shape(clip: video_measure.Clip, rest: tuple[float, float]) -> str:
    """What the tracker reads of one step in a take's `track` frames, following what comes to
    rest at `rest` (shares of the frames' width and height)."""
    found = motion.seen(clip, (rest[0] * clip.width, rest[1] * clip.height))
    if found.failure:
        return f"cannot follow it ({found.failure.split(':')[0]}): read the sheets"
    if not found.travels:
        return "in place: nothing travels its own size"
    sizes = f"begins at {found.start:g} times its size at rest, largest {found.peak:g}"
    eased = f"half way along at {found.half:g} of its time ({found.curve})"
    return "; ".join([eased, sizes, said(video_shape.words(found.bow, found.shape))])


def center(pose: dict[str, Any]) -> tuple[float, float]:
    """The middle of a pose's card, leaned and then turned about the pose's corner as the
    stage draws it (ui/src/stage/desktop.ts `middle`)."""
    width = float(pose["width"])
    down = width * ASPECT / 2
    across = width / 2 + math.tan(math.radians(float(pose["skew"]))) * down
    turn = math.radians(float(pose["rotation"]))
    return (
        float(pose["x"]) + across * math.cos(turn) - down * math.sin(turn),
        float(pose["y"]) + across * math.sin(turn) + down * math.cos(turn),
    )


def pose_lines(step: dict[str, Any], tick: float) -> list[str]:
    """The stage's own pose of a step's subject thirty times a second as it plays, from a
    tick before to a tick after: how far along the line between its ends it is and how far
    off it to the left of its way (shares of that line), its size over its size at rest, its
    turn and lean in degrees, how much of its height shows as it turns over (`flat`), its
    opacity, its depth and its face; and the bow of the whole path. The line ends where the
    subject rests once the step is over, the tick after it, if the stage still holds it
    then. In place, where it lies instead."""
    shown = [
        p
        for p in step["poses"]
        if "x" in p and step["from"] - tick <= p["at"] <= step["to"] + tick
    ]
    if not shown:
        return ["  no card of the stage's to follow: read the sheets"]
    playing = [p for p in shown if step["from"] <= p["at"] <= step["to"]] or shown
    rest = next((p for p in shown if p["at"] > step["to"]), playing[-1])
    (ax, ay), (bx, by) = center(playing[0]), center(rest)
    dx, dy = bx - ax, by - ay
    length = math.hypot(dx, dy)
    end = float(rest["width"])
    out: list[str] = []
    if length >= 1:
        flown = playing if rest is playing[-1] else [*playing, rest]
        bow = video_shape.bow([center(p) for p in flown], [p["at"] for p in flown])
        if bow is not None:
            out.append(f"  the path's {said(video_shape.words(bow, None)[:1])}")
    out.append(
        "  ms      along   off     size   turn   lean   flat  opacity  depth  face"
        if length >= 1
        else "  ms      x       y       size   turn   lean   flat  opacity  depth  face"
    )
    # Thirty a second, as the strip: every other tick from the step's first, and its last.
    first = shown.index(playing[0])
    for n, pose in enumerate(shown):
        if (n - first) % 2 and pose is not playing[-1]:
            continue
        x, y = center(pose)
        if length >= 1:
            along = ((x - ax) * dx + (y - ay) * dy) / (length * length)
            off = ((x - ax) * dy - (y - ay) * dx) / (length * length)
            place = f"{along:7.3f} {off:7.3f}"
        else:
            place = f"{x:7.1f} {y:7.1f}"
        out.append(
            f"  {pose['at']:+5.0f}  {place}  {float(pose['width']) / end:5.2f}x"
            f"  {float(pose['turn']):5.1f}  {float(pose['rotation']):5.1f}"
            f"  {float(pose.get('flat', 1)):5.2f}"
            f"  {float(pose['opacity']):6.2f}  {float(pose['depth']):6.0f}  {pose['face']}"
        )
    return out


def span(step: dict[str, Any]) -> str:
    case = f" ({step['case']})" if step.get("case") else ""
    return f"{step.get('verb') or step['act']['verb']}{case} {step['from']:+.0f} to {step['to']:+.0f}"


def take_lines(take: dict[str, Any], tick: float) -> list[str]:
    """One take's part of the brief."""
    out = [
        f"{take['layout']} / {take['seat']} / {take['name']}"
        f"   (fixture {take['fixture']}, {take['to'] - take['from']:.0f} ms)",
        "  steps: " + ", ".join(span(step) for step in take["steps"]),
        "  playing meanwhile: " + ", ".join(span(step) for step in take["during"]),
    ]
    for kind in ("table", "strip", "ticks"):
        out += [f"  {kind}: {path}" for path in take["sheets"][kind]]
    clip = None
    if take["layout"] == "desktop":
        try:
            clip = video_measure.read_folder(
                Path(take["track"]), "*", None, video_measure.WORK_WIDTH, None
            )
        except video.VideoError as error:
            out.append(f"  tracker: not read ({str(error).splitlines()[0]})")
    for step in take["steps"]:
        out.append(f"  step {step['n']}, {span(step)}:")
        if clip is not None:
            x, y = step["rest"]
            out.append(f"  tracker: {stage_shape(clip, (float(x), float(y)))}")
        out += pose_lines(step, tick)
    return out


def brief(
    data: dict[str, Any],
    reference: dict[str, Any],
    verb: str,
    case: str | None,
    filmed: dict[str, Any],
    arena: list[dict[str, Any]],
    missing: list[str],
) -> str:
    """Everything a reviewer is given, as text: the verb's prose, its values, Arena's
    instances and the stage's takes with each sheet's path."""
    declared = next(v for v in data.get("verbs", []) if v["name"] == verb)
    entry = motion.entries(reference).get(verb, {})
    values = [
        line
        for line in motion.values_report(data, reference)
        if line.startswith((f"{verb}.", f"{verb} ("))
        and (case is None or line.startswith((f"{verb}.", f"{verb} ({case})")))
    ]
    out = [
        f"MOTION REVIEW: {verb}" + (f", {case}" if case else ""),
        "",
        "THE VERB",
        f"Shows: {declared.get('shows', '')}",
        f"Arena does: {declared.get('arena', {}).get('does', 'not seen in Arena')}",
        f"Arena's path: {entry.get('path', 'none taken')}",
        *(f"Gap in Arena's evidence: {gap}" for gap in entry.get("gaps", [])),
        "",
        "BERYL'S VALUES, Arena's summary beside as reference (never a target)",
        *values,
        "",
        "ARENA'S INSTANCES FILMED (data/arena-motion.json)",
    ]
    contacts: dict[str, list[str]] = {}
    for contact in filmed.get("contacts", []):
        contacts.setdefault(str(contact["instance"]), []).append(str(contact["path"]))
    for instance in arena:
        record = instance["instance"]
        out += [
            instance["label"],
            f"  measured: {record.get('duration_ms')} ms, curve {record.get('curve')},"
            f" by {record.get('method')}",
            f"  note: {record.get('note', '')}",
            f"  tracker: {instance['tracker']}",
        ]
        out += [
            f"  contact sheet: {path}" for path in contacts.get(instance["label"], [])
        ]
        if instance["label"] not in contacts:
            out.append(
                "  contact sheet: none, the desktop's stage played none of this case"
            )
    out += [f"  left out: {why}" for why in missing] or []
    if not arena:
        out.append("  none")
    out += ["", "BERYL'S TAKES (times in stage ms from each take's start)"]
    for take in filmed.get("takes", []):
        out += take_lines(take, float(filmed.get("tick") or motion.TICK))
        out.append("")
    return "\n".join(out)


def fixtures_of(verb: str) -> list[str]:
    """The fixtures the verb's moments play: the suite's first moment whole, and the moments
    it watches besides, one a seat (ui/verbs.json)."""
    suite: dict[str, Any] = json.loads((paths.REPO / suites.VERBS).read_text())
    found = [seat["whole"] for seat in suite["moment"].values()]
    for moment in suite.get("watches", {}).get(verb, []):
        found += list(suite["more"][moment].values())
    return sorted({str(f) for f in found})


def command(args: argparse.Namespace) -> int:
    try:
        data = conventions.load()
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1
    reference = motion.motion()
    verb, case = str(args.film[0]), " ".join(args.film[1:]) or None
    declared = {v["name"]: v for v in data.get("verbs", [])}.get(verb)
    suite = suites.verb_suite().get("verbs")
    if declared is None or not isinstance(suite, list) or verb not in suite:
        print(f"motion: the verb suite has no verb {verb!r}", file=sys.stderr)
        return 2
    if case is not None and case not in declared.get("cases", {}):
        known = ", ".join(declared.get("cases", {})) or "none"
        print(f"motion: {verb} has no case {case!r}: {known}", file=sys.stderr)
        return 2
    out = paths.cache_root() / "motion" / paths.REPO.name / "film" / verb
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    arena: list[dict[str, Any]] = []
    missing: list[str] = []
    for n, instance in enumerate(chosen(reference, verb, case, int(args.instances))):
        take = retake(instance)
        label = (
            f"{instance.get('case')} · {take.recording} {video.clock(take.start, 2)}"
        )
        try:
            frames = arena_frames(take, out / "arena" / f"{n + 1:02d}")
        except video.VideoError as error:
            missing.append(f"{label}: {str(error).splitlines()[0]}")
            continue
        arena.append(
            {
                "case": instance.get("case"),
                "label": label,
                "instance": instance,
                "tracker": arena_shape(take),
                **frames,
            }
        )
    manifest = out / "arena.json"
    manifest.write_text(json.dumps({"instances": arena}, indent=1) + "\n")
    recorded = out.parent / "fixtures"
    if shots.record_fixtures(recorded, fixtures_of(verb)):
        print("motion: recording the fixtures failed", file=sys.stderr)
        return 1
    env = shots.browser_env()
    options = ["--case", case] if case else []
    options += ["--all"] if args.all else []
    try:
        with shots.served_ui(env) as url:
            code = subprocess.run(
                ["node", str(paths.REPO / FILM_SCRIPT), url, str(recorded), str(out)]
                + [verb, "--arena", str(manifest), *options],
                cwd=paths.UI,
                env=env,
                stdin=subprocess.DEVNULL,
                check=False,
            ).returncode
    except shots.UiError as e:
        print(f"motion: {e}", file=sys.stderr)
        code = 1
    takes = out / "takes.json"
    if not takes.is_file():
        print("motion: the film failed", file=sys.stderr)
        return code or 1
    filmed: dict[str, Any] = json.loads(takes.read_text())
    text = brief(data, reference, verb, case, filmed, arena, missing)
    (out / "brief.txt").write_text(text + "\n")
    for take in filmed["takes"]:
        print(f"{take['layout']}/{take['seat']}/{take['name']}:")
        for kind in ("table", "strip", "ticks"):
            for path in take["sheets"][kind]:
                print(f"  {kind}: {path}")
    for contact in filmed["contacts"]:
        print(
            f"Arena over Beryl ({contact['seat']}), {contact['instance']}: {contact['path']}"
        )
    for why in missing:
        print(f"left out: {why}")
    if not filmed["takes"]:
        print(f"motion: the stage played no {verb} to film", file=sys.stderr)
        code = code or 1
    print(f"brief: {out / 'brief.txt'}")
    return code
