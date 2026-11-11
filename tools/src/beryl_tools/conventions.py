"""The design language as data (docs/project-50k.md, K28): `data/ui-conventions.json` holds
every ruling on where Beryl's UI follows or leaves MTG Arena, and `docs/ui-conventions.md` is
generated from it. `by conventions` writes the page; the repo lane fails when it is stale.

Each entry has a kind: a numbered Beryl `exception`, a rule `settled` as Arena, one with
`no-equivalent` in Arena, one `not-seen` in the recordings, or one still `open`. It states the
rule, the report beads behind it, and what Arena does with the recording moments that show it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

import mdformat

from . import paths

DATA: Path = paths.REPO / "data" / "ui-conventions.json"
PAGE: Path = paths.REPO / "docs" / "ui-conventions.md"
KINDS = ("exception", "settled", "no-equivalent", "not-seen", "open")
# What .mdformat.toml sets for the repo lane's mdformat, so the page passes it as written.
MDFORMAT_OPTIONS: dict[str, Any] = {"wrap": 80, "number": True}
REPORT: re.Pattern[str] = re.compile(r"^by-[0-9a-z]+(\.[0-9]+)*$")
# A YouTube id, as the recordings in ~/arena are named, and a time in the recording.
RECORDING: re.Pattern[str] = re.compile(r"^[A-Za-z0-9_-]{11}$")
TIME: re.Pattern[str] = re.compile(r"^(\d+:)?\d{1,2}:\d{2}(\.\d+)?$")


def problems(data: dict[str, Any]) -> list[str]:
    """Why the data cannot be rendered, one line each; none when it can."""
    found: list[str] = []
    sections = [s.get("kind") for s in data.get("sections", [])]
    if sorted(sections) != sorted(KINDS):
        found.append(f"sections: want one per kind {list(KINDS)}, have {sections}")
    numbers: list[int] = []
    for i, entry in enumerate(data.get("entries", [])):
        where = f"entry {i} ({entry.get('name', '?')})"
        kind = entry.get("kind")
        if kind not in KINDS:
            found.append(f"{where}: kind {kind!r} is not one of {list(KINDS)}")
        if not entry.get("name") or not entry.get("rule"):
            found.append(f"{where}: needs a name and a rule")
        if (kind == "exception") != ("number" in entry):
            found.append(f"{where}: exceptions, and only they, have a number")
        elif kind == "exception":
            numbers.append(entry["number"])
        for report in entry.get("reports", []):
            if not REPORT.match(report):
                found.append(f"{where}: {report!r} is not a bead id")
        for moment in entry.get("arena", {}).get("evidence", []):
            if not RECORDING.match(moment.get("recording", "")):
                found.append(f"{where}: {moment.get('recording')!r} is not a recording")
            if not TIME.match(moment.get("time", "")):
                found.append(f"{where}: {moment.get('time')!r} is not a time")
    if numbers != list(range(1, len(numbers) + 1)):
        found.append(
            f"exceptions: numbered {numbers}, want 1 to {len(numbers)} in order"
        )
    return found


def cite(text: str, refs: list[str]) -> str:
    """`text` with `refs` in parentheses before its final full stop."""
    if not refs:
        return text
    body = text[:-1] if text.endswith(".") else text
    return f"{body} ({', '.join(refs)})."


def item(entry: dict[str, Any]) -> str:
    """One entry as the text of its list item."""
    text = f"**{entry['name']}.** {cite(entry['rule'], entry.get('reports', []))}"
    arena = entry.get("arena")
    if arena:
        moments = [f"{m['recording']} {m['time']}" for m in arena.get("evidence", [])]
        text += " " + cite(arena["does"], moments)
    return text


def render(data: dict[str, Any]) -> str:
    """The page, formatted as the repo lane's mdformat would leave it."""
    out = [f"# {data['title']}", *data["intro"]]
    for section in data["sections"]:
        entries = [e for e in data["entries"] if e["kind"] == section["kind"]]
        if not entries:
            continue
        out += [f"## {section['heading']}", *section.get("intro", [])]
        if section["kind"] == "exception":
            entries.sort(key=lambda e: e["number"])
            out.append("\n".join(f"{e['number']}. {item(e)}" for e in entries))
        else:
            out.append("\n".join(f"- {item(e)}" for e in entries))
    return mdformat.text(
        "\n\n".join(out) + "\n", options=MDFORMAT_OPTIONS, extensions={"gfm"}
    )


def load(path: Path = DATA) -> dict[str, Any]:
    data = json.loads(path.read_text())
    found = problems(data)
    if found:
        raise ValueError("\n".join(f"{path.name}: {p}" for p in found))
    return data


def stale(data_path: Path = DATA, page: Path = PAGE) -> bool:
    """Whether the page differs from what its data renders."""
    have = page.read_text() if page.exists() else ""
    return have != render(load(data_path))


def step() -> int:
    """The repo lane's check: the page matches its data."""
    try:
        if not stale():
            return 0
    except ValueError as e:
        print(e)
        return 1
    page, data = PAGE.relative_to(paths.REPO), DATA.relative_to(paths.REPO)
    print(f"{page} is stale: edit {data}, run `just conventions` and commit both")
    return 1


def run(args: argparse.Namespace) -> int:
    if args.check:
        return step()
    try:
        PAGE.write_text(render(load()))
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1
    print(f"wrote {PAGE.relative_to(paths.REPO)}")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "conventions", help="write docs/ui-conventions.md from its data (--check)"
    )
    p.add_argument("--check", action="store_true", help="fail when the page is stale")
    p.set_defaults(func=run)
