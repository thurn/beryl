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
  case is the one the session sent with it (`StepTiming.case`), read from the table the step found, as the stage times it. Each step is held to its verb's values (its case's, where the case has its own):
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
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING

from . import conventions, deadline, paths, runlog, selection, shots, suites
from .motion_measure import (
    TIMED,
    SIZED,
    SHARE,
    TICK,
    EASED,
    LAYOUTS,
    CAPTURE_SCRIPT,
    MORE_ONLY,
    SPANS,
    ALONE,
    UNSTAGGERED,
    motion,
    entries,
    values_of,
    duration_of,
    arena_of,
    values_report,
    Verdict,
    held,
    exempted,
    timing_verdicts,
    meets,
    apart,
    case_in,
    curve_class,
    Seen,
    distance,
    progress_at_half,
    seen,
    capture_verdicts,
    measure_capture,
)

if TYPE_CHECKING:
    from .checks import Lane

# Seconds the capture takes after its fixtures are recorded: serving the UI and playing the
# stepped capture, 12 s to 19 s on this Mac at load 10 (2026-11-13). The fast check skips it
# when recording, which builds beryl_cli when the cloned rust/target is behind, leaves less
# than this of the box: under load, a rebuild of about 35 s and the capture passed 60 s.
CAPTURE_SECONDS = 20.0
# What the capture's verdicts follow: the stage, the capture, the tracker and the data the verbs
# are played with and held to. The fast check runs the lane when one of them changed.
INPUTS = (
    r"^ui/src/stage/|^ui/scripts/stepped\.mjs$|^ui/verbs\.json$"
    r"|^data/(ui-conventions|arena-motion)\.json$"
    r"|^tools/src/beryl_tools/(motion|motion_measure|video_measure|video_shape)\.py$"
)


def boxed_out(box: float, recording: float) -> str | None:
    """Why the fast check skips the capture, when recording the fixtures took `recording` of
    its `box` seconds and the capture would not fit in what is left; None when it fits.
    """
    contract = deadline.left()
    if contract is not None:
        # Inside a fast check, what is left of its target (deadline.py), not a box of its own.
        left = contract - selection.STARTUP
        of = "the fast check's target"
    else:
        left = box - selection.STARTUP - recording
        of = f"the {box:.0f} s box"
    if left >= CAPTURE_SECONDS:
        return None
    return (
        f"recording the fixtures took {recording:.0f} s, leaving {max(left, 0):.0f} s of"
        f" {of} for a capture of about {CAPTURE_SECONDS:.0f} s; the nightly plays it"
    )


def capture(
    out: Path, tick: float | None = None, more: bool = False, box: float | None = None
) -> int | str:
    """Records the verb suite's fixtures, serves the UI and runs the stepped capture into
    `out`, in ticks of `tick` ms (stepped.mjs's sixtieth of a second by default), with the
    moments of `more` if `more`; its exit code. With `box`, the reason it skipped the
    capture instead, when recording left too little of `box` seconds (`boxed_out`). An
    earlier capture's frames are removed first, so that none is measured again."""
    recorded = out / "fixtures"
    began = time.monotonic()
    if shots.record_fixtures(recorded, suites.verb_fixtures()):
        return 1
    if box is not None and (why := boxed_out(box, time.monotonic() - began)):
        return why
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
        code = capture(out, args.tick, args.more, args.box)
    except shots.UiError as e:
        print(f"motion: {e}", file=sys.stderr)
        code = 1
    if isinstance(code, str):
        return cut(code, time.monotonic() - began)
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


def cut(why: str, took: float) -> int:
    """Logs every verb and layout as cut by the fast check's box (`boxed_out`), which fails
    nothing."""
    print(f"motion: capture cut: {why}")
    verbs = suites.verb_suite().get("verbs")
    checks = [
        f"motion:{v}/{layout}"
        for layout in LAYOUTS
        for v in (verbs if isinstance(verbs, list) else [])
    ]
    for check in checks:
        runlog.result(check, "skip", took / max(1, len(checks)), "cut")
    return 0


def lane() -> Lane:
    """The capture as a lane of the fast check (checks.py), which runs it when its inputs
    changed, inside the browser lanes' box (selection.BOX); the nightly run always does
    (nightly.py)."""
    from .checks import BY, Lane  # It imports this module.

    box = ["--box", str(selection.BOX)]
    return Lane("motion", warm=True).step("motion", [*BY, "motion", *box])


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
        "--box",
        type=float,
        help="skip the capture, logged as cut, when recording the fixtures leaves too little"
        " of this many seconds for it (the fast check's lane)",
    )
    p.add_argument(
        "--tick",
        type=float,
        help="the capture's tick in stage ms (default a sixtieth of a second)",
    )
    p.set_defaults(func=command)
