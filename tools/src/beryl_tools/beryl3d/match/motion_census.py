"""`just b3d match motion --all`: every verb, effect and mark that `data/beryl3d/parity.json`
names a recording for, ranked by how far its motion in Beryl 3D is from the web interface's,
worst first (docs/beryl-3d.md, "Matching the web interface's look").

1. The census, `data/beryl3d/match/motion-census.json`, holds one span for each item: for a verb
   or an effect, the first step of its recording that plays the verb or begins the effect
   (from `b3d timeline`, the step timings both clients share), from its start on Beryl 3D's
   clock, with every script played at real time (`--no-catch-up`) as `match motion` plays
   them, to `AFTER_MS` after its end; for a mark, the frames before its recording's first
   prompt is still. The file is kept so that two runs measure the same frames and their
   totals compare; `--refresh-census` finds the spans again.
2. Each span is measured as `match motion` measures one (motion.py's `measure`) at 1600x900
   and scale 2, `PAGES` browser pages at a time. A span's result is kept beside its snapshot
   and reused while its inputs (the commit, uncommitted changes to either client or these
   tools, and the span) are unchanged.
3. Each moving pair adds its `tracks.Deviation`, weighed into points by `tracks.WEIGHTS`, to
   its item's total, and each element that moves on one side only adds how far it moves.
   Element records hold no rotation on either side, so rotation is not compared. An effect's
   total counts only its own elements (role `effect` of its name: the web's `.stage-effect`
   nodes and the effects Beryl 3D's records list), not everything else that moves in its span,
   so an effect drawn on one side only adds the whole of its track.
4. Each effect is also compared in pixels at the frame where it is strongest: where its web
   element is most opaque (or its Beryl 3D element, if the web draws none), a snapshot of that
   moment counts the pixels that differ inside that element's box (`b3d-render pixels --box`,
   glyph pixels left out). `shake` moves the whole table and has no box of its own, so it has
   no pixel count.
5. The report lists the items worst first, each with its total, the element that deviates
   most, the kind of difference that adds most to that element's, an effect's pixel count, and
   the `match motion` command that shows it. It is written to `motion-census.txt` in the tree's
   match directory.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import math
from collections.abc import Callable, Sequence
from dataclasses import dataclass, replace
from pathlib import Path

from .. import compare
from .. import paths as beryl3d_paths
from . import pixels, snapshot, tracks
from .motion import Measured, command, measure
from .target import UNSAFE, Target, target, tree_key

# The spans `--all` measures: one for each verb, effect and mark that `data/beryl3d/parity.json`
# names a recording for, so that two runs measure the same frames and their totals compare.
CENSUS: Path = beryl3d_paths.DATA / "match" / "motion-census.json"
# The lists of `parity.json` the census covers, each mapping a name to the recording that plays
# it.
CENSUS_KINDS: tuple[str, ...] = ("verbs", "effects", "marks")
# How long after a step's motion ends its span goes on, so that a side whose motion ends later
# is still seen ending.
AFTER_MS = 100
# How long before a prompt is open and still a mark's span begins, at most: a mark appears with
# its prompt, so its span starts a frame after the prompt's view arrives unless that is longer
# ago than this.
MARK_MS = 600
# The browser pages `--all` keeps open at once. Every session shares the machine's page slots.
PAGES = 2


# A span's recording, seat, first moment and last moment.
Where = tuple[str, str, str, str]


@dataclass(frozen=True)
class Span:
    """What the census measures for one item: its kind (`verbs`, `effects` or `marks`), its
    name, and the span of its recording, from one seat, that shows it; `why` instead says why
    no span shows it."""

    kind: str
    name: str
    recording: str
    seat: str = "you"
    start: str = ""
    end: str = ""
    why: str = ""

    def where(self) -> Where:
        """The frames the span covers, which items that share them are measured once for."""
        return (self.recording, self.seat, self.start, self.end)

    def target(self) -> Target:
        """The span's first moment, with Beryl 3D at real time as `match motion` plays it."""
        return replace(
            target(
                argparse.Namespace(
                    recording=self.recording,
                    at=self.start,
                    seat=self.seat,
                    layout=None,
                    size=None,
                    scale=None,
                    only=None,
                )
            ),
            real_time=True,
        )


def lines_of(printed: str) -> list[dict[str, object]]:
    return [json.loads(line) for line in printed.splitlines() if line.startswith("{")]


def ms(value: object) -> float:
    return float(str(value))


def step_span(
    kind: str,
    name: str,
    timeline: Sequence[dict[str, object]],
    info: dict[str, object],
    arrival: Callable[[int], float | None],
) -> tuple[str, str] | None:
    """The span of the first step of `timeline` (`b3d timeline`'s lines) that plays the verb or
    begins the effect `name`, as `--from` and `--to`: from the step's start on Beryl 3D's clock
    to `AFTER_MS` after its end. `info`'s step rows give a script step's times; a step the
    client plays for a prompt itself (`own`) has no row, so its times are its offsets after
    `arrival` of its view. None if no step does."""
    moments = info["moments"]
    assert isinstance(moments, list)
    steps = {
        str(row["moment"]): row
        for row in moments
        if str(row.get("moment", "")).startswith("step:")
    }
    for line in timeline:
        effects = line.get("effects")
        assert isinstance(effects, list)
        if kind == "verbs" and line.get("verb") != name:
            continue
        if kind == "effects" and not any(
            str(e).startswith(f"{name}@") for e in effects
        ):
            continue
        row = steps.get(f"step:{line['view']}.{line['beat']}.{line['step']}")
        if line.get("own"):
            given = arrival(int(ms(line["view"])))
            if given is None:
                continue
            start, end = given + ms(line["start"]), given + ms(line["end"])
        elif row is not None and row.get("verb") == line.get("verb"):
            start, end = ms(row["from_ms"]), ms(row["to_ms"])
        else:
            continue
        # A frame after the start, so that the span begins once its view has arrived.
        return f"{math.floor(start) + 1}ms", f"{math.ceil(end) + AFTER_MS}ms"
    return None


def view_arrival(recording: str, seat: str, view: int) -> float | None:
    """When the client was given the `view`th view (counted from 1) of the recording, on its
    clock: what `tree --records` says of the frame where that view is first still."""
    t = replace(
        target(
            argparse.Namespace(
                recording=recording,
                at=f"view:{view}",
                seat=seat,
                layout=None,
                size=None,
                scale=None,
                only=None,
            )
        ),
        real_time=True,
    )
    printed = compare.b3d(
        ["tree", recording, *t.common(), "--at", f"view:{view}", "--records"], False
    )
    frames = [] if printed is None else lines_of(printed)
    # A later view given by the time this one is still would give its own time instead.
    if not frames or frames[0].get("view") != view:
        return None
    given = frames[0].get("view_given_ms")
    return float(given) if isinstance(given, int) else None


def mark_span(recording: str, seat: str) -> tuple[str, str] | str:
    """A mark's span: from a frame after the view of the recording's first prompt arrives (or
    `MARK_MS` before the prompt is still, if later) to `AFTER_MS` after it is still; why not if
    the recording has no prompt."""
    t = replace(
        target(
            argparse.Namespace(
                recording=recording,
                at="prompt:1",
                seat=seat,
                layout=None,
                size=None,
                scale=None,
                only=None,
            )
        ),
        real_time=True,
    )
    printed = compare.b3d(
        ["tree", recording, *t.common(), "--at", "prompt:1", "--records"], False
    )
    frames = [] if printed is None else lines_of(printed)
    if not frames or not isinstance(frames[0].get("view_given_ms"), int):
        return f"{recording} has no prompt with a view before it"
    still, given = ms(frames[0]["t_ms"]), ms(frames[0]["view_given_ms"])
    start = max(given + 17, still - MARK_MS)
    return f"{start:.0f}ms", f"{still + AFTER_MS:.0f}ms"


def find_spans(parity: dict[str, dict[str, str]]) -> list[Span]:
    """Each census item's span, from the recordings' timelines: the first step from your seat
    that plays the verb or begins the effect, else the first from the opponent's, and for a
    mark its recording's first prompt."""
    found: list[Span] = []
    read: dict[
        tuple[str, str], tuple[list[dict[str, object]], dict[str, object]] | None
    ] = {}

    def recording_of(
        recording: str, seat: str
    ) -> tuple[list[dict[str, object]], dict[str, object]] | None:
        if (recording, seat) not in read:
            timeline = compare.b3d(["timeline", recording, "--seat", seat], False)
            info = compare.b3d(
                ["info", recording, "--seat", seat, "--json", "--no-catch-up"], False
            )
            read[(recording, seat)] = (
                None
                if timeline is None or info is None
                else (lines_of(timeline), lines_of(info)[-1])
            )
        return read[(recording, seat)]

    for kind in CENSUS_KINDS:
        for name, recording in sorted(parity[kind].items()):
            span = Span(kind, name, recording, why=f"no step of {recording} shows it")
            for seat in ("you", "opponent"):
                if kind == "marks":
                    got = mark_span(recording, seat)
                    if isinstance(got, str):
                        span = Span(kind, name, recording, why=got)
                        continue
                    span = Span(kind, name, recording, seat, *got)
                    break
                both = recording_of(recording, seat)
                at = (
                    None
                    if both is None
                    else step_span(
                        kind,
                        name,
                        *both,
                        lambda view: view_arrival(recording, seat, view),
                    )
                )
                if at is not None:
                    span = Span(kind, name, recording, seat, *at)
                    break
            found.append(span)
    return found


def read_census() -> list[Span]:
    entries = json.loads(CENSUS.read_text())["spans"]
    return [Span(**entry) for entry in entries]


def write_census(spans: Sequence[Span]) -> None:
    about = (
        "The spans `just b3d match motion --all` measures, one for each verb, effect and mark "
        "of data/beryl3d/parity.json, so that runs compare. `--all --refresh-census` finds them "
        "again from the recordings' timelines and rewrites this file."
    )
    entries = [
        {k: v for k, v in vars(span).items() if v or k in ("kind", "name", "recording")}
        for span in spans
    ]
    CENSUS.write_text(json.dumps({"about": about, "spans": entries}, indent=1) + "\n")


def scored(found: Measured, window: tuple[int, int]) -> dict[str, object]:
    """What `--all` keeps of a measured span: each moving pair's and lonely element's
    `tracks.Deviation`, and the report's notes."""
    times = found.times
    rows: list[dict[str, object]] = [
        {
            "key": p.key,
            "effect": p.effect,
            **vars(tracks.deviation(p.web, p.b3d, times)),
        }
        for p in found.pairs
    ]
    rows += [
        {
            "key": f"{lonely.key} (Beryl 3D only)",
            "effect": lonely.effect,
            **vars(tracks.alone(lonely.track)),
        }
        for lonely in found.lonely_b3d
    ]
    rows += [
        {
            "key": f"{lonely.key} (web only)",
            "effect": lonely.effect,
            **vars(tracks.alone(lonely.track)),
        }
        for lonely in found.lonely_web
    ]
    return {"rows": rows, "notes": found.notes, "peaks": peaks(found, window)}


def peaks(found: Measured, window: tuple[int, int]) -> dict[str, dict[str, object]]:
    """For each effect measured in the span, the frame where it is strongest and its box
    there: the most opaque frame of its web elements, or of its Beryl 3D elements if the web
    draws none of it, as `{"t_ms", "box"}`. Only frames where its box is inside the window
    (`window`, width and height in points) count, since a picture shows nothing outside it.
    """
    width: int = window[0]
    height: int = window[1]

    def shown(sample: tracks.Sample) -> bool:
        return (
            sample.x >= 0
            and sample.y >= 0
            and sample.x + sample.width <= width
            and sample.y + sample.height <= height
        )

    web: dict[str, list[tracks.Track]] = {}
    b3d: dict[str, list[tracks.Track]] = {}
    for pair in found.pairs:
        if pair.effect:
            web.setdefault(pair.effect, []).append(pair.web)
            b3d.setdefault(pair.effect, []).append(pair.b3d)
    for lonely in found.lonely_web:
        if lonely.effect:
            web.setdefault(lonely.effect, []).append(lonely.track)
    for lonely in found.lonely_b3d:
        if lonely.effect:
            b3d.setdefault(lonely.effect, []).append(lonely.track)
    out: dict[str, dict[str, object]] = {}
    for name in sorted({*web, *b3d}):

        def on_window(
            side: list[tracks.Track],
        ) -> list[tuple[float, int, tracks.Sample]]:
            return [
                (sample.opacity, at, sample)
                for track in side
                for at, sample in enumerate(track)
                if sample is not None and shown(sample)
            ]

        # The web's element where it shows on the window, else Beryl 3D's.
        seen = on_window(web.get(name, [])) or on_window(b3d.get(name, []))
        if not seen:
            continue
        _, at, sample = max(seen, key=lambda found: (found[0], -found[1]))
        box = [sample.x, sample.y, sample.width, sample.height]
        out[name] = {"t_ms": found.times[at], "box": box}
    return out


def span_inputs(span: Span, art: bool) -> str:
    """A digest of what a span's measurement depends on: the snapshot's inputs (target, commit,
    uncommitted changes to either client and these tools) and the span's end."""
    t = span.target()
    return hashlib.sha256(f"{snapshot.inputs(t, art)}\0{span.end}".encode()).hexdigest()


def measured_span(
    span: Span, art: bool, fresh: bool, serving: snapshot.Shared
) -> dict[str, object]:
    """`scored` of `span`, from its cache file when its inputs have not changed since, or
    `{"failed": why}`."""
    t = span.target()
    cache = t.directory() / UNSAFE.sub("_", f"motion-to-{span.end}") / "ranked.json"
    digest = span_inputs(span, art)
    if not fresh and cache.exists():
        kept = json.loads(cache.read_text())
        if kept.get("inputs") == digest:
            result: dict[str, object] = kept["result"]
            return result
    found = measure(t, span.end, None, art, serving)
    if isinstance(found, str):
        return {"failed": found}
    result = scored(found, t.width_height())
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(json.dumps({"inputs": digest, "result": result}) + "\n")
    return result


@dataclass(frozen=True)
class Ranked:
    """One census item's place in the ranking: its span, the sum of its rows' weighed
    deviations, and its worst row's element and the kind of difference that adds most to it.
    """

    span: Span
    total: float
    element: str
    most: str
    failed: str = ""
    # For an effect, the pixels that differ inside its box where it is strongest, or why there
    # is no count.
    pixels: int | str | None = None


def rank(
    spans: Sequence[Span], results: dict[Where, dict[str, object]]
) -> list[Ranked]:
    """Each span's `Ranked` from the results of the spans' frames, worst first; spans without a
    measurement last."""
    out: list[Ranked] = []
    for span in spans:
        result = results.get(span.where(), {"failed": span.why})
        failed = result.get("failed")
        if failed is not None:
            out.append(Ranked(span, 0.0, "", "", str(failed)))
            continue
        rows = result["rows"]
        assert isinstance(rows, list)
        if span.kind == "effects":
            rows = [row for row in rows if row.get("effect") == span.name]
        deviations = [
            (
                str(row["key"]),
                tracks.Deviation(*(float(row[kind]) for kind in tracks.WEIGHTS)),
            )
            for row in rows
        ]
        if not deviations:
            nothing = (
                "neither side draws it in its span"
                if span.kind == "effects"
                else "nothing moves on either side"
            )
            out.append(Ranked(span, 0.0, nothing, ""))
            continue
        key, worst = max(deviations, key=lambda kd: kd[1].total())
        total = sum(d.total() for _, d in deviations)
        out.append(Ranked(span, total, key, worst.most()))
    return sorted(out, key=lambda r: (bool(r.failed), -r.total))


def effect_pixels(
    span: Span,
    results: dict[Where, dict[str, object]],
    art: bool,
    serving: snapshot.Shared,
) -> int | str:
    """The pixels that differ inside the effect's box at the frame where it is strongest
    (`peaks`), or why there is no count."""
    if span.name == "shake":
        return "it moves the whole table and has no box"
    found = results.get(span.where(), {}).get("peaks")
    peak = found.get(span.name) if isinstance(found, dict) else None
    if not isinstance(peak, dict):
        return "neither side draws it on the window in its span"
    at = f"{round(float(str(peak['t_ms'])))}ms"
    box = peak["box"]
    assert isinstance(box, list)
    return pixels.inside(
        replace(span.target(), at=at), [float(n) for n in box], art, serving
    )


def ranking_lines(ranked: Sequence[Ranked]) -> list[str]:
    lines: list[str] = []
    for n, r in enumerate(ranked, 1):
        what = f"{r.span.kind[:-1]} {r.span.name}"
        if r.failed:
            lines.append(f"{n:>3}. {what}: not measured: {r.failed}")
            continue
        line = f"{n:>3}. {what}  {r.total:.1f}  {r.element}  most: {r.most}"
        if isinstance(r.pixels, int):
            line += f"  pixels: {r.pixels}"
        elif isinstance(r.pixels, str):
            line += f"  pixels: - ({r.pixels})"
        lines.append(line)
        lines.append(f"     {command(r.span.target(), r.span.end)}")
    return lines


def run_all(args: argparse.Namespace) -> int:
    if args.refresh_census or not CENSUS.exists():
        parity = json.loads((beryl3d_paths.DATA / "parity.json").read_text())
        if compare.b3d(["list"]) is None:
            return 1
        spans = find_spans(parity)
        write_census(spans)
        print(f"wrote {CENSUS}")
    else:
        spans = read_census()
    # One span for each set of frames, however many items share them.
    measurable = sorted({s.where(): s for s in spans if not s.why}.items())
    results: dict[Where, dict[str, object]] = {}
    with snapshot.shared() as serving:
        if serving is None:
            return 1
        given: snapshot.Shared = serving
        with concurrent.futures.ThreadPoolExecutor(PAGES) as pool:
            jobs = {
                pool.submit(
                    measured_span, s, bool(args.art), bool(args.fresh), given
                ): s
                for where, s in measurable
            }
            for job in concurrent.futures.as_completed(jobs):
                results[jobs[job].where()] = job.result()
        ranked = [
            (
                replace(r, pixels=effect_pixels(r.span, results, bool(args.art), given))
                if r.span.kind == "effects" and not r.failed
                else r
            )
            for r in rank(spans, results)
        ]
    total = sum(r.total for r in ranked)
    report = [
        f"just b3d match motion --all: {len(ranked)} items of {CENSUS.name}, worst first",
        "each: summed deviation (points of position, with size, opacity and timing weighed "
        "as tracks.WEIGHTS says; an effect's from its own elements only), the element that "
        "deviates most, the kind of difference that adds most to it, for an effect the "
        "pixels that differ inside its box where it is strongest, and the command that "
        "shows it",
        *ranking_lines(ranked),
        f"total {total:.1f}",
    ]
    text = "\n".join(report) + "\n"
    out = beryl3d_paths.cache() / "match" / tree_key() / "motion-census.txt"
    out.write_text(text)
    print(text, end="")
    print(out)
    return 0
