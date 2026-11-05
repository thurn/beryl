"""The pixel verdict ledger in docs/ui_mockups/regions.json, and the reviewer packets.

A part of a mockup is done when a fresh reviewer calls its replica indistinguishable at a glance
(no numeric thresholds). The ledger keeps each part's last verdict where every bead sees it:

    "hand-bolt": {
      "rect": [468, 766, 194, 175],
      "verdict": "pass",
      "bead": "by-b0q.1.27",
      "commit": "44861e7",
      "fingerprint": "<base64: the 8x8 mean colors of the replica crop>",
      "accepted": ["by-b0q.1.27 residual: the mockup's slightly darker red rim"]
    }

- `verdict` is `pass` or `differs`; `bead` gave it and `commit` is the HEAD it was rendered at,
  with `+` when uncommitted edits were rendered on top.
- `fingerprint` (on a pass) sketches the replica crop that passed: the mean color of each cell of
  an 8x8 grid, masks blacked out (capture.mjs's `kit.sketch`). `ui-compare` lists the passed
  parts whose replica has changed since, so a bead sees at once when it moved a part another
  passed: re-review those. A cell's color may drift by DRIFT levels a channel: WebKit's text
  antialiasing varies by a level between some runs.
- `accepted` holds residuals and mockup conflicts, one line each with the bead that accepted
  them; the reviewer packet (`ui-compare --review`) hands them to the next reviewer so nobody
  argues them again. A contradiction between two reviewer rounds is recorded as a conflict.

`ui-verdict` (mockups.py) records verdicts and accepted lines from the last `ui-compare` run.
"""

from __future__ import annotations

import base64
import binascii
import json
import subprocess
from pathlib import Path
from typing import cast

from . import paths, shots

Json = dict[str, object]

# Where `ui-compare` leaves each run's fingerprints and the commit they were rendered at.
FINGERPRINTS = "fingerprints.json"
# Ledger fields of a part, in the order they are written after its rect and masks.
FIELDS = ("verdict", "bead", "commit", "fingerprint", "accepted")
VERDICTS = ("pass", "differs")
# The most a fingerprint cell's channel may move, in levels of 255, before the part has changed.
DRIFT = 2

RUBRIC = """\
## Rubric

"Indistinguishable at a glance" means that someone who looks at the two crops side by side at
1x, for a few seconds, cannot tell which one is the mockup. Count a difference when it shows at
that glance:

- an element's shape, size, position or rotation;
- its color, brightness, contrast, glow or shadow;
- an element missing or extra; different text, typeface, weight, size or line breaks.

Ignore:

- Black areas of diff.png: masks over the mockup's MAGIC card back artwork and mouse cursor. The
  replica draws Project Beryl's own card back there by design.
- The pictures inside card art windows: the replica's art is cut from the mockup itself. Judge
  the frame around them.
- Painted texture from the mockup's renderer: grain, mottling, marbling, cloudy fills,
  specular glints, brush noise.
- Garbled, swash or mis-painted glyphs in the mockup's text. Judge typeface, size, weight, color
  and placement, not misspelled letters.
- Differences you can find only in diff.png or by zooming in. The diff shows every pixel that
  differs; it tells you where to look, not whether a difference shows at a glance.
- The accepted lines below: they were judged before. Do not raise them again.

## How to answer

The first line is exactly `VERDICT: pass` or `VERDICT: differs`. Then:

- For differs, one line per difference, the most visible first:
  `- <element>: <what differs>, at <x,y in crop pixels>; the replica should <change>`.
- Optionally, differences you noticed but judge invisible at a glance, one line each:
  `- residual: <what>`. They are recorded as accepted so the next reviewer skips them.

Judge only this part, from these images. Do not suggest code."""


def load() -> Json:
    return cast(Json, json.loads(shots.REGIONS.read_text()))


def save(raw: Json) -> None:
    shots.REGIONS.write_text(dumps(raw))


def dumps(value: object) -> str:
    """The region file's layout: two-space indents, and a list or object on one line when it
    holds no text and fits in 100 columns (rects, masks, art windows)."""
    return _dump(value, 0, 0) + "\n"


def _textless(value: object) -> bool:
    if isinstance(value, list):
        return all(_textless(v) for v in value)
    if isinstance(value, dict):
        return all(_textless(v) for v in value.values())
    return not isinstance(value, str)


def _dump(value: object, depth: int, column: int) -> str:
    """`value` written at `column` of a line indented `depth` levels."""
    flat = json.dumps(value, ensure_ascii=False)
    if not isinstance(value, (list, dict)) or (
        _textless(value) and column + len(flat) < 100
    ):
        return flat
    pad = "  " * (depth + 1)
    if isinstance(value, list):
        items = [pad + _dump(v, depth + 1, len(pad)) for v in value]
        brackets = "[]"
    else:
        items = []
        for k, v in cast(Json, value).items():
            key = f"{pad}{json.dumps(k, ensure_ascii=False)}: "
            items.append(key + _dump(v, depth + 1, len(key)))
        brackets = "{}"
    if not items:
        return brackets
    return f"{brackets[0]}\n" + ",\n".join(items) + "\n" + "  " * depth + brackets[1]


def parts(raw: Json) -> dict[str, Json]:
    """Every part by `mockup/part`, as an object (`rect` and any masks and ledger fields)."""
    found: dict[str, Json] = {}
    for mockup, m in cast(dict[str, Json], raw["mockups"]).items():
        for part, spec in cast(Json, m["parts"]).items():
            found[f"{mockup}/{part}"] = (
                cast(Json, {"rect": spec})
                if isinstance(spec, list)
                else cast(Json, spec)
            )
    return found


def set_part(raw: Json, name: str, spec: Json) -> None:
    """Writes a part back, as the bare rect when nothing else is left."""
    mockup, _, part = name.partition("/")
    table = cast(Json, cast(Json, cast(Json, raw["mockups"])[mockup])["parts"])
    order = ["rect", "masks", *FIELDS]
    tidy = {k: spec[k] for k in order if k in spec}
    tidy.update((k, v) for k, v in spec.items() if k not in order)
    table[part] = tidy["rect"] if list(tidy) == ["rect"] else tidy


def drift(then: str, now: str) -> int:
    """How far apart two fingerprints are: the largest change of a cell's channel (255 when they
    are not comparable)."""
    try:
        a, b = base64.b64decode(then), base64.b64decode(now)
    except (binascii.Error, ValueError):
        return 255
    if len(a) != len(b) or not a:
        return 255
    return max(abs(x - y) for x, y in zip(a, b))


def changed_since_pass(
    ledger: dict[str, Json], fingerprints: dict[str, str]
) -> dict[str, int]:
    """The parts that passed whose replica, rendered now, no longer matches the passing one, with
    their drift."""
    changed: dict[str, int] = {}
    for name, now in fingerprints.items():
        spec = ledger.get(name, {})
        then = spec.get("fingerprint")
        if spec.get("verdict") == "pass" and isinstance(then, str):
            moved = drift(then, now)
            if moved > DRIFT:
                changed[name] = moved
    return changed


def report(ledger: dict[str, Json], fingerprints: dict[str, str]) -> str:
    """The ledger's state over the parts just rendered."""
    counts = {"pass": 0, "differs": 0, "unreviewed": 0}
    for name in fingerprints:
        verdict = ledger.get(name, {}).get("verdict")
        counts[str(verdict) if verdict in VERDICTS else "unreviewed"] += 1
    lines = ["ledger: " + ", ".join(f"{n} {k}" for k, n in counts.items())]
    changed = changed_since_pass(ledger, fingerprints)
    if changed:
        lines.append(f"changed since pass ({len(changed)}; re-review them):")
        lines += [
            f"  {n}: {ledger[n].get('bead')}'s pass at {ledger[n].get('commit')}"
            f" (drift {moved})"
            for n, moved in changed.items()
        ]
    return "\n".join(lines)


def review_prompt(name: str, spec: Json, part_dir: Path, zoom: int) -> str:
    """A reviewer's prompt for one part: the images, the rubric and the part's accepted lines."""
    x, y, w, h = cast(list[int], spec["rect"])
    accepted = cast(list[str], spec.get("accepted", []))

    def image(file: str) -> str:
        return f"`{part_dir / file}`"

    lines = [
        f"# Pixel review: {name}",
        "",
        "You are a fresh reviewer. Decide whether the replica of one part of a UI mockup is",
        "indistinguishable from the mockup at a glance. Read the images with your file tool.",
        "",
        "## Images",
        "",
        f"The part is the {w}x{h} px rectangle at {x},{y} of the mockup"
        + (f", shown at {zoom}x." if zoom != 1 else "."),
        "",
        f"- {image('side-by-side.png')}: the mockup left, the replica right, a magenta gap.",
        f"- {image('onion.png')}: both at 50%; doubled edges show offsets.",
        f"- {image('diff.png')}: the absolute difference; black where masked.",
        f"- {image('mockup.png')} and {image('replica.png')}: the crops alone.",
        f"- `{part_dir.parent / 'mockup-regions.png'}`: where the part sits in the whole"
        " mockup.",
        "",
        RUBRIC,
        "",
        "## Accepted (do not raise these again)",
        "",
        *([f"- {line}" for line in accepted] or ["None yet."]),
        "",
    ]
    return "\n".join(lines)


def write_packets(
    out: Path, ledger: dict[str, Json], names: list[str], zoom: int
) -> list[Path]:
    """A prompt.md beside each part's comparison images; returns their paths."""
    written = []
    for name in names:
        mockup, _, part = name.partition("/")
        path = out / mockup / part / "prompt.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(review_prompt(name, ledger[name], path.parent, zoom))
        written.append(path)
    return written


def head() -> str:
    """HEAD's short hash, with `+` when the UI or the mockups have uncommitted edits (the region
    file's ledger aside)."""

    def git(*args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=paths.REPO, capture_output=True, text=True, check=True
        ).stdout.strip()

    dirty = git(
        "status",
        "--porcelain",
        "--",
        "ui",
        "docs/ui_mockups",
        f":(exclude){shots.REGIONS.relative_to(paths.REPO)}",
    )
    return git("rev-parse", "--short", "HEAD") + ("+" if dirty else "")


def record(
    raw: Json,
    names: list[str],
    *,
    verdict: str | None,
    bead: str,
    commit: str,
    fingerprints: dict[str, str],
    residuals: list[str],
    conflicts: list[str],
) -> None:
    """Records a verdict on each part (a pass with its fingerprint from the run) and appends
    accepted lines, skipping ones already there."""
    ledger = parts(raw)
    for name in names:
        spec = dict(ledger[name])
        if verdict is not None:
            spec.update(verdict=verdict, bead=bead, commit=commit)
            spec.pop("fingerprint", None)
            if verdict == "pass":
                spec["fingerprint"] = fingerprints[name]
        accepted = list(cast(list[str], spec.get("accepted", [])))
        for kind, texts in (("residual", residuals), ("conflict", conflicts)):
            for text in texts:
                line = f"{bead} {kind}: {text}"
                if line not in accepted:
                    accepted.append(line)
        if accepted:
            spec["accepted"] = accepted
        set_part(raw, name, spec)


def last_run(out: Path) -> tuple[str, dict[str, str]]:
    """The commit and fingerprints of this checkout's last `ui-compare` run."""
    path = out / FINGERPRINTS
    if not path.exists():
        return "", {}
    run = cast(Json, json.loads(path.read_text()))
    return str(run["commit"]), cast(dict[str, str], run["parts"])
