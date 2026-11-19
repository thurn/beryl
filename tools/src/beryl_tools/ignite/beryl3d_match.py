"""The checks that hold Beryl 3D's look against the web interface's: the `b3d-web-match` and
`b3d-web-drift` families of `ignite/checks/beryl3d.toml`, run through the adapter
`beryl3d.py` (docs/beryl-3d.md, "Matching the web interface's look").

- **`b3d-web-match`** is a regression family with one check for each reference target (one
  desktop and one phone window of every screen kind). It plays the target's recording in Beryl 3D
  with the programs the `b3d` product built, pairs the elements with the web interface's
  committed records (`data/beryl3d/match/web/<target>.json`) and fails when the target's
  unmatched, boxes, styles or unsupported count rises above its entry in
  `data/beryl3d/web-match.json`. It reads no file under `ui/` and needs no browser, so a change
  to the web interface runs none of Beryl 3D's checks, and its result depends only on its
  declared inputs. The pixel count is left out, since it needs the web's picture.
- **`b3d-web-drift`** is an exploration family with one check for each reference target. It shoots
  the live web interface, compares it with the committed reference, and runs the full score,
  pixels included. It fails when the web interface no longer matches the reference or the score
  rose above its entry, and names the target, its worst elements and the command that
  reproduces it. Its `next` command yields every target with a digest of the web interface's
  sources, so a target is played again only after the sources changed.
"""

from __future__ import annotations

import concurrent.futures
import hashlib
import json
import os
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

from .. import paths
from ..beryl3d import paths as b3d_paths
from ..beryl3d.match import boxes, reference, score, snapshot, styles
from ..beryl3d.match.reference import RECORD_COUNTS, Counts
from ..beryl3d.match.target import Target
from . import Listed, Result
from .rust import asked

WEB_MATCH = "b3d-web-match"
WEB_DRIFT = "b3d-web-drift"
DEBUG: Path = b3d_paths.WORKSPACE / "target" / "debug"
# What the web interface's page is made of, which the digest of `next` covers.
WEB_SOURCES = ("ui/src", "ui/fixtures.json", "ui/package.json")
# How many worst elements a failure's message lists for each kind of difference.
WORST = 3
# The longest a failure's message grows, in characters.
MESSAGE_LIMIT = 1500


def relative(file: Path) -> str:
    return file.relative_to(paths.REPO).as_posix()


def reproduce(t: Target) -> str:
    """The command that scores `t` again from the live web interface."""
    return f"just b3d match score {t.test_name()} --fresh"


def clip(text: str) -> str:
    return text if len(text) <= MESSAGE_LIMIT else text[: MESSAGE_LIMIT - 1] + "…"


# b3d-web-match.


def list_web_match(cards: Mapping[str, Sequence[str]]) -> list[Listed]:
    """One check for each reference target. Each reads its reference file and the card files its
    recording plays (`cards`, by recording)."""
    return [
        Listed(
            f"{WEB_MATCH}:{reference.check_name(t)}",
            [relative(reference.path(t)), *cards.get(t.recording, [])],
        )
        for t in reference.targets()
    ]


def b3d_records(t: Target) -> list[dict[str, Any]]:
    """Beryl 3D's element records of `t`: `b3d tree --records`, as the snapshot takes them."""
    line = [str(DEBUG / "b3d"), "tree", t.recording, *t.common()]
    line += ["--at", t.at, "--records"]
    done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise RuntimeError(f"{' '.join(line)} failed:\n{done.stdout}{done.stderr}")
    elements: list[dict[str, Any]] = json.loads(done.stdout.strip().splitlines()[-1])[
        "elements"
    ]
    return elements


def match_one(name: str, document: Mapping[str, Any]) -> Result:
    check = f"{WEB_MATCH}:{name}"
    t = reference.by_check_name(name)
    if t is None:
        return Result(check, "fail", f"{name} is not a reference target")
    web = reference.load(t)
    if web is None:
        return Result(
            check,
            "fail",
            f"{relative(reference.path(t))} is missing; `just b3d match score "
            "--refresh-web` writes it",
        )
    held = reference.entry(document, t.test_name())
    if held is None:
        return Result(
            check,
            "fail",
            f"{t.test_name()} has no entry in {reference.RATCHET.name}; "
            "`just b3d match score --update --raise` adds it",
        )
    found = Counts(reference.count(web, b3d_records(t)))
    rose = found.rose(held, RECORD_COUNTS)
    if not rose:
        return Result(check, "pass")
    return Result(
        check,
        "fail",
        clip(
            f"{t.test_name()} is further from the web interface than "
            f"{reference.RATCHET.name} allows: {', '.join(rose)}. "
            f"`just b3d match boxes {t.test_name()} --at {t.at}` (or `styles`) lists the "
            f"differences; `just b3d match score {t.test_name()}` measures them."
        ),
    )


def run_web_match(checks: Sequence[str]) -> list[Result]:
    wanted = asked(WEB_MATCH, checks)
    if not wanted:
        return []
    document = reference.load_ratchet()
    workers = max(1, int(os.environ.get("IGNITE_CORES") or os.cpu_count() or 1))
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        return list(pool.map(lambda name: match_one(name, document), wanted))


# b3d-web-drift.


def web_digest() -> str:
    """A digest of the web interface's sources (`WEB_SOURCES`)."""
    digest = hashlib.sha256()
    for entry in WEB_SOURCES:
        root = paths.REPO / entry
        files = (
            [root]
            if root.is_file()
            else sorted(p for p in root.rglob("*") if p.is_file())
        )
        for file in files:
            digest.update(relative(file).encode())
            digest.update(b"\0")
            digest.update(file.read_bytes())
    return digest.hexdigest()


def next_web_drift() -> list[dict[str, str]]:
    """Every reference target as a check, keyed by the web sources and the target's reference
    and entry, so that it is played again when any of them changed."""
    web = web_digest()
    document = reference.load_ratchet()
    out: list[dict[str, str]] = []
    for t in reference.targets():
        file = reference.path(t)
        held = reference.entry(document, t.test_name())
        digest = hashlib.sha256()
        for part in (
            web,
            file.read_text() if file.is_file() else "",
            json.dumps(held.values if held else None, sort_keys=True),
        ):
            digest.update(part.encode())
            digest.update(b"\0")
        out.append(
            {
                "check": f"{WEB_DRIFT}:{reference.check_name(t)}",
                "digest": digest.hexdigest(),
            }
        )
    return out


def worst_lines(
    web: Sequence[Mapping[str, Any]], live: Sequence[Mapping[str, Any]]
) -> list[str]:
    """The worst elements by which `live`, the web interface now, differs from `web`, the
    committed reference: unpaired elements, then boxes, then styles."""
    from ..beryl3d.match import joined

    found = joined.join(web, live)
    pairs, unmatched = found["pairs"], found["unmatched"]
    assert isinstance(pairs, list) and isinstance(unmatched, dict)
    lines: list[str] = []
    for side in ("web", "b3d"):
        for e in unmatched[side][:WORST]:
            where = "gone from the web" if side == "web" else "new on the web"
            lines.append(f'{e["role"]} "{e["name"] or ""}" is {where}')
    lines += [d.line() for d in boxes.differences(web, live, pairs)[:WORST]]
    lines += [d.line() for d in styles.measure(web, live, pairs).differences[:WORST]]
    return lines


def pixel_lines(t: Target, directory: Path) -> list[str]:
    """The worst elements by pixels of `t`'s snapshot (`pixels.json`'s rows), as `locator: n
    pixels`."""
    b3d = json.loads((directory / "b3d.json").read_text())["elements"]
    web = json.loads((directory / "web.json").read_text())["elements"]
    rows = json.loads((directory / "pixels.json").read_text())["rows"]
    out: list[str] = []
    for row in rows[:WORST]:
        side = (
            b3d[row["b3d"]]
            if row.get("b3d") is not None
            else web[row["web"]] if row.get("web") is not None else None
        )
        if side is not None:
            out.append(f"{boxes.describe(side)}: {row['pixels']} pixels")
    return out


def drift_one(t: Target, directory: Path, document: Mapping[str, Any]) -> Result:
    check = f"{WEB_DRIFT}:{reference.check_name(t)}"
    live = json.loads((directory / "web.json").read_text())["elements"]
    web = reference.load(t)
    problems: list[str] = []
    worst: list[str] = []
    if web is None:
        problems.append(f"{relative(reference.path(t))} is missing")
    else:
        drift = reference.count(web, reference.trim(live))
        moved = {n: drift[n] for n in ("unmatched", "boxes", "styles") if drift[n]}
        if moved:
            shown = ", ".join(f"{n} {v}" for n, v in moved.items())
            problems.append(
                f"the web interface differs from its committed reference: {shown}"
            )
            worst += worst_lines(web, reference.trim(live))
    counts = score.measure(t, directory)
    held = reference.entry(document, t.test_name())
    if counts is None:
        problems.append("the pixel count could not be taken")
    elif held is None:
        problems.append(f"{t.test_name()} has no entry in {reference.RATCHET.name}")
    else:
        rose = Counts(counts).rose(held)
        if rose:
            problems.append(f"the score rose above its entry: {', '.join(rose)}")
            worst += pixel_lines(t, directory)
    if not problems:
        return Result(check, "pass")
    lines = [f"{t.test_name()}: {'; '.join(problems)}."]
    if worst:
        lines.append(
            "Worst elements (where a line says `web` it means the committed reference, "
            "and `b3d` the web interface now):"
        )
    lines += [f"  {w}" for w in worst]
    lines.append(
        f"Reproduce: `{reproduce(t)}`; `just b3d match score --refresh-web` rewrites the "
        "reference if the web change was meant."
    )
    return Result(check, "fail", clip("\n".join(lines)))


def run_web_drift(checks: Sequence[str]) -> list[Result]:
    wanted = asked(WEB_DRIFT, checks)
    if not wanted:
        return []
    document = reference.load_ratchet()
    targets: list[Target] = []
    results: list[Result] = []
    for name in wanted:
        t = reference.by_check_name(name)
        if t is None:
            results.append(
                Result(
                    f"{WEB_DRIFT}:{name}", "fail", f"{name} is not a reference target"
                )
            )
        else:
            targets.append(t)
    if targets:
        for t, result in snapshot.measured(
            targets, True, False, lambda t, d: drift_one(t, d, document)
        ):
            results.append(
                result
                or Result(
                    f"{WEB_DRIFT}:{reference.check_name(t)}",
                    "fail",
                    f"{t.test_name()}: the snapshot could not be taken. Reproduce: "
                    f"`{reproduce(t)}`",
                )
            )
    return results
