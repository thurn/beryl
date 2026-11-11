"""`by motion`: every verb held to Arena's numbers within a tolerance (docs/project-50k.md, K27,
Motion oracle). The reference is `data/arena-motion.json`, Arena's instances of each verb and
their summaries (conventions.py).

- The values check needs no browser: each verb's duration, delay and stagger in
  `data/ui-conventions.json` against Arena's range for them. tests/test_motion.py holds the tree
  to it in the python lane; `by motion --values` prints it.
- The stepped capture: ui/scripts/stepped.mjs plays the verb suite's moment (ui/verbs.json) on
  the stage in stepped stage time and screenshots every tick of each step of the suite's verbs.
  `by video measure` (video_measure.py) reads each step's frames as it read Arena's clips, and
  what it finds is held to Arena's: whether the step's subject travels, how its progress is
  eased, and its size where it begins and at its peak, as multiples of its size at rest. Stage
  time, never wall time, so machine load cannot change a result. `by motion` runs it: the fast
  check when the stage or these data change (checks.py) and nightly (nightly.py), with one
  run-log record a verb and layout, `motion:<verb>/<layout>`.

The tolerance is Arena's range over a verb's instances (or its case's, where its cases differ),
widened by one frame of the coarsest recording behind it or a share of the median, whichever is
larger: the data's `tolerance`, which a verb may override with a reason. A verb that a numbered
exception covers (`exempt` in ui-conventions) is skipped and listed. A measure that cannot be
brought in after three attempts is waived in the data, naming the bead that follows it up;
never tune a verb to one clip.
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from . import conventions, paths, runlog, selection, shots, video_measure

if TYPE_CHECKING:
    from .checks import Lane

# The measures a value or a capture is held to, and a waiver may name.
TIMED: tuple[str, ...] = ("duration", "delay", "stagger")
SIZED: tuple[str, ...] = ("peak", "start")
MEASURES: tuple[str, ...] = (*TIMED, *SIZED, "curve", "travel", "tracked")
# How far from half way a motion must be at half its time to ease out (past it) or in (short
# of it). A tracked flight that does not ease reads within 0.02 of half way, and one easing as
# CSS's ease-in or ease-out does reads 0.1 or more from it, growing or shrinking as it goes:
# the tracker drops a motion's lightest frames, so a growing card is followed late and a
# shrinking one stops early (tests/test_motion.py).
EASED = 0.05
# A layout the capture plays (ui/scripts/stepped.mjs LAYOUTS).
LAYOUTS = ("desktop",)
CAPTURE_SCRIPT = "ui/scripts/stepped.mjs"
# What the capture's verdicts follow: the stage, the capture, the tracker and the data the verbs
# are played with and held to. The fast check runs the lane when one of them changed.
INPUTS = (
    r"^ui/src/stage/|^ui/scripts/stepped\.mjs$|^ui/verbs\.json$"
    r"|^data/(ui-conventions|arena-motion)\.json$"
    r"|^tools/src/beryl_tools/(motion|video_measure)\.py$"
)


def motion() -> dict[str, Any]:
    return json.loads(conventions.MOTION.read_text())


def entries(reference: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Arena's entry for each verb, by name."""
    return {e["verb"]: e for e in reference.get("verbs", [])}


def data_problems(reference: dict[str, Any], verbs: list[str]) -> list[str]:
    """Why the tolerance or a waiver in Arena's data does not say what it must."""
    found: list[str] = []
    default = reference.get("tolerance", {})
    for key in ("frames", "share", "fps"):
        if not isinstance(default.get(key), (int, float)) or default[key] <= 0:
            found.append(f"arena-motion tolerance: needs {key} above 0")
    if not all(isinstance(v, (int, float)) for v in default.get("fps_of", {}).values()):
        found.append(
            "arena-motion tolerance: fps_of gives a recording's frames a second"
        )
    for name, entry in entries(reference).items():
        own = entry.get("tolerance")
        if own is not None and (
            not own.get("reason") or set(own) - {"frames", "share", "reason"}
        ):
            found.append(
                f"arena-motion {name}: a tolerance sets frames or share, with a reason"
            )
        cases = entry.get("summary", {}).get("cases", {})
        for waiver in entry.get("waivers", []):
            where = f"arena-motion {name} waiver"
            if waiver.get("measure") not in MEASURES:
                found.append(f"{where}: measure is one of {MEASURES}")
            if not conventions.REPORT.match(str(waiver.get("bead", ""))):
                found.append(f"{where}: names the bead that follows it up")
            if not waiver.get("reason"):
                found.append(f"{where}: says why, with the value measured")
            if "case" in waiver and waiver["case"] not in cases:
                found.append(f"{where}: {waiver['case']!r} is none of its cases")
        if name not in verbs:
            found.append(f"arena-motion {name}: no such verb")
    return found


def frame_ms(reference: dict[str, Any], instances: list[dict[str, Any]]) -> float:
    """One frame of the coarsest recording among `instances`, in ms."""
    rates = reference["tolerance"]
    fps = [rates.get("fps_of", {}).get(i["recording"], rates["fps"]) for i in instances]
    return 1000 / min(fps, default=rates["fps"])


def bounds(
    reference: dict[str, Any],
    entry: dict[str, Any],
    measure: str,
    case: str | None = None,
) -> tuple[float, float] | None:
    """What `measure` of the verb `entry` (of its `case`, if Arena has it) may be: Arena's range
    widened by the tolerance; None where Arena took none."""
    summary = entry.get("summary", {})
    if case is not None and case in summary.get("cases", {}):
        summary = summary["cases"][case]
    else:
        case = None
    field = f"{measure}_ms" if measure in TIMED else measure
    spread = summary.get(field)
    if not spread:
        return None
    tolerance = {**reference["tolerance"], **entry.get("tolerance", {})}
    widen = tolerance["share"] * spread["median"]
    if measure in TIMED:
        behind = [
            i
            for i in entry.get("instances", [])
            if i.get(field) is not None and (case is None or i.get("case") == case)
        ]
        widen = max(widen, tolerance["frames"] * frame_ms(reference, behind))
    return max(0.0, spread["min"] - widen), spread["max"] + widen


def waiver(entry: dict[str, Any], measure: str, case: str | None) -> str | None:
    """The bead a waiver of `measure` (for `case`, or for the whole verb) names, if any."""
    for w in entry.get("waivers", []):
        if w["measure"] == measure and w.get("case") in (None, case):
            return str(w["bead"])
    return None


@dataclass(frozen=True)
class Verdict:
    """One measure held to Arena's: `fault` says why it is out, `waived` names the bead that
    follows it up, and `seen` and `want` are what was found and what Arena allows."""

    verb: str
    measure: str
    seen: str
    want: str
    fault: bool
    waived: str | None = None
    case: str | None = None

    @property
    def failed(self) -> bool:
        return self.fault and self.waived is None

    def line(self) -> str:
        mark = (
            "ok"
            if not self.fault
            else f"waived ({self.waived})" if self.waived else "OUT"
        )
        case = f" ({self.case})" if self.case else ""
        return f"{mark:>18}  {self.verb}{case} {self.measure}: {self.seen}, Arena {self.want}"


def held(
    reference: dict[str, Any],
    entry: dict[str, Any],
    measure: str,
    value: float | None,
    case: str | None = None,
) -> Verdict | None:
    """`value` of `measure` held to Arena's bounds; None where either is missing."""
    span = bounds(reference, entry, measure, case)
    if value is None or span is None:
        return None
    low, high = span
    fault = not low <= value <= high
    return Verdict(
        entry["verb"],
        measure,
        f"{value:g}",
        f"{low:.4g} to {high:.4g}",
        fault,
        waiver(entry, measure, case) if fault else None,
        case,
    )


def values_check(
    data: dict[str, Any], reference: dict[str, Any]
) -> tuple[list[Verdict], list[str]]:
    """Each verb's values held to Arena's ranges, and the verbs a numbered exception exempts,
    each as `<verb> (exception <n>)`."""
    arena = entries(reference)
    verdicts: list[Verdict] = []
    exempt: list[str] = []
    for verb in data.get("verbs", []):
        if "exempt" in verb:
            exempt.append(f"{verb['name']} (exception {verb['exempt']})")
            continue
        entry = arena.get(verb["name"])
        if entry is None or "unseen" in entry:
            continue
        for measure in TIMED:
            value = verb.get("values", {}).get(measure)
            if value is None and bounds(reference, entry, measure) is not None:
                verdicts.append(
                    Verdict(verb["name"], measure, "none", "a value", True, None)
                )
            elif (found := held(reference, entry, measure, value)) is not None:
                verdicts.append(found)
    return verdicts, exempt


# The capture: what the tracker found of a step, and Arena's case of it.


def case_of(act: dict[str, Any], seat: str) -> str | None:
    """Arena's name for the case of the step `act`, as `seat` saw it; None for a case Arena's
    instances do not have, which is held to the verb's whole summary."""
    verb = act.get("verb")
    origin = act.get("from")
    zone = (act.get("object") or {}).get("zone")
    if verb == "fly":
        cases = {
            ("hand", "stack"): (
                "own spell cast" if seat == "yours" else "opponent spell cast"
            ),
            ("stack", "battlefield"): "permanent resolves",
            ("hand", "battlefield"): "land played",
            ("hand", "graveyard"): "discard",
            ("library", "hand"): "draw" if seat == "yours" else None,
        }
        return cases.get((origin, zone))
    if verb == "emerge":
        triggered = (act.get("item") or {}).get("triggered")
        return "triggered ability" if triggered else "activated ability"
    if verb == "appear" and origin == "nowhere":
        return "token"
    if verb == "vanish" and (act.get("subject") or {}).get("kind") == "ability":
        return "ability finished"
    return None


def curve_class(curve: str | None) -> str | None:
    """A curve as a capture can tell it: easing in, out or neither; None where nothing travels."""
    if curve in (None, "none"):
        return None
    return curve if curve in ("ease-in", "ease-out") else "even"


@dataclass(frozen=True)
class Seen:
    """A step as the tracker found it: whether its subject travelled; how far along its path
    it was at half its time; its size where it began and at its peak over its size at rest;
    or why it could not be followed."""

    travels: bool
    half: float | None = None
    start: float | None = None
    peak: float | None = None
    failure: str | None = None

    @property
    def curve(self) -> str | None:
        if self.half is None:
            return None
        if self.half >= 0.5 + EASED:
            return "ease-out"
        return "ease-in" if self.half <= 0.5 - EASED else "even"


def distance(a: video_measure.Blob, b: video_measure.Blob) -> float:
    (ax, ay), (bx, by) = a.center, b.center
    return math.hypot(bx - ax, by - ay)


def progress_at_half(card: video_measure.Card) -> float:
    """How far along the line from where the card was first seen to where it was last seen it
    was at half the time between, by its blobs' centers."""
    first, last = card.blobs[0].center, card.blobs[-1].center
    dx, dy = last[0] - first[0], last[1] - first[1]
    length = dx * dx + dy * dy
    along = [
        ((b.center[0] - first[0]) * dx + (b.center[1] - first[1]) * dy) / length
        for b in card.blobs
    ]
    middle = (card.times[0] + card.times[-1]) / 2
    for (t0, p0), (t1, p1) in zip(
        zip(card.times, along), zip(card.times[1:], along[1:])
    ):
        if t0 <= middle <= t1:
            return p0 + (p1 - p0) * (middle - t0) / (t1 - t0)
    return along[-1]


def seen(clip: video_measure.Clip, landing: tuple[float, float] | None = None) -> Seen:
    """What the tracker finds of one step's frames, following only what comes to rest over
    `landing` (working pixels) when given. A step whose subject the tracker sees no motion
    of, such as a card coming to be in place within a tick, did not travel; nor did one that
    moved less than its own size."""
    try:
        card = video_measure.measure(clip, landing=landing)[0]
    except video_measure.Unmeasurable as error:
        if not error.moved:
            return Seen(travels=False)
        far = any(distance(t[0], t[-1]) >= max(b.size for b in t) for t in error.moved)
        return Seen(travels=far, failure=str(error))
    if distance(card.blobs[0], card.blobs[-1]) < card.rest.size:
        return Seen(travels=False, peak=round(card.peak[0], 3))
    return Seen(
        travels=True,
        half=round(progress_at_half(card), 3),
        start=round(card.blobs[0].size / card.rest.size, 3),
        peak=round(card.peak[0], 3),
    )


def capture_verdicts(
    reference: dict[str, Any], verb: str, case: str | None, found: Seen
) -> list[Verdict]:
    """What the tracker found of one step of `verb`, held to Arena's case of it: whether it
    travels and how it eases by Arena's curve (nothing travels for `none`, and neither is held
    where Arena took no curve), and its sizes by Arena's."""
    entry: dict[str, Any] = entries(reference)[verb]
    summary = entry.get("summary", {})
    # The case's curve, or where Arena took none for it, the verb's commonest.
    curve = summary.get("curve")
    if case is not None and case in summary.get("cases", {}):
        curve = summary["cases"][case].get("curve") or curve
    else:
        case = None
    if curve is None:
        return [
            v
            for measure in SIZED
            if (v := held(reference, entry, measure, getattr(found, measure), case))
        ]
    want = curve_class(curve)
    travels = want is not None

    def verdict(measure: str, seen: str, wanted: str, fault: bool) -> Verdict:
        bead = waiver(entry, measure, case) if fault else None
        return Verdict(verb, measure, seen, wanted, fault, bead, case)

    out = [
        verdict(
            "travel",
            "travels" if found.travels else "in place",
            "travels" if travels else "in place",
            found.travels != travels,
        )
    ]
    if found.failure and travels:
        out.append(verdict("tracked", found.failure, "one mover", True))
    if travels and found.curve is not None:
        out.append(
            verdict(
                "curve",
                f"{found.curve} (half way at {found.half:g})",
                want or "",
                found.curve != want,
            )
        )
    for measure in SIZED:
        value = getattr(found, measure)
        if (v := held(reference, entry, measure, value, case)) is not None:
            out.append(v)
    return out


def measure_capture(
    reference: dict[str, Any], folder: Path
) -> list[tuple[str, str, list[Verdict]]]:
    """Each captured step under `folder` (stepped.mjs's output): its verb, its folder's name and
    its verdicts."""
    out: list[tuple[str, str, list[Verdict]]] = []
    for steps_file in sorted(folder.glob("*/*/steps.json")):
        captured = json.loads(steps_file.read_text())
        for step in captured["steps"]:
            frames = steps_file.parent / step["folder"]
            name = f"{captured['layout']}/{captured['seat']}/{step['folder']}"
            clip = video_measure.read_folder(
                frames, "*", None, video_measure.WORK_WIDTH, None
            )
            case = case_of(step["act"], captured["seat"])
            x, y = step["rest"]
            found = seen(clip, (x * clip.width, y * clip.height))
            out.append(
                (
                    step["verb"],
                    name,
                    capture_verdicts(reference, step["verb"], case, found),
                )
            )
    return out


def capture(out: Path, tick: float | None = None) -> int:
    """Records the verb suite's fixtures, serves the UI and runs the stepped capture into
    `out`, in ticks of `tick` ms (stepped.mjs's sixtieth of a second by default); its exit
    code."""
    recorded = out / "fixtures"
    if shots.record_fixtures(recorded, selection.verb_fixtures()):
        return 1
    env = shots.browser_env()
    ticks = ["--tick", str(tick)] if tick else []
    with shots.served_ui(env) as url:
        return subprocess.run(
            [
                "node",
                str(paths.REPO / CAPTURE_SCRIPT),
                url,
                str(recorded),
                str(out / "frames"),
            ]
            + ticks,
            cwd=paths.UI,
            env=env,
            stdin=subprocess.DEVNULL,
            check=False,
        ).returncode


def command(args: argparse.Namespace) -> int:
    data = conventions.load()
    reference = motion()
    problems = data_problems(reference, [v["name"] for v in data["verbs"]])
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    if args.values:
        verdicts, exempt = values_check(data, reference)
        for v in verdicts:
            print(v.line())
        print(f"exempt: {', '.join(exempt) or 'none'}")
        return 1 if any(v.failed for v in verdicts) else 0
    out = paths.cache_root() / "motion" / paths.REPO.name
    began = time.monotonic()
    try:
        code = capture(out, args.tick)
    except shots.UiError as e:
        print(f"motion: {e}", file=sys.stderr)
        code = 1
    if code:
        print("motion: the capture failed", file=sys.stderr)
        return code
    measured = measure_capture(reference, out / "frames")
    verbs = selection.verb_suite().get("verbs")
    verbs = [str(v) for v in verbs] if isinstance(verbs, list) else []
    took = (time.monotonic() - began) / max(1, len(verbs) * len(LAYOUTS))
    failed = False
    for layout in LAYOUTS:
        for verb in verbs:
            mine = [
                (name, vs)
                for v, name, vs in measured
                if v == verb and name.startswith(f"{layout}/")
            ]
            bad = not mine or any(v.failed for _, vs in mine for v in vs)
            failed = failed or bad
            runlog.result(f"motion:{verb}/{layout}", "fail" if bad else "pass", took)
            if not mine:
                print(f"{layout}: the stage played no {verb} to capture")
            for name, vs in mine:
                print(f"{name}:")
                for v in vs:
                    print(f"  {v.line()}")
    print(f"motion: frames in {out / 'frames'}")
    return 1 if failed else 0


def lane() -> Lane:
    """The capture as a lane of the fast check (checks.py), which runs it when its inputs
    changed; the nightly run always does (nightly.py)."""
    from .checks import BY, Lane  # It imports this module.

    return Lane("motion", warm=True).step("motion", [*BY, "motion"])


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "motion",
        help="hold each verb to Arena's numbers: the stepped capture, or --values",
    )
    p.add_argument(
        "--values",
        action="store_true",
        help="only the verbs' durations, delays and staggers, with no browser",
    )
    p.add_argument(
        "--tick",
        type=float,
        help="the capture's tick in stage ms (default a sixtieth of a second)",
    )
    p.set_defaults(func=command)
