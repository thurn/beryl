"""`just b3d match score`: how far Beryl 3D's look is from the web interface's on every test
target, held to a ratchet so that one agent's fix is not undone by another's (docs/beryl-3d.md,
"Matching the web interface's look").

For each reference target (reference.py: one desktop and one phone window of every screen kind,
and a few mid-game moments), one reference target named in full (`<test>#<moment>`), or each
test target whose name starts with the `prefix` given, at its end, it takes the snapshot (snapshot.py) and counts
what `boxes`, `styles`, `gaps` and `pixels` find:

    unmatched    elements the matcher left unpaired, on either side
    boxes        pairs laid out differently, and text that overlaps on one side only
    styles       properties a pair draws differently
    unsupported  web values `Style` cannot express
    pixels       differing pixels outside glyphs, in regions larger than a speck

`data/beryl3d/web-match.json` holds each target's counts. A count above its entry, or a target
with no entry, fails. A count below its entry fails too, with the command that lowers the entry
(`--update`), as `data/beryl3d/test-faults.json` does. `--raise`, with `--update`, is the only
way to raise or add an entry, and its commit body says why, as for `timeline-parity.json`.

`--refresh-web` takes each snapshot again and rewrites the committed web reference of the
reference targets (reference.py) from the live web interface.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

from . import joined, reference, snapshot
from .reference import COUNTS, Counts
from .target import Target

# How many differing targets a failing score lists before giving a count of the rest.
LISTED = 40


def score_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "prefix",
        nargs="?",
        default="",
        help="the test targets whose names start with this (default: the reference targets, one "
        "desktop and one phone window of every screen kind)",
    )
    parser.add_argument(
        "--update", action="store_true", help="lower the entries to the counts measured"
    )
    parser.add_argument(
        "--raise",
        dest="raise_",
        action="store_true",
        help="with --update, also raise or add entries; say why in the commit body",
    )
    parser.add_argument(
        "--refresh-web",
        action="store_true",
        help="rewrite the committed web reference of the reference targets from the live web",
    )
    parser.add_argument("--fresh", action="store_true", help="take the snapshots again")


def pixel_count(t: Target, directory: Path) -> int | None:
    """The pixels `b3d-render pixels` counts for `t`'s snapshot in `directory` (the regions
    larger than a speck), or None after printing why it could not."""
    from .. import cli

    line = [str(cli.linked("b3d-render")), "pixels", t.recording, *t.common()]
    line += ["--at", t.at, "--dir", str(directory)]
    done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode == cli.MISSING_PACK and cli.record() == 0:
        done = subprocess.run(line, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        print(
            f"match score: b3d-render pixels failed on {t.test_name()}:\n"
            f"{done.stdout}{done.stderr}",
            file=sys.stderr,
        )
        return None
    counted = json.loads((directory / "pixels.json").read_text())["counted"]
    return int(counted)


def measure(t: Target, directory: Path, refresh: bool = False) -> dict[str, int] | None:
    """Counts for shared elements and the full picture, or None when `b3d-render` failed.
    With `refresh`, write the complete web element records as the target's reference.
    """
    web = json.loads((directory / "web.json").read_text())["elements"]
    b3d = json.loads((directory / "b3d.json").read_text())["elements"]
    if refresh and t in reference.targets():
        reference.write(t, web)
    counts = reference.count_shared(web, b3d)
    joined.ensure(directory)
    pixels = pixel_count(t, directory)
    if pixels is None:
        return None
    return {**counts, "pixels": pixels}


def shown(values: Mapping[str, int]) -> str:
    return " ".join(f"{n} {values[n]}" for n in COUNTS if n in values)


def judge(
    measured: Mapping[str, Mapping[str, int]], document: Mapping[str, Any]
) -> tuple[list[str], list[str], list[str]]:
    """The targets that rose above their entries or have none (`over`), those that fell below
    them (`under`), and the lines naming each: `(over, under, lines)`."""
    over: list[str] = []
    under: list[str] = []
    lines: list[str] = []
    for name in sorted(measured):
        found = Counts(measured[name])
        held = reference.entry(document, name)
        if held is None:
            over.append(name)
            lines.append(f"{name}: not listed ({shown(found.values)})")
            continue
        rose = found.rose(held)
        fell = found.fell(held)
        if rose:
            over.append(name)
            lines.append(f"{name}: {', '.join(rose)}")
        elif fell:
            under.append(name)
            lines.append(f"{name}: {', '.join(fell)}")
    return over, under, lines


def updated(
    measured: Mapping[str, Mapping[str, int]],
    document: Mapping[str, Any],
    raise_: bool,
) -> tuple[dict[str, dict[str, int]], list[str]]:
    """The ratchet's targets after `--update`: each measured target's entry set to what was
    measured, those not measured kept. Returns them with the targets it left alone because an
    entry would have risen or been added, which `raise_` allows."""
    entries: dict[str, dict[str, int]] = dict(document["targets"])
    refused: list[str] = []
    for name, values in measured.items():
        held = reference.entry(document, name)
        rising = held is None or bool(Counts(values).rose(held))
        if rising and not raise_:
            refused.append(name)
            continue
        entries[name] = {n: int(values.get(n, 0)) for n in COUNTS}
    return entries, refused


def command_line(args: argparse.Namespace) -> str:
    parts = ["just b3d match score"]
    if args.prefix:
        parts.append(args.prefix)
    for flag, on in (
        ("--update", args.update),
        ("--raise", args.raise_),
        ("--refresh-web", args.refresh_web),
    ):
        if on:
            parts.append(flag)
    return " ".join(parts)


def listed(lines: Sequence[str]) -> list[str]:
    out = list(lines[:LISTED])
    if len(lines) > LISTED:
        out.append(f"... {len(lines) - LISTED} more targets")
    return out


def run(args: argparse.Namespace) -> int:
    if args.raise_ and not args.update:
        print("match score: --raise needs --update", file=sys.stderr)
        return 2
    prefix = args.prefix
    if prefix and not prefix.startswith("b3d:"):
        prefix = f"b3d:{prefix}"
    # A reference target at a moment of its own is named in full, `<test>#<moment>`.
    exact = [t for t in reference.targets() if reference.entry_name(t) == prefix]
    if exact:
        targets = exact
    elif prefix:
        targets = snapshot.test_targets(prefix, reference.AT)
        if targets is None:
            return 1
    else:
        targets = reference.targets()
    if not targets:
        print(
            f"match score: no test target starts with {args.prefix!r}", file=sys.stderr
        )
        return 1
    print(command_line(args))

    measured: dict[str, dict[str, int]] = {}
    failed: list[str] = []
    results = snapshot.measured(
        targets,
        args.fresh or args.refresh_web,
        False,
        lambda t, d: measure(t, d, args.refresh_web),
    )
    for t, counts in results:
        if counts is None:
            failed.append(reference.entry_name(t))
        else:
            measured[reference.entry_name(t)] = counts
    if failed:
        print(
            f"match score: no snapshot or pixels for {', '.join(failed)}",
            file=sys.stderr,
        )
        return 1
    if args.refresh_web:
        print(f"wrote the web reference under {reference.REFERENCE}")

    document = reference.load_ratchet()
    if args.update:
        entries, refused = updated(measured, document, args.raise_)
        write = {**document, "targets": entries}
        reference.write_ratchet(write)
        print(f"wrote {reference.RATCHET.name}: {len(entries)} targets listed")
        if refused:
            print(
                f"left alone, since they would rise or are new (use --raise): "
                f"{', '.join(sorted(refused))}"
            )
        return 1 if refused else 0

    over, under, lines = judge(measured, document)
    for line in listed(lines):
        print(line)
    if over:
        print(
            f"{len(over)} of {len(measured)} targets are above their entries or not listed; "
            f"`just b3d match boxes|styles|pixels <target> --at {reference.AT}` finds why"
        )
        return 1
    if under:
        print(
            f"{len(under)} of {len(measured)} targets improved: lower their entries with "
            f"`{command_line(args)} --update`"
        )
        return 1
    print(f"{len(measured)} targets hold their entries")
    return 0
