"""The design language as data (docs/project-50k.md, K28): `data/ui-conventions.json` holds
every ruling on where Beryl's UI follows or leaves MTG Arena, and `docs/ui-conventions.md` is
generated from it. `by conventions` writes the page; the repo lane fails when it is stale.

Each entry has a kind: a numbered Beryl `exception`, a rule `settled` as Arena, one with
`no-equivalent` in Arena, one `not-seen` in the recordings, or one still `open`. It states the
rule, the report beads behind it, and what Arena does with the recording moments that show it.
One that shows a prompt's cards a second time names that prompt's shape as `twice`: the
duplicate audit lets the cards such a prompt is about show again while it is open
(`ui/scripts/stage-audit.mjs`), and nothing else.

The closed lists follow: the `verbs` a script's steps apply, each with what it shows, what Arena
does and the values the stage plays it with, and the `marks` a card can carry, each with what it
means and its one look. Their names are the session's (`beryl_session`'s `script::Verb` and
`script::Mark`), and a test there fails when the two disagree.

A verb's values are Beryl's own (the exception "Motion is Beryl's own"): the stage plays it with
them and `by motion` holds the stage to them. Where a verb has several cases (a draw, a
discard), its `cases` hold each case's own, and a value a case leaves null is the verb's.
`data/arena-motion.json` is the reference they started from (K27): each verb's instances in the
recordings, how each was taken and their summary, or why the recordings cannot show the verb.
`by conventions` derives the summaries from the instances, and the check fails when they
disagree. A verb another numbered exception covers is `exempt`.

The `effects` are the third closed list: the light and changes of surface the stage plays on a
card, each with what it shows, the `layer` it is drawn in (under its card or over it) and its
own values, a duration of null lasting as long as the step that begins it. A verb's `effects`
are those its steps begin, each `when` its step starts or ends, or as a strike's `blow` lands:
every step's, or only those of a `case`, or `into` a zone. One is `on` what the step is about,
unless it says it is on what the step `struck` (what a strike or a streak lands on) or on its
`source` (what a streak leaves). A case with effects of its own begins none of its verb's.
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
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
LISTS: dict[str, tuple[str, ...]] = {
    "verbs": ("shows",),
    "effects": ("shows",),
    "marks": ("means", "look"),
}
NAME: re.Pattern[str] = re.compile(r"^[a-z]+(-[a-z]+)*$")
# A prompt shape's name, as the session's views say it (`beryl_session`'s `ShapeName`).
SHAPE: re.Pattern[str] = re.compile(r"^[a-z]+([A-Z][a-z]+)*$")
# What the stage plays a verb with, Beryl's own; null where the verb (or a case, its verb) has
# none.
VALUES = ("duration", "delay", "stagger", "curve")
# What the stage plays an effect with: a duration of null is its step's.
EFFECT_VALUES = ("duration", "delay", "curve")
# Where an effect is drawn against the card it is on, which end of its step begins it (or the
# blow of a strike, part of the way through), and what it is on when that is not what its step
# is about.
LAYERS = ("under", "over")
WHENS = ("start", "end", "blow")
ONS = ("struck", "source")

MOTION: Path = paths.REPO / "data" / "arena-motion.json"
# How an instance was taken: by `by video measure`, or counted on a filmstrip.
METHODS = ("tracker", "filmstrip")
CURVES = ("linear", "ease-in", "ease-out", "ease-in-out", "none")
# An instance's timings, in whole ms, and the verb's value each one's median gives.
TIMINGS = {"duration_ms": "duration", "delay_ms": "delay", "stagger_ms": "stagger"}
# An instance's sizes, as multiples of its size at rest: the largest in its motion, and where
# the motion begins.
SIZES = ("peak", "start")
# Thin evidence is no evidence (docs/project-50k.md, Risks): instances, over recordings.
FEWEST = (3, 2)


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
            if key == "verbs":
                found += value_problems(where, member, exceptions(data))
                found += use_problems(where, member, data)
            if key == "effects":
                found += effect_problems(where, member)
            found += evidence_problems(where, member)
    return found


def exceptions(data: dict[str, Any]) -> list[int]:
    """The numbers of the Beryl exceptions."""
    return [e["number"] for e in data.get("entries", []) if "number" in e]


def value_problems(where: str, verb: dict[str, Any], numbered: list[int]) -> list[str]:
    """Why a verb's values, or a case's, are not ms and a curve, or the exception that exempts
    it is none."""
    found: list[str] = []
    named = [("values", verb.get("values", {}))] + [
        (f"case {case!r}", own) for case, own in verb.get("cases", {}).items()
    ]
    for name, values in named:
        if tuple(values) != VALUES or any(
            not isinstance(values[v], (int, type(None))) for v in VALUES[:3]
        ):
            found.append(f"{where}: {name} are {list(VALUES)}, each ms or null")
        elif values["curve"] not in (*CURVES, None):
            found.append(f"{where}: curve {values['curve']!r} is not one of {CURVES}")
    if "exempt" in verb and verb["exempt"] not in numbered:
        found.append(f"{where}: exempt by {verb['exempt']!r}, not an exception")
    return found


def effect_problems(where: str, effect: dict[str, Any]) -> list[str]:
    """Why an effect does not say where it is drawn and how it is played."""
    found: list[str] = []
    if effect.get("layer") not in LAYERS:
        found.append(f"{where}: layer is one of {LAYERS}")
    values = effect.get("values", {})
    if tuple(values) != EFFECT_VALUES or any(
        not isinstance(values[v], (int, type(None))) for v in EFFECT_VALUES[:2]
    ):
        found.append(f"{where}: values are {list(EFFECT_VALUES)}, each ms or null")
    elif values["curve"] not in CURVES:
        found.append(f"{where}: curve {values['curve']!r} is not one of {CURVES}")
    return found


def use_problems(where: str, verb: dict[str, Any], data: dict[str, Any]) -> list[str]:
    """Why the effects a verb's steps begin are not effects, or not begun by its steps."""
    found: list[str] = []
    effects = [effect.get("name") for effect in data.get("effects", [])]
    for use in verb.get("effects", []):
        if use.get("effect") not in effects:
            found.append(f"{where}: {use.get('effect')!r} is not an effect")
        if use.get("when") not in WHENS:
            found.append(f"{where}: an effect begins at one of {WHENS}")
        if "case" in use and use["case"] not in verb.get("cases", {}):
            found.append(f"{where}: {use['case']!r} is not one of its cases")
        if "on" in use and use["on"] not in ONS:
            found.append(f"{where}: an effect is on one of {ONS}, or on its step's own")
        if set(use) - {"effect", "when", "case", "into", "on"}:
            found.append(f"{where}: an effect's use is effect, when, case, into and on")
    return found


def spread(numbers: list[float], digits: int) -> dict[str, float] | None:
    """The median and range of some measurements; None with none."""
    if not numbers:
        return None
    median = round(statistics.median(numbers), digits)
    return {
        "median": median if digits else int(median),
        "min": min(numbers),
        "max": max(numbers),
    }


def summarize(instances: list[dict[str, Any]]) -> dict[str, Any]:
    """What a verb's instances say together, and each case's when they have several."""

    def taken(field: str) -> list[Any]:
        return [i[field] for i in instances if i.get(field) is not None]

    summary: dict[str, Any] = {field: spread(taken(field), 0) for field in TIMINGS}
    for field in SIZES:
        summary[field] = spread(taken(field), 2)
    curves = taken("curve")
    # The commonest curve; a tie goes to the first in CURVES.
    summary["curve"] = (
        max(CURVES, key=lambda c: (curves.count(c), -CURVES.index(c)))
        if curves
        else None
    )
    cases = list(dict.fromkeys(i.get("case") for i in instances))
    if len(cases) > 1:
        summary["cases"] = {
            case: summarize([i for i in instances if i.get("case") == case])
            for case in cases
        }
    return summary


def instance_problems(where: str, instance: dict[str, Any]) -> list[str]:
    """Why one instance does not say where it is, how it was taken and what it gave."""
    found = evidence_problems(where, {"arena": {"evidence": [instance]}})
    for field in ("case", "command", "path"):
        if not instance.get(field):
            found.append(f"{where}: needs {field}")
    if instance.get("method") not in METHODS:
        found.append(f"{where}: method is one of {METHODS}")
    elif instance["method"] == "filmstrip" and not instance.get("why"):
        found.append(f"{where}: a filmstrip needs why the tracker was not used")
    crop = instance.get("crop")
    if crop is not None and not (
        isinstance(crop, list)
        and len(crop) == 4
        and all(isinstance(c, int) for c in crop)
    ):
        found.append(f"{where}: crop is [x, y, w, h] or null")
    if not isinstance(instance.get("duration_ms"), int):
        found.append(f"{where}: needs duration_ms")
    for field in ("delay_ms", "stagger_ms"):
        if not isinstance(instance.get(field), (int, type(None))):
            found.append(f"{where}: {field} is whole ms or null")
    if instance.get("delay_ms") is not None and not instance.get("since"):
        found.append(f"{where}: a delay needs what it counts from")
    for field in SIZES:
        if not isinstance(instance.get(field), (int, float, type(None))):
            found.append(f"{where}: {field} is a ratio or null")
    if instance.get("curve") not in (*CURVES, None):
        found.append(f"{where}: curve is one of {CURVES} or null")
    return found


def motion_problems(motion: dict[str, Any], data: dict[str, Any]) -> list[str]:
    """Why Arena's measurements do not cover every verb, or their summaries are stale."""
    found: list[str] = [] if motion.get("about") else ["arena-motion: needs about"]
    verbs = {v["name"]: v for v in data.get("verbs", [])}
    names = [m.get("verb") for m in motion.get("verbs", [])]
    if names != list(verbs):
        found.append(f"arena-motion: verbs are {names}, want {list(verbs)}")
    unseen = [e["name"] for e in data.get("entries", []) if e["kind"] == "not-seen"]
    for entry in motion.get("verbs", []):
        name, instances = entry.get("verb"), entry.get("instances", [])
        where = f"arena-motion {name}"
        if "unseen" in entry:
            if not entry["unseen"] or instances or entry.get("not-seen") not in unseen:
                found.append(
                    f"{where}: an unseen verb says why and names its not-seen entry"
                )
            continue
        bad = [
            problem
            for i, instance in enumerate(instances)
            for problem in instance_problems(f"{where} {i}", instance)
        ]
        found += bad
        if bad:
            continue
        recordings = {i["recording"] for i in instances}
        if len(instances) < FEWEST[0] or len(recordings) < FEWEST[1]:
            found.append(
                f"{where}: {len(instances)} instances over {len(recordings)} recordings,"
                f" want {FEWEST[0]} over {FEWEST[1]}"
            )
        summary = summarize(instances)
        if entry.get("summary") != summary:
            found.append(f"{where}: summary is stale: run `just conventions`")
    return found


def derive(motion: dict[str, Any]) -> None:
    """Write each verb's summary of Arena's instances; its values are Beryl's own, untouched."""
    for entry in motion.get("verbs", []):
        if "unseen" not in entry:
            entry["summary"] = summarize(entry.get("instances", []))


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
        if "twice" in entry and not SHAPE.match(str(entry["twice"])):
            found.append(f"{where}: twice {entry['twice']!r} is not a prompt shape")
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
    said = f"**{entry['name']}.** {cite(entry['rule'], entry.get('reports', []))}{arena(entry)}"
    if "twice" in entry:
        said += (
            f" The cards a `{entry['twice']}` prompt is about are the one thing on screen"
            " twice: the duplicate audit allows them while it is open."
        )
    return said


def arena(entry: dict[str, Any]) -> str:
    """What Arena does, citing its moments, to follow an entry's own text; empty with none."""
    found = entry.get("arena")
    if not found:
        return ""
    moments = [f"{m['recording']} {m['time']}" for m in found.get("evidence", [])]
    return " " + cite(found["does"], moments)


def member(key: str, entry: dict[str, Any]) -> str:
    """One verb, effect or mark as the text of its list item."""
    said = " ".join(entry[field] for field in LISTS[key])
    if key == "effects":
        said += f" Drawn {entry['layer']} its card."
    return f"**{entry['name']}.** {said}{arena(entry)}{timing(entry)}{uses(entry)}"


WHEN_WORDS = {
    "start": "as it begins",
    "end": "as it ends",
    "blow": "as its blow lands",
}


ON_WORDS = {
    "struck": "(on what it strikes)",
    "source": "(on what it leaves)",
}


def uses(verb: dict[str, Any]) -> str:
    """The effects a verb's steps begin, in words, to follow its values; empty with none."""
    said = [
        " ".join(
            [
                use["effect"],
                WHEN_WORDS[use["when"]],
                *([f"({use['case']})"] if "case" in use else []),
                *([f"(into the {use['into']})"] if "into" in use else []),
                *([ON_WORDS[use["on"]]] if "on" in use else []),
            ]
        )
        for use in verb.get("effects", [])
    ]
    return f" Effects: {', '.join(said)}." if said else ""


def timing(verb: dict[str, Any]) -> str:
    """A verb's values in words, to follow what Arena does; empty with none."""
    if "exempt" in verb:
        return f" Exempt: exception {verb['exempt']}."
    values = verb.get("values", {})
    said = [
        text.format(values[value])
        for value, text in (
            ("duration", "{} ms"),
            ("delay", "{} ms after its cause"),
            ("stagger", "{} ms apart"),
        )
        if values.get(value) is not None
    ]
    if values.get("curve"):
        # A curve of none: nothing travels (a fade, a pop, a hold).
        said.append("in place" if values["curve"] == "none" else values["curve"])
    return f" Beryl: {', '.join(said)}." if said else ""


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


def load(path: Path = DATA, motion: Path | None = MOTION) -> dict[str, Any]:
    """The conventions, checked, and checked against Arena's measurements unless `motion` is
    None."""
    data = json.loads(path.read_text())
    found = [f"{path.name}: {p}" for p in problems(data)]
    if motion is not None and not found:
        found = motion_problems(json.loads(motion.read_text()), data)
    if found:
        raise ValueError("\n".join(found))
    return data


def write(path: Path, data: dict[str, Any]) -> None:
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def stale(
    data_path: Path = DATA, page: Path = PAGE, motion: Path | None = MOTION
) -> bool:
    """Whether the page differs from what its data renders."""
    have = page.read_text() if page.exists() else ""
    return have != render(load(data_path, motion))


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
    motion = json.loads(MOTION.read_text())
    derive(motion)
    write(MOTION, motion)
    try:
        PAGE.write_text(render(load()))
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1
    print(f"wrote {PAGE.relative_to(paths.REPO)}")
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "conventions",
        help="summarize Arena's motion and write docs/ui-conventions.md (--check)",
    )
    p.add_argument("--check", action="store_true", help="fail when the page is stale")
    p.set_defaults(func=run)
