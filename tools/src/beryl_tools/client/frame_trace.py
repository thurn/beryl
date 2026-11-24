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
    return (
        line
        + f", gap {ms(decision['gap_ns']):.2f} ms, work {ms(decision['work_ns']):.2f} ms,"
        f" {decision['missed']}/{decision['window']} misses ({decision['misses_needed']} needed),"
        f" {decision['roomy']}/{decision['roomy_needed']} frames with room"
    )
