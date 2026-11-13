"""`by motion`: the stage held to each verb's own values in `data/ui-conventions.json`, with
Arena's numbers (`data/arena-motion.json`) beside them as reference and no verdict
. This is a regression guard: it fails when the stage stops playing a
verb as its values say.

- `by motion --values` needs no browser: each verb's values, and each case's, beside Arena's
  summary of its instances.
- `by motion --film <verb> [case]` films a verb for the independent motion review, which runs
  when the user asks and gates nothing: motion_film.py.
- The stepped capture: ui/scripts/stepped.mjs plays the verb suite's moments (ui/verbs.json)
  on the stage in stepped stage time: the first (a cast, its trigger and tokens), every step of
  the suite's verbs screenshotted at every tick; and with `--more` the moments of cards
  changing zone (`zones` and `scry`: fly, vanish and lift; `look`, a scry's look left open, so
  that the opponent's lifted card rests) and of changes on a card where it lies and to players
  (`combat` and `place`: tap, strike, count and flip), every step of those verbs timed and the
  first of each kind (a case, whose flight it is, alone or among like steps) screenshotted,
  its subject drawn alone, with a plate of the table with no node drawn, which the tracker
  finds the subject against: whole in every frame, however much of its path overlaps where it
  lay or rests (a card lifted off a pile and growing as it goes; by-w8gv.21.46). A step's
  case is the page's (src/stage/timing.ts caseOf), read from the table the step found, as the stage times it. Each step is held to its verb's values (its case's, where the case has its own):
  how long it played and, in a batch of like steps, how far apart they began, by the step's own
  start and end in stage time; and for a step screenshotted, whether its subject travels and
  how its progress is eased, by what `by video measure` (video_measure.py) reads of its frames
  as it read Arena's clips. Its size where it begins and at its peak, as multiples of its size
  at rest, are printed beside Arena's, and how its path bows, how far it turns and how it is
  squashed (video_shape.py); no value declares them. A step whose subject moves
  again at once or comes to rest out of the window is not screenshotted (the tracker needs it
  still either side and at rest in the frames), and says so. Stage time, never wall time, so
  machine load cannot change a result. `by motion` runs it: the fast check when the stage or
  these data change (checks.py) and nightly (nightly.py), with one run-log record a verb and
  layout, `motion:<verb>/<layout>`.
- What the tracker can hold of the verbs that change a card where it lies (by-w8gv.21.41).
  A flip grows and turns over in place (src/stage/tracks.ts FLIP): its travel is held (in
  place) and its peak size shown. A strike lunges at what it hits and drifts back to where it
  lay, which the tracker, measuring a card from one rest to the next, cannot follow (it reads
  more than one mover, or a card that never comes to rest): the verb declares no curve, so its
  duration and stagger are held and what the tracker found is shown. A count of
  counters changes a badge on a card that stays put: its travel is held (in place). A count of
  life moves no card: the screen draws the change beside the life orb (src/stage/Chrome.tsx
  LifeChange) and an effect of the stage's flares the orb and drifts the change on the stage's
  clock (src/stage/effects.ts), which no tracker of cards reads: it is only timed. A tap turns
  its card a quarter about its own middle where it lies, which is no travel, before it and its
  row go to their places, and video_shape reads a turn only of a card the tracker follows from
  one place to another: its steps are timed, as every verb's are against its own values, and
  never screenshotted, until the motion review wants a measure of a card turning where it
  lies. The two blows of one fight begin at one time (`meets`), not a stagger apart.
- The verbs that move no card are only timed: a streak is a comet of light drawn between two
  things (src/stage/streaks.ts), an announcement a banner the chrome draws from a number the
  stage writes (src/stage/overlay.ts), a dwell a wait, a set a change with no motion, and a
  showcase a face that grows where it rests in the Revealed Cards pile, as Arena's does. An
  announcement still passing over the table when a moment's recording ends is told no end and
  is not timed. The first moment plays mana's streaks and, from the seat across, the dwell on
  the opponent's spell; `--more` has the rest (a turn's banner, a scry's pill, a coin, damage).
- `--more` runs nightly only: its moments add about two minutes to the lane, past the fast
  check's 60 s (by-w8gv.21.38: the capture took 107 to 127 s with the moments of cards
  changing zone and about 20 s without, at loads of 25 to 50 on 18 cores, `zones` nearly all
  of it, its ten steps screenshotted about 350 frames a seat; by-w8gv.21.41: about 145 s with
  `combat` and `place` too, at loads of 20 to 30, their seven steps shot about 360 frames a
  seat). The fast check skips the verbs only they play (MORE_ONLY).

A number is held within one tick of the capture or SHARE of its value, whichever is larger. A
step's delay counts from its turn in its beat, but it begins no earlier than the step before
it, so its start is not held to its delay; it is to the stagger of a like step before it.
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from . import conventions, paths, runlog, shots, suites, video_measure, video_shape

if TYPE_CHECKING:
    from .checks import Lane

TIMED: tuple[str, ...] = ("duration", "delay", "stagger")
SIZED: tuple[str, ...] = ("peak", "start")
# How far a step's timing may be from its value, as a share of it, when that is more than a
# tick: the stage plays a step on whole ticks of the capture.
SHARE = 0.15
# The capture's tick in stage ms, where its record names none (ui/scripts/stepped.mjs TICK).
TICK = 1000 / 60
# How far from half way a motion must be at half its time to ease out (past it) or in (short
# of it). A tracked flight that does not ease reads within 0.02 of half way, and one easing as
# CSS's ease-in or ease-out does reads 0.1 or more from it, growing or shrinking as it goes:
# the tracker drops a motion's lightest frames, so a growing card is followed late and a
# shrinking one stops early (tests/test_motion.py).
EASED = 0.05
# A layout the capture plays (ui/scripts/stepped.mjs LAYOUTS).
LAYOUTS = ("desktop",)
CAPTURE_SCRIPT = "ui/scripts/stepped.mjs"
# The suite's verbs only the moments of `--more` play, which the fast check leaves to the
# nightly run: logged as skipped without it.
MORE_ONLY: dict[str, str] = {
    verb: "only the moments of `--more` play it, captured nightly (`by motion --more`)"
    for verb in ("lift", "strike", "count", "flip", "announce", "showcase")
}
# The verbs whose steps begin and end a span (an arrow, a line, a mark). The stage plays one
# with the step that calls for it and staggers none (ui/src/stage/timing.ts), and an end's
# delay is how long its arrow has stood whole, so only how long each plays is held.
SPANS: tuple[str, ...] = ("point", "mark")
# The verbs whose steps are each alone in a beat (a step of the turn beginning, a wait, an
# announcement): the next like step in the script is another beat's, a whole step later, and
# no stagger is held between the two.
ALONE: tuple[str, ...] = ("set", "dwell", "announce")
UNSTAGGERED: tuple[str, ...] = SPANS + ALONE
# What the capture's verdicts follow: the stage, the capture, the tracker and the data the verbs
# are played with and held to. The fast check runs the lane when one of them changed.
INPUTS = (
    r"^ui/src/stage/|^ui/scripts/stepped\.mjs$|^ui/verbs\.json$"
    r"|^data/(ui-conventions|arena-motion)\.json$"
    r"|^tools/src/beryl_tools/(motion|video_measure|video_shape)\.py$"
)


def motion() -> dict[str, Any]:
    return json.loads(conventions.MOTION.read_text())


def entries(reference: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Arena's entry for each verb, by name."""
    return {e["verb"]: e for e in reference.get("verbs", [])}


def values_of(verb: dict[str, Any], case: str | None) -> dict[str, Any]:
    """What the stage plays a step of `verb`'s `case` with: the case's own values, each it
    leaves null being the verb's (ui/src/stage/timing.ts)."""
    whole: dict[str, Any] = verb.get("values", {})
    own = verb.get("cases", {}).get(case) if case is not None else None
    if own is None:
        return dict(whole)
    return {
        v: own.get(v) if own.get(v) is not None else whole.get(v)
        for v in conventions.VALUES
    }


def duration_of(act: dict[str, Any], values: dict[str, Any]) -> float:
    """How long the stage plays the step `act` with `values`: its duration, but for a look at
    several cards of a library, which lifts them one after another, each as long as one
    alone (ui/src/stage/timing.ts `timingOf`)."""
    duration = float(values.get("duration") or 0)
    handling: dict[str, Any] = act.get("handling") or {}
    count = int(handling.get("count") or 0)
    if (
        act.get("verb") == "lift"
        and act.get("span") == "begin"
        and handling.get("zone") == "library"
        and count > 1
        and duration > 0
    ):
        return duration + (count - 1) * float(values.get("stagger") or 0)
    return duration


def arena_of(
    reference: dict[str, Any], verb: str, measure: str, case: str | None
) -> str | None:
    """Arena's summary of `measure` for `verb` (its `case`'s, where Arena's instances have
    it), in words: a curve, or a median and range; None where Arena took none."""
    summary = entries(reference).get(verb, {}).get("summary", {})
    if case is not None and case in summary.get("cases", {}):
        summary = summary["cases"][case]
    if measure == "curve":
        return summary.get("curve")
    spread = summary.get(f"{measure}_ms" if measure in TIMED else measure)
    if not spread:
        return None
    unit = " ms" if measure in TIMED else ""
    return f"{spread['median']:g}{unit} ({spread['min']:g} to {spread['max']:g})"


def values_report(data: dict[str, Any], reference: dict[str, Any]) -> list[str]:
    """Each verb's values and each case's, beside Arena's summary of them: no verdict."""
    out: list[str] = []
    for verb in data.get("verbs", []):
        if "exempt" in verb:
            out.append(f"{verb['name']}. Exempt: exception {verb['exempt']}.")
            continue
        for case in [None, *verb.get("cases", {})]:
            said = conventions.timing({"values": values_of(verb, case)}).strip()
            arena = [
                f"{measure} {found}"
                for measure in (*TIMED, "curve")
                if (found := arena_of(reference, verb["name"], measure, case))
            ]
            name = f"{verb['name']} ({case})" if case else verb["name"]
            out.append(
                f"{name}. {said or 'None.'} Arena: {', '.join(arena) or 'none'}."
            )
    return out


@dataclass(frozen=True)
class Verdict:
    """One measure of a step: `seen` is what the capture found, `want` the verb's value it is
    held to (None for a measure only shown beside Arena's), `failed` whether it is out, and
    `arena` Arena's summary of the measure, as reference."""

    verb: str
    measure: str
    seen: str
    want: str | None
    failed: bool
    case: str | None = None
    arena: str | None = None

    def line(self) -> str:
        mark = "OUT" if self.failed else "ok" if self.want is not None else "seen"
        case = f" ({self.case})" if self.case else ""
        want = f", Beryl {self.want}" if self.want is not None else ""
        arena = f"; Arena {self.arena}" if self.arena else ""
        return f"{mark:>4}  {self.verb}{case} {self.measure}: {self.seen}{want}{arena}"


def held(
    reference: dict[str, Any],
    verb: str,
    measure: str,
    seen: float,
    value: float,
    case: str | None,
    tick: float,
) -> Verdict:
    """A step's `measure`, `seen` in stage ms, held to its verb's `value` within a `tick` or
    SHARE of it, whichever is larger."""
    widen = max(tick, SHARE * value)
    return Verdict(
        verb,
        measure,
        f"{seen:g} ms",
        f"{value:g} ms (±{widen:.3g})",
        abs(seen - value) > widen,
        case,
        arena_of(reference, verb, measure, case),
    )


def exempted(data: dict[str, Any]) -> dict[str, int]:
    """The verbs a numbered exception exempts, each with its exception: stepped.mjs times their
    steps but shoots none. A permanent that taps turns where it lies, which the tracker, made
    to follow a card that travels, does not read."""
    return {v["name"]: v["exempt"] for v in data.get("verbs", []) if "exempt" in v}


def timing_verdicts(
    data: dict[str, Any],
    reference: dict[str, Any],
    captured: dict[str, Any],
) -> list[list[Verdict]]:
    """For each step a capture recorded (stepped.mjs's steps.json), how long it played held to
    its verb's duration and, where the next step is the same verb and case right after it in
    the script, how long after it that one began held to its stagger (`apart`), or to no time
    at all where the two are one fight's blows (`meets`). A value the data leaves null is
    played as 0 (timing.ts), and held so."""
    verbs = {v["name"]: v for v in data.get("verbs", [])}
    tick = float(captured.get("tick") or TICK)
    seat = captured["seat"]
    steps: list[dict[str, Any]] = captured["steps"]
    out: list[list[Verdict]] = []
    for step, after in zip(steps, [*steps[1:], None]):
        verb = verbs.get(step["verb"], {})
        if not verb:
            out.append([])
            continue
        case = case_in(step, seat)
        values = values_of(verb, case)
        found = [
            held(
                reference,
                step["verb"],
                "duration",
                step["to"] - step["from"],
                duration_of(step["act"], values),
                case,
                tick,
            )
        ]
        if (
            after is not None
            and step["verb"] not in UNSTAGGERED
            and after["n"] == step["n"] + 1
            and after["verb"] == step["verb"]
            and case_in(after, seat) == case
        ):
            found.append(
                held(
                    reference,
                    step["verb"],
                    "stagger",
                    after["from"] - step["from"],
                    0.0 if meets(step["act"], after["act"]) else apart(step, values),
                    case,
                    tick,
                )
            )
        out.append(found)
    return out


def meets(act: dict[str, Any], after: dict[str, Any]) -> bool:
    """Whether the strikes `act` and `after` are the two blows of one fight, each creature
    striking the other: the stage plays them as one, at one time (ui/src/stage/tracks.ts
    `meets`)."""
    if act.get("verb") != "strike" or after.get("verb") != "strike":
        return False
    at, back = act.get("to", {}), after.get("to", {})
    return (
        at.get("kind") == "object"
        and back.get("kind") == "object"
        and at.get("slot") == after.get("source")
        and back.get("slot") == act.get("source")
    )


def apart(step: dict[str, Any], values: dict[str, Any]) -> float:
    """How long after `step`, played with `values`, a like step right after it in its beat
    (its verb and case) begins: its stagger, but for a step that began after its turn, held
    back by an earlier step's delay (the stage begins a beat's steps in order), which the next
    step's turn may not be. Where the capture recorded no turn, the stagger."""
    stagger = float(values["stagger"] or 0)
    if "turn" not in step:
        return stagger
    delay = float(values["delay"] or 0)
    return max(step["from"], step["turn"] + stagger + delay) - step["from"]


# The capture: what the tracker found of a step, and the case its verb's values are of.


def case_in(step: dict[str, Any], seat: str) -> str | None:
    """The case of a captured step: as the page told it, which reads it from the table the
    step found (a life total's rise or fall), where the capture recorded it; else as
    `case_of` reads the step's act alone."""
    if "case" in step:
        return step["case"]
    return case_of(step["act"], seat)


def case_of(act: dict[str, Any], seat: str) -> str | None:
    """The case of the step `act`, as `seat` saw it, by the name its values and Arena's
    instances have (ui/src/stage/timing.ts `caseOf` reads it the same); None for a case of
    neither, which is held to its verb's values."""
    verb = act.get("verb")
    origin = act.get("from")
    zone = (act.get("object") or {}).get("zone")
    # Whose it is: its controller's, as the step's record says, or the seat that plays the
    # capture's moment.
    controller = (act.get("object") or {}).get("controller")
    own = controller == "you" if controller else seat == "yours"
    if verb == "fly":
        cases = {
            ("hand", "stack"): "own spell cast" if own else "opponent spell cast",
            ("stack", "battlefield"): "permanent resolves",
            ("hand", "battlefield"): "land played" if own else "opponent land played",
            ("hand", "graveyard"): "discard",
        }
        if (origin, zone) == ("library", "hand"):
            # A card an effect puts into a hand is no draw. A card no one reads flies face
            # down. Whether a card read is drawn alone the act does not say: the page tells
            # a capture so (`case_in`).
            if act.get("why") != "draw":
                return None
            drawn: dict[str, Any] = act.get("object") or {}
            read = drawn.get("owner") == "you" and drawn.get("name") is not None
            return "draw" if read else "opponent draw"
        return cases.get((origin, zone))
    if verb == "emerge":
        triggered = (act.get("item") or {}).get("triggered")
        return "triggered ability" if triggered else "activated ability"
    if verb == "appear" and origin == "nowhere":
        return "token"
    if verb == "vanish" and (act.get("subject") or {}).get("kind") == "ability":
        return "ability finished"
    if verb == "vanish":
        # Where it goes says more than why: a spell an effect returns is bounced.
        left = {"hand": "bounced", "exile": "exiled"}
        why = {
            "destroy": "destroyed",
            "stateBased": "destroyed",
            "counter": "countered",
            "resolve": "resolved instant or sorcery",
            "sacrifice": "sacrificed",
        }
        return left.get(str(zone)) or why.get(str(act.get("why")))
    if verb == "tap":
        return "tap" if (act.get("object") or {}).get("tapped") else "untap"
    if verb == "strike":
        struck = (act.get("to") or {}).get("kind")
        return "unblocked attacker" if struck == "player" else "blocked attacker"
    if verb == "count":
        # A life total's case is whether it rose, which the table it finds says (the stage's
        # timing.ts caseOf reads it there): a step alone is held to the verb's whole summary.
        tally = act.get("tally") or {}
        if tally.get("kind") == "counters":
            return "loyalty" if tally.get("counter") == "loyalty" else "counter"
    if verb == "streak":
        return (act.get("stream") or {}).get("kind")
    if verb == "announce":
        kind = (act.get("notice") or {}).get("kind")
        return {"turn": "turn banner", "scry": "scry pill", "coin": "coin"}.get(kind)
    if verb == "dwell" and (act.get("subject") or {}).get("kind") == "player":
        return "game's end"
    if verb == "set":
        fact = (act.get("fact") or {}).get("kind")
        return {"turn": "step begins", "library": "library shuffled"}.get(fact)
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
    how its path bows and how far it turns and is squashed on the way (video_shape.py); or why
    it could not be followed."""

    travels: bool
    half: float | None = None
    start: float | None = None
    peak: float | None = None
    failure: str | None = None
    bow: video_shape.Bow | None = None
    shape: video_shape.Shape | None = None

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
    moved less than its own size, where it is smaller: a card that grows as it goes (lifted
    off an opponent's library, which lies small and far) travels by its size as it left.
    """
    try:
        card = video_measure.measure(clip, landing=landing)[0]
    except video_measure.Unmeasurable as error:
        if not error.moved:
            return Seen(travels=False)
        far = any(distance(t[0], t[-1]) >= max(b.size for b in t) for t in error.moved)
        return Seen(travels=far, failure=str(error))
    smallest = min(card.blobs[0].size, card.rest.size)
    if distance(card.blobs[0], card.blobs[-1]) < smallest:
        return Seen(travels=False, peak=round(card.peak[0], 3))
    return Seen(
        travels=True,
        half=round(progress_at_half(card), 3),
        start=round(card.blobs[0].size / card.rest.size, 3),
        peak=round(card.peak[0], 3),
        bow=video_shape.card_bow(clip, card),
        shape=video_shape.shape(clip, card),
    )


def capture_verdicts(
    data: dict[str, Any],
    reference: dict[str, Any],
    verb: str,
    case: str | None,
    found: Seen,
) -> list[Verdict]:
    """What the tracker found of one step of `verb`, held to its curve (its `case`'s): nothing
    travels for `none`, and neither is held where the verb has no curve. Its sizes are shown
    beside Arena's, its path's bow, its turn and its squash for the motion review, and what
    the tracker could not follow of a step held to no travel, each held to nothing."""
    declared = next(v for v in data.get("verbs", []) if v["name"] == verb)
    curve = values_of(declared, case)["curve"]
    arena_curve = arena_of(reference, verb, "curve", case)
    out: list[Verdict] = []
    if curve is not None:
        want = curve_class(curve)
        travels = want is not None
        out.append(
            Verdict(
                verb,
                "travel",
                "travels" if found.travels else "in place",
                "travels" if travels else "in place",
                found.travels != travels,
                case,
            )
        )
        if found.failure and travels:
            out.append(Verdict(verb, "tracked", found.failure, "one mover", True, case))
        if travels and found.curve is not None:
            out.append(
                Verdict(
                    verb,
                    "curve",
                    f"{found.curve} (half way at {found.half:g})",
                    want or "",
                    found.curve != want,
                    case,
                    arena_curve,
                )
            )
    if found.failure and curve_class(curve) is None:
        # What the tracker could not follow where nothing is held of how it travels, such as
        # a strike's lunge, which comes back to where it lay: shown, held to nothing.
        out.append(Verdict(verb, "tracked", found.failure, None, False, case))
    for measure in SIZED:
        value = getattr(found, measure)
        if value is not None:
            arena = arena_of(reference, verb, measure, case)
            out.append(Verdict(verb, measure, f"{value:g}", None, False, case, arena))
    if found.travels and not found.failure:
        for measure, said in video_shape.words(found.bow, found.shape):
            out.append(Verdict(verb, measure, said, None, False, case))
    return out


def measure_capture(
    data: dict[str, Any], reference: dict[str, Any], folder: Path
) -> list[tuple[str, str, list[Verdict]]]:
    """Each captured step under `folder` (stepped.mjs's output): its verb, its name (its
    layout, play and folder) and its verdicts. A step only timed, which has no frames (no
    `rest`), has those of its timing alone."""
    out: list[tuple[str, str, list[Verdict]]] = []
    for steps_file in sorted(folder.glob("*/*/steps.json")):
        captured = json.loads(steps_file.read_text())
        timings = timing_verdicts(data, reference, captured)
        for step, timed in zip(captured["steps"], timings):
            frames = steps_file.parent / step["folder"]
            name = f"{captured['layout']}/{steps_file.parent.name}/{step['folder']}"
            case = case_in(step, captured["seat"])
            if "rest" not in step:
                why = step.get("unshot")
                said = [Verdict(step["verb"], "frames", why, None, False, case)]
                out.append((step["verb"], name, timed + (said if why else [])))
                continue
            clip = video_measure.read_folder(
                frames, "*", None, video_measure.WORK_WIDTH, None
            )
            x, y = step["rest"]
            found = seen(clip, (x * clip.width, y * clip.height))
            verdicts = capture_verdicts(data, reference, step["verb"], case, found)
            out.append((step["verb"], name, timed + verdicts))
    return out


def capture(out: Path, tick: float | None = None, more: bool = False) -> int:
    """Records the verb suite's fixtures, serves the UI and runs the stepped capture into
    `out`, in ticks of `tick` ms (stepped.mjs's sixtieth of a second by default), with the
    moments of `more` if `more`; its exit code. An earlier capture's frames are
    removed first, so that none is measured again."""
    recorded = out / "fixtures"
    if shots.record_fixtures(recorded, suites.verb_fixtures()):
        return 1
    shutil.rmtree(out / "frames", ignore_errors=True)
    env = shots.browser_env()
    ticks = ["--tick", str(tick)] if tick else []
    ticks += ["--more"] if more else []
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
    try:
        data = conventions.load()
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1
    reference = motion()
    if args.film:
        from . import motion_film  # It imports this module.

        return motion_film.command(args)
    if args.values:
        print("\n".join(values_report(data, reference)))
        return 0
    out = paths.cache_root() / "motion" / paths.REPO.name
    began = time.monotonic()
    try:
        code = capture(out, args.tick, args.more)
    except shots.UiError as e:
        print(f"motion: {e}", file=sys.stderr)
        code = 1
    if code:
        print("motion: the capture failed", file=sys.stderr)
        return code
    measured = measure_capture(data, reference, out / "frames")
    verbs = suites.verb_suite().get("verbs")
    verbs = [str(v) for v in verbs] if isinstance(verbs, list) else []
    took = (time.monotonic() - began) / max(1, len(verbs) * len(LAYOUTS))
    failed = False
    exempt = exempted(data)
    for layout in LAYOUTS:
        for verb in verbs:
            mine = [
                (name, vs)
                for v, name, vs in measured
                if v == verb and name.startswith(f"{layout}/")
            ]
            if not mine and verb in exempt:
                print(
                    f"{layout}: {verb} is exempt (exception {exempt[verb]}), not played"
                )
                runlog.result(f"motion:{verb}/{layout}", "pass", took)
                continue
            if not mine:
                said = f"{layout}: the stage played no {verb} to capture"
                why = None if args.more else MORE_ONLY.get(verb)
                runlog.result(
                    f"motion:{verb}/{layout}", "skip" if why else "fail", took
                )
                print(f"{said}, skipped: {why}" if why else said)
                failed = failed or why is None
                continue
            bad = any(v.failed for _, vs in mine for v in vs)
            failed = failed or bad
            runlog.result(f"motion:{verb}/{layout}", "fail" if bad else "pass", took)
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
        help="hold the stage to each verb's own values, Arena's beside: the stepped capture,"
        " or --values; or film a verb for the motion review",
    )
    p.add_argument(
        "--values",
        action="store_true",
        help="only the verbs' values beside Arena's, with no browser",
    )
    p.add_argument(
        "--more",
        action="store_true",
        help="also the moments of cards changing zone and changing in place (nightly: past"
        " the fast check's budget)",
    )
    p.add_argument(
        "--film",
        nargs="+",
        metavar=("VERB", "CASE"),
        help="film a verb, or one case of it, for the motion review: the stage's takes as"
        " sheets, Arena's instances beside, and the reviewer's brief",
    )
    p.add_argument(
        "--all",
        action="store_true",
        help="with --film, every take of the verb, not the first of each case",
    )
    p.add_argument(
        "--instances",
        default="2",
        help="with --film, how many of Arena's instances of each case (default 2)",
    )
    p.add_argument(
        "--tick",
        type=float,
        help="the capture's tick in stage ms (default a sixtieth of a second)",
    )
    p.set_defaults(func=command)
