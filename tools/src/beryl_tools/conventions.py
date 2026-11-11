"""The design language as data (docs/project-50k.md, K28): `data/ui-conventions.json` holds
every ruling on where Beryl's UI follows or leaves MTG Arena, and `docs/ui-conventions.md` is
generated from it. `by conventions` writes the page; the repo lane fails when it is stale.

Each entry has a kind: a numbered Beryl `exception`, a rule `settled` as Arena, one with
`no-equivalent` in Arena, one `not-seen` in the recordings, or one still `open`. It states the
rule, the report beads behind it, and what Arena does with the recording moments that show it.

The closed lists follow: the `verbs` a script's steps apply, each with what it shows, what Arena
does and the values the stage plays it with, and the `marks` a card can carry, each with what it
means and its one look. Their names are the session's (`beryl_session`'s `script::Verb` and
`script::Mark`), and a test there fails when the two disagree.
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
# The closed lists, and what each of their members must say besides its name.
LISTS: dict[str, tuple[str, ...]] = {"verbs": ("shows",), "marks": ("means", "look")}
NAME: re.Pattern[str] = re.compile(r"^[a-z]+(-[a-z]+)*$")
# What the stage plays a verb with: none until it is measured from Arena (K27).
VALUES = ("duration", "delay", "stagger", "curve")


def evidence_problems(where: str, entry: dict[str, Any]) -> list[str]:
    """Why an entry's Arena evidence does not name moments in recordings."""
    found: list[str] = []
    for moment in entry.get("arena", {}).get("evidence", []):
        if not RECORDING.match(moment.get("recording", "")):
            found.append(f"{where}: {moment.get('recording')!r} is not a recording")
        if not TIME.match(moment.get("time", "")):
            found.append(f"{where}: {moment.get('time')!r} is not a time")
    return found


def list_problems(data: dict[str, Any]) -> list[str]:
    """Why the closed lists cannot be rendered."""
    found: list[str] = []
    headed = [section.get("key") for section in data.get("lists", [])]
    if headed != [key for key in LISTS if key in data]:
        found.append(f"lists: want a heading for each of {list(LISTS)}, have {headed}")
    for key, fields in LISTS.items():
        names: list[str] = []
        for i, member in enumerate(data.get(key, [])):
            name = member.get("name", "")
            where = f"{key} {i} ({name or '?'})"
            if not NAME.match(name):
                found.append(f"{where}: {name!r} is not a name")
            if name in names:
                found.append(f"{where}: listed twice")
            names.append(name)
            for field in fields:
                if not member.get(field):
                    found.append(f"{where}: needs {field}")
            if key == "verbs" and tuple(member.get("values", {})) != VALUES:
                found.append(
                    f"{where}: values are {list(VALUES)}, each a number or null"
                )
            found += evidence_problems(where, member)
    return found


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
        found += evidence_problems(where, entry)
    if numbers != list(range(1, len(numbers) + 1)):
        found.append(
            f"exceptions: numbered {numbers}, want 1 to {len(numbers)} in order"
        )
    return found + list_problems(data)


def cite(text: str, refs: list[str]) -> str:
    """`text` with `refs` in parentheses before its final full stop."""
    if not refs:
        return text
    body = text[:-1] if text.endswith(".") else text
    return f"{body} ({', '.join(refs)})."


def item(entry: dict[str, Any]) -> str:
    """One entry as the text of its list item."""
    return f"**{entry['name']}.** {cite(entry['rule'], entry.get('reports', []))}{arena(entry)}"


def arena(entry: dict[str, Any]) -> str:
    """What Arena does, citing its moments, to follow an entry's own text; empty with none."""
    found = entry.get("arena")
    if not found:
        return ""
    moments = [f"{m['recording']} {m['time']}" for m in found.get("evidence", [])]
    return " " + cite(found["does"], moments)


def member(key: str, entry: dict[str, Any]) -> str:
    """One verb or mark as the text of its list item."""
    said = " ".join(entry[field] for field in LISTS[key])
    return f"**{entry['name']}.** {said}{arena(entry)}"


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
    for section in data.get("lists", []):
        key = section["key"]
        out += [f"## {section['heading']}", *section.get("intro", [])]
        out.append("\n".join(f"- {member(key, e)}" for e in data[key]))
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
