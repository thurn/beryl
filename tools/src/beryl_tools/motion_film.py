"""Select and film recorded Arena animation examples."""

from __future__ import annotations

import json
import re
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import conventions, video, video_measure

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
    entries = {entry["verb"]: entry for entry in reference.get("verbs", [])}
    for instance in entries.get(verb, {}).get("instances", []):
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


def reference() -> dict[str, Any]:
    """Read the measured Arena instances used in the comparison films."""
    return json.loads(conventions.MOTION.read_text())
