"""How the web page is handed a recording's views so that it shows the same point of a script
as Beryl 3D at the same time, for `match snapshot`, `match motion` and `b3d compare`
(docs/beryl-3d.md, "Matching the web interface's look").

Both clients queue the views they are given and play their scripts one after another. Beryl 3D
was given each view at a time `b3d tree --records` lists (`views_given_ms`), and a view given
while an earlier script still played waits for it. The web page treats every view it was handed
before it began listening as past and shows it at once, at rest (`Feed.arrive` in
ui/src/stage/feed.ts). So a page opened with every view Beryl 3D had been given showed the end
of the latest one, while Beryl 3D was still playing an earlier script: at 1000 ms of
`verbs-zones`, the web had Snapcaster Mage on the stack while Beryl 3D still had lands tapping
for the spell before it.

The page is therefore opened with only the views whose scripts Beryl 3D had played out before
the first view still playing was given (`first_live`), and is handed the rest live with the
fixture transport's `advance` at the times Beryl 3D was given them (`schedule`). Both clocks
count from the time the first live view was given, and Beryl 3D plays every script at real time
(`Target.common`), as the web page does, so the two stay together.

The page's own timers, such as the opening's coin, which spins for 1500 ms from when the
opening first shows (`FLIP_MS` in ui/src/screens/Opening.tsx), start when the page opens. Beryl
3D started its timers when it was given the views the page opens with, which can be seconds
earlier: at 2255 ms of `opening-bottom` the page opened at 2165 ms, so the web's coin still spun
while Beryl 3D's had stopped at 1500 ms and it showed the opening hand. So before counting, the
page's clock runs on for as long as Beryl 3D had had the latest of those views (`preroll`).
Those views are past and shown at rest, so only the page's timers move meanwhile.
"""

from __future__ import annotations

import json
import threading
from collections.abc import Mapping, Sequence

from .. import compare

# Each recording and seat's script ends, read once per process (`script_ends`).
_ENDS: dict[tuple[str, str], dict[int, float] | None] = {}
_ENDS_LOCK = threading.Lock()


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
    whose first live views arrived at `start` and which ends at `end`: the events the page
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


def ends_of(info: Mapping[str, object]) -> dict[int, float]:
    """When each view's script ended on Beryl 3D's clock, by view number (from 1), from
    `b3d info --json`'s step rows (`step:<view>.<beat>.<step>` with `to_ms`). A view with no
    step row has no entry."""
    moments = info.get("moments")
    out: dict[int, float] = {}
    for row in moments if isinstance(moments, list) else []:
        moment = str(row.get("moment", ""))
        if not moment.startswith("step:") or "to_ms" not in row:
            continue
        view = int(moment[len("step:") :].split(".")[0])
        out[view] = max(out.get(view, 0.0), float(row["to_ms"]))
    return out


def script_ends(recording: str, seat: str, build: bool) -> dict[int, float] | None:
    """`ends_of` the recording from `seat`, with every script played at real time
    (`--no-catch-up`) as every match target plays it, or None if `b3d info` failed. Read once.
    """
    key = (recording, seat)
    with _ENDS_LOCK:
        if key in _ENDS:
            return _ENDS[key]
    line = ["info", recording, "--seat", seat, "--json"]
    printed = compare.b3d([*line, "--no-catch-up"], build)
    lines = [] if printed is None else printed.strip().splitlines()
    ends = ends_of(json.loads(lines[-1])) if lines else None
    with _ENDS_LOCK:
        _ENDS[key] = ends
    return ends


def first_live(given: Sequence[float], ends: Mapping[int, float], at: float) -> int:
    """How many of the views Beryl 3D was given at the times `given` the web page opens with
    as past, for a moment `at`: those whose scripts had ended by `at`, less any whose script
    still played when the view after it was given, since that view then waited for it. The
    views after them are handed to the page live. All of the views given by `at` when every
    script has ended."""
    count = sum(1 for g in given if g <= at)

    def end(index: int) -> float:
        return max(given[index], ends.get(index + 1, given[index]))

    past = 0
    while past < count and end(past) <= at:
        past += 1
    while 0 < past < count and end(past - 1) > given[past]:
        past -= 1
    return past


def preroll(given: Sequence[float], base: float) -> float:
    """How long the page's clock runs before it counts from `base`, on Beryl 3D's clock, for
    the page to stand where Beryl 3D's timers stood: since the latest of the views given at the
    times `given` that the page opens with, those given before `base`, was given. Zero when it
    opens with none."""
    opened = [g for g in given if g < base]
    return base - max(opened) if opened else 0.0


def handover(
    messages: Sequence[dict[str, object]],
    given: Sequence[float],
    ends: Mapping[int, float] | None,
    start: float,
    end: float,
    latest: float,
) -> tuple[float, int, list[list[float]], list[str]]:
    """How to hand the web page its views for a span from `start` to `end` on Beryl 3D's
    clock: the time both clocks count from, the events the page opens with, the advances
    (`schedule`) and notes. With no script ends, or when every script given by `start` had
    ended, the page opens with every view given by then and counts from `latest`, when the
    latest of them was given."""
    past = len(given) if ends is None else first_live(given, ends, start)
    live = [g for g in given if g <= start]
    base = given[past] if past < len(live) else latest
    opened, advances, notes = schedule(messages, given, base, end)
    if past < len(live):
        notes = [
            f"note: views {past + 1} to {len(live)} were still to play at {start:.0f}ms in "
            f"Beryl 3D, so the web page is handed them live from {base:.0f}ms",
            *notes,
        ]
    return base, opened, advances, notes
