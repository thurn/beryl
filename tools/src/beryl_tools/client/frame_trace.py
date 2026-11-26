"""Summarize live frame timing over periods when the display link was running."""

from collections import Counter, defaultdict
from math import ceil
from typing import Any

Row = dict[str, Any]

CPU_STAGES = {
    "session_input_ns": "session input",
    "before_frame_ns": "feed and pre-frame work",
    "asset_delivery_ns": "asset delivery (before frame)",
    "prepare_ns": "animation and layout preparation",
    "client_ns": "Client::frame",
    "drawable_wait_ns": "drawable acquisition",
    "encode_submit_ns": "render encoding and submission",
    "present_call_ns": "CPU presentation call and hook",
    "after_draw_ns": "post-draw work and assets",
    "feed_after_ns": "feed after frame",
    "requests_ns": "platform requests",
}


def ms(ns: int) -> float:
    return ns / 1e6


def percentiles(samples: list[int]) -> str:
    if not samples:
        return "unavailable"
    ordered = sorted(samples)
    values = [ordered[ceil(len(ordered) * p) - 1] for p in (0.5, 0.95, 0.99, 1.0)]
    return (
        " / ".join(f"{ms(value):.2f}" for value in values) + f" ms (n={len(ordered)})"
    )


def same_active_period(previous: Row, current: Row) -> bool:
    return (
        previous["epoch"] == current["epoch"]
        and previous["outcome"] not in ("idle", "occluded")
        and current["outcome"] not in ("idle", "occluded")
    )


def rates(frames: list[Row], pairs: list[tuple[Row, Row]]) -> list[str]:
    lines = []
    counts: dict[float, list[tuple[Row, Row]]] = defaultdict(list)
    for previous, current in pairs:
        if previous["requested_hz"] == current["requested_hz"]:
            counts[current["requested_hz"]].append((previous, current))
    for target in sorted({frame["requested_hz"] for frame in frames}, reverse=True):
        rows = counts[target]
        seconds = sum(b["callback_ns"] - a["callback_ns"] for a, b in rows) / 1e9
        observed = [
            b["reported_hz"] for _, b in rows if b.get("reported_hz") is not None
        ]
        reported = (
            f"{min(observed):g}–{max(observed):g} Hz"
            if observed and max(observed) - min(observed) >= 0.1
            else f"{observed[0]:.1f} Hz" if observed else "unavailable"
        )
        if seconds > 0:
            submits = sum(b.get("submitted_ns") is not None for _, b in rows)
            throughput = f"callbacks {len(rows) / seconds:.1f} Hz, submissions {submits / seconds:.1f} Hz"
        else:
            throughput = "insufficient active intervals for throughput"
        lines.append(
            f"  target {target:g} Hz, display link reported {reported}, {throughput}"
        )
    return lines


def gap_line(before: Row, after: Row, gap: int, *, submission: bool) -> str:
    cpu = before.get("cpu") or {}
    cost = [
        f"{label} {ms(cpu[key]):.2f}"
        for key, label in (
            ("asset_delivery_ns", "assets"),
            ("prepare_ns", "prepare"),
            ("client_ns", "client"),
            ("drawable_wait_ns", "drawable"),
            ("encode_submit_ns", "encode/submit"),
            ("present_call_ns", "present call"),
            ("after_draw_ns", "after draw"),
        )
        if cpu.get(key) is not None
    ]
    next_session = (after.get("cpu") or {}).get("session_input_ns", 0)
    inbox = f", next session input {ms(next_session):.2f} ms" if next_session else ""
    intervening = after["sequence"] - before["sequence"] - 1
    leftover = (
        f", {intervening} intervening callbacks"
        if submission and intervening > 0
        else ""
    )
    return (
        f"    #{before['sequence']} → #{after['sequence']}: {ms(gap):.2f} ms{leftover},"
        f" next callback at {ms(after['callback_ns']):.2f} ms,"
        f" queued {ms(after['handled_ns'] - after['callback_ns']):.2f} ms,"
        f" previous handler {ms(before['finished_ns'] - before['handled_ns']):.2f} ms{inbox}"
        + (f" ({', '.join(cost)} ms)" if cost else "")
        + f", {before['tier']}, {before['render_size'][0]}×{before['render_size'][1]},"
        f" {'fullscreen' if before['fullscreen'] else 'windowed'},"
        f" AI searching {before['searching']}, animating {before['animating']}"
    )


def describe(window: Row) -> list[str]:
    events = window["events"]
    frames = sorted(
        (e["frame"] for e in events if "frame" in e), key=lambda f: f["sequence"]
    )
    states = [e["pacer"] for e in events if "pacer" in e]
    gpu = [e["gpu"] for e in events if "gpu" in e]
    pairs = [(a, b) for a, b in zip(frames, frames[1:]) if same_active_period(a, b)]
    callback_gaps = [(b["callback_ns"] - a["callback_ns"], a, b) for a, b in pairs]
    submission_gaps: list[tuple[int, Row, Row]] = []
    previous_submission: Row | None = None
    for frame in frames:
        if previous_submission is not None and not same_active_period(
            previous_submission, frame
        ):
            previous_submission = None
        if (submitted := frame.get("submitted_ns")) is not None:
            if previous_submission is not None:
                submission_gaps.append(
                    (
                        submitted - previous_submission["submitted_ns"],
                        previous_submission,
                        frame,
                    )
                )
            previous_submission = frame

    outcomes = Counter(frame["outcome"] for frame in frames)
    skipped = ", ".join(
        f"{key} {count}"
        for key, count in sorted(outcomes.items())
        if key != "submitted"
    )
    lines = [
        f"{window['reason'].replace('_', ' ')} timing window ending at {ms(window['end_ns']):.2f} ms",
        f"  {len(frames)} callbacks, {outcomes['submitted']} submissions,"
        f" skipped {skipped or 'none'},"
        f" {sum(not state['running'] for state in states)} pauses and"
        f" {sum(state['running'] for state in states)} resumes",
        *rates(frames, pairs),
        "  timing (p50 / p95 / p99 / max)",
    ]
    stages: dict[str, list[int]] = {
        "callback gap while active": [gap for gap, _, _ in callback_gaps],
        "submission gap while active": [gap for gap, _, _ in submission_gaps],
        "callback to event-loop handler": [
            f["handled_ns"] - f["callback_ns"] for f in frames
        ],
        "callback minus display-link target": [
            f["callback_ns"] - target
            for f in frames
            if (target := f.get("target_ns")) is not None
        ],
        "whole tick handler": [
            f["finished_ns"] - f["handled_ns"] for f in frames if f.get("cpu")
        ],
    }
    for key, label in CPU_STAGES.items():
        stages[label] = [
            value
            for f in frames
            if (value := (f.get("cpu") or {}).get(key)) is not None
        ]
    stages.update(
        {
            "scene submit to observed GPU completion": [
                g["observed_ns"] - g["submit_ns"] for g in gpu
            ],
            "main render pass GPU time": [
                g["main_pass_gpu_ns"]
                for g in gpu
                if g.get("main_pass_gpu_ns") is not None
            ],
        }
    )
    lines.extend(
        f"    {label}: {percentiles(values)}" for label, values in stages.items()
    )
    attempted = sum(
        bool(g.get("main_pass_sampled", g.get("main_pass_gpu_ns") is not None))
        for g in gpu
    )
    if attempted:
        lines.append(
            f"  main pass timestamps requested for {attempted} of {len(gpu)} completed submissions"
        )
    pending = [f["gpu_in_flight"] for f in frames if f.get("gpu_in_flight") is not None]
    lines.append(
        f"  most outstanding scene buffers before submit: {max(pending) if pending else 'unavailable'}"
    )
    lines.append(
        f"  actual presentation timestamps: {sum(f.get('presented_ns') is not None for f in frames)} available"
    )
    for kind, gaps in (("callback", callback_gaps), ("submission", submission_gaps)):
        if gaps:
            lines.append(f"  longest {kind} gaps while active")
            for gap, before, after in sorted(gaps, key=lambda row: -row[0])[:3]:
                lines.append(
                    gap_line(before, after, gap, submission=kind == "submission")
                )
    return lines


def describe_change(change: Row) -> str:
    line = (
        f"  at {ms(change['at_ns']):.2f} ms: {change['previous_hz']:g} → {change['new_hz']:g} Hz,"
        f" {change['previous_tier']} → {change['new_tier']}"
    )
    if (decision := change.get("adaptive")) is None:
        return line + ", graphics setting"
    line += (
        f", gap {ms(decision['gap_ns']):.2f} ms, work {ms(decision['work_ns']):.2f} ms"
    )
    if "previous_ns" in decision:
        line += f", frame before {ms(decision['previous_ns']):.2f} ms"
    line += (
        f", {decision['missed']}/{decision['window']} misses"
        f" ({decision['misses_needed']} needed)"
    )
    if "run" in decision:
        # The rule since misses had to be explained: a run toward a step up that may hold
        # `up_misses` frames that were late or lacked room.
        return line + (
            f", run {decision['run']}/{decision['run_needed']} frames"
            f" with {decision['run_bad']} late or without room"
            f" ({decision['up_misses']} allowed)"
        )
    # Logs written by the earlier rule, which stepped up after a run of frames all with room.
    return line + f", {decision['roomy']}/{decision['roomy_needed']} frames with room"


# The causes of a late frame, in the order `cause` tries them (the frame log's `late` line).
CAUSES = (
    "drawable",
    "handler",
    "queued",
    "callback late",
    "link skipped",
    "handling only",
    "unknown",
)


def callback_offset(frame: Row) -> int | None:
    """How long after its own display timestamp a tick's callback ran. The two are on
    different clocks, so only differences between offsets mean anything."""
    display = frame.get("display_ns")
    return None if display is None else frame["callback_ns"] - display


def median_offset(frames: list[Row]) -> int | None:
    """The median `callback_offset` of `frames`, or None when none has one."""
    offsets = sorted(o for f in frames if (o := callback_offset(f)) is not None)
    return offsets[len(offsets) // 2] if offsets else None


def largest_stage(frame: Row) -> str | None:
    """The CPU stage of `frame` that took longest, with its time."""
    cpu = frame.get("cpu") or {}
    stages = [(cpu[key], label) for key, label in CPU_STAGES.items() if cpu.get(key)]
    if not stages:
        return None
    took, label = max(stages)
    return f"{label} {ms(took):.2f} ms"


def cause(late: Row, *, median_offset_ns: int | None, refresh_ns: float) -> str:
    """Why a late frame came late, as the first of `CAUSES` its timing records show.
    `median_offset_ns` is the run's median `callback_offset`, and `refresh_ns` a refresh of
    the display at its highest rate."""
    frame = late.get("frame")
    if frame is None:
        return "unknown"
    interval = 1e9 / max(late.get("hz") or frame["requested_hz"], 1.0)
    wait = (frame.get("cpu") or {}).get("drawable_wait_ns")
    if wait is None:
        wait = late["waited_ms"] * 1e6
    if wait > interval / 2:
        return "drawable"
    previous = late.get("previous")
    if (
        previous is not None
        and previous["finished_ns"] - previous["callback_ns"] > 0.8 * interval
    ):
        return "handler"
    if frame["handled_ns"] - frame["callback_ns"] > interval / 2:
        return "queued"
    offset = callback_offset(frame)
    if (
        offset is not None
        and median_offset_ns is not None
        and offset - median_offset_ns > refresh_ns / 2
    ):
        return "callback late"
    if (
        previous is None
        or frame.get("display_ns") is None
        or previous.get("display_ns") is None
    ):
        return "unknown"
    if frame["display_ns"] - previous["display_ns"] >= 1.5 * interval:
        return "link skipped"
    return "handling only"


def cause_table(rows: list[tuple[str, float]]) -> list[str]:
    """A table of late frames by cause (rows, in `CAUSES` order) and the rate asked for
    (columns, highest first), from each frame's cause and rate."""
    counts = Counter(rows)
    hz = sorted({rate for _, rate in rows}, reverse=True)
    heads = [f"{rate:g} Hz" for rate in hz] + ["all"]
    width = max(len(name) for name in CAUSES)
    lines = ["  " + "cause".ljust(width) + "".join(f"{h:>9}" for h in heads)]
    for name in CAUSES:
        cells = [counts[(name, rate)] for rate in hz]
        lines.append(
            "  " + name.ljust(width) + "".join(f"{n:>9}" for n in [*cells, sum(cells)])
        )
    return lines
