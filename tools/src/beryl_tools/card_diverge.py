"""`card diverge`: record that one oracle, XMage or Forge, is wrong about a card's scenario.

A card that is right can still fail in an oracle: XMage's Spellseeker must find a card although
CR 701.23b lets the player find none. An entry in data/oracle-divergences.json lets that failure
pass, which also makes it the easiest way to hide a wrong definition. So a card writer never edits
the file. `card diverge <card> <scenario> <xmage|forge> --cr <rule> --reason <text>` plays the
card's scenarios in both oracles and writes the entry only when:

- the named oracle fails that scenario;
- the other oracle plays it and passes, so two engines of three (Beryl, the other oracle) agree
  and the named one is the odd one out;
- the rule exists in the release pinned in data/rules-version.json, whose text the tool quotes
  itself.

Otherwise it refuses and says why. When both oracles fail the scenario, or the other cannot play
it, the definition is treated as wrong.

Two checks keep the file honest after that. `divergences` (a step of the `names` lane) fails an
entry that is malformed, names no single card, scenario and oracle, or whose `cr` does not quote the rule it names. The oracle run fails (`faults`) when an entry's scenario now
passes in its own oracle, so that a fixed XMage or Forge loses its entries, or when the other
oracle does not pass it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Mapping
from pathlib import Path
from typing import cast

from . import card, card_check, card_store, citations, paths

Json = card_store.Json

ORACLES = ("xmage", "forge")
DIVERGENCES: Path = paths.REPO / "data" / "oracle-divergences.json"
FIELDS = {"oracle", "card", "scenario", "cr", "reason"}
# The `cr` field: `CR 701.23b (link): "quotation"`.
CR_FIELD: re.Pattern[str] = re.compile(
    r"^CR (?P<number>\d{1,3}(?:\.\d+[a-z]*)?) "
    r'\(https://yawgatog\.com/resources/magic-rules/#(?P<anchor>\w+)\): "(?P<quote>.+)"$'
)


class Refused(Exception):
    """The tool will not write the entry; the message says why."""


def other(oracle: str) -> str:
    return ORACLES[1 - ORACLES.index(oracle)]


def statuses(report: Mapping[str, object], oracle: str) -> dict[str, str]:
    """Each scenario of `oracle`'s result in an oracle report, "card / scenario" to its
    status."""
    result = cast(Json, report.get(oracle, {}))
    return {
        f"{s['card']} / {s['name']}": str(s["status"])
        for s in cast(list[Json], result.get("scenarios", []))
    }


def rule_quotation(number: str, rules: Mapping[str, str]) -> str | None:
    """The `cr` field for rule `number`: the rule's own text, without its examples, linked to
    the rules page; None when the pinned release has no such rule."""
    anchor = citations.anchor_of(number)
    if anchor not in rules:
        return None
    text = rules[anchor].split(" Example: ")[0].strip()
    link = f"https://yawgatog.com/resources/magic-rules/#{anchor}"
    return f'CR {number} ({link}): "{text}"'


def judge(
    report: Mapping[str, object],
    card_name: str,
    scenario: str,
    oracle: str,
    number: str,
    reason: str,
    rules: Mapping[str, str],
) -> dict[str, str]:
    """The entry for `oracle` failing `scenario` of `card_name`, or `Refused` saying why not,
    from an oracle report (`oracle.json`) of the card's scenarios."""
    if oracle not in ORACLES:
        raise Refused(f"the oracle is xmage or forge, not {oracle!r}")
    if not reason.strip():
        raise Refused("--reason must say what that oracle does")
    key = f"{card_name} / {scenario}"
    named = statuses(report, oracle).get(key)
    if named is None:
        raise Refused(f"{oracle} did not play {key!r}")
    if named == "allowlisted":
        raise Refused(f"{oracle} already has an entry for {key!r}")
    if named != "fail":
        raise Refused(
            f"{oracle} does not fail {key!r} (it is {named}), so there is nothing to record"
        )
    agreeing = other(oracle)
    seen = statuses(report, agreeing).get(key)
    if seen == "fail":
        raise Refused(
            f"{agreeing} fails {key!r} too, so both oracles disagree with Beryl:"
            " the definition is wrong"
        )
    if seen != "pass":
        raise Refused(
            f"{agreeing} did not play {key!r} (it is {seen or 'missing'}), so nothing shows"
            f" that {oracle} is the one that is wrong"
        )
    cr = rule_quotation(number.removeprefix("CR ").strip(), rules)
    if cr is None:
        raise Refused(f"CR {number} is not a rule in the pinned release")
    return {
        "oracle": oracle,
        "card": card_name,
        "scenario": scenario,
        "cr": cr,
        "reason": reason.strip(),
    }


def problems(entries: object, rules: Mapping[str, str]) -> list[str]:
    """What is wrong with the entries of the divergences file, one line each."""
    if not isinstance(entries, list):
        return ["the file is not a list of entries"]
    found: list[str] = []
    seen: set[tuple[str, str, str]] = set()
    for index, entry in enumerate(entries):
        where = f"entry {index + 1}"
        if not isinstance(entry, dict) or set(entry) != FIELDS:
            found.append(f"{where}: must have exactly the fields {sorted(FIELDS)}")
            continue
        fields = entry
        if not all(isinstance(v, str) and v.strip() for v in fields.values()):
            found.append(f"{where}: every field must be a non-empty string")
            continue
        text = cast(dict[str, str], fields)
        where = f"{text['card']} / {text['scenario']} ({text['oracle']})"
        if text["oracle"] not in ORACLES:
            found.append(f"{where}: the oracle is xmage or forge")
        if "*" in text["card"] or "*" in text["scenario"]:
            found.append(f"{where}: no wildcard; name one card and one scenario")
        key = (text["oracle"], text["card"], text["scenario"])
        if key in seen:
            found.append(f"{where}: listed twice")
        seen.add(key)
        match = CR_FIELD.match(text["cr"])
        if match is None:
            found.append(f'{where}: `cr` must read CR <rule> (<link>): "<rule text>"')
            continue
        number = match["number"]
        anchor = citations.anchor_of(number)
        if anchor not in rules:
            found.append(f"{where}: CR {number} is not a rule in the pinned release")
        elif anchor != match["anchor"]:
            found.append(f"{where}: the link is not CR {number}'s")
        else:
            # An entry may quote part of the rule, in pieces between ellipses.
            quote = citations.normalize(match["quote"]).replace('"', "'")
            if not all(s in rules[anchor] for s in citations.segments(quote)):
                found.append(f"{where}: the quotation is not CR {number}'s text")
    return found


def faults(results: Mapping[str, object]) -> list[str]:
    """The entries of an oracle run's results (name to result) that should not stand: those
    whose scenario now passes in the oracle they blame, which has been fixed, and those whose
    scenario the other oracle does not pass. Prints each."""
    found: list[str] = []
    for engine in ORACLES:
        if engine not in results:
            continue
        for s in cast(list[Json], cast(Json, results[engine]).get("scenarios", [])):
            key = f"{s['card']} / {s['name']}"
            if s.get("stale_allowlist"):
                found.append(f"{engine} now passes {key!r}: remove its entry")
            elif s["status"] == "allowlisted" and other(engine) in results:
                if statuses(results, other(engine)).get(key) != "pass":
                    found.append(
                        f"{other(engine)} does not pass {key!r}, so {engine}'s entry"
                        " has no backing"
                    )
    for line in found:
        print(f"oracle: divergence: {line}")
    return found


def failed(results: Mapping[str, Json]) -> bool:
    """Whether an oracle run fails: an oracle failed a scenario with no entry, or an entry
    should not stand (`faults`)."""
    return any(r["fail"] for r in results.values()) or bool(faults(results))


def step(*args: str) -> int:
    """The `divergences` step: `--fetch` to fetch the pinned rules when they are not cached."""
    version = citations.pinned(paths.REPO)
    rules, missing = citations.step_rules(version, bool(args) and args[0] == "--fetch")
    if rules is None:
        print(f"divergences: skipped: {missing}")
        return 0
    found = problems(json.loads(DIVERGENCES.read_text(encoding="utf-8")), rules)
    for line in found:
        print(f"divergences: {line}")
    return 1 if found else 0


def oracle_report(name: str) -> Json | str:
    """The report of every oracle's run of card `name`'s scenarios, or why there is none."""
    result = card_check.command(
        [sys.executable, "-m", "beryl_tools.cli", "oracle", name, "--no-sim"],
        paths.REPO,
    )
    start = result.stdout.find("\n{")
    text = result.stdout[start + 1 :] if start >= 0 else result.stdout
    try:
        summary = cast(Json, json.loads(text)) if text.startswith("{") else None
    except json.JSONDecodeError:
        summary = None
    if summary is None:
        return "the oracles did not run: " + card_check.first_error(
            result.stdout + result.stderr
        )
    return cast(Json, json.loads(Path(str(summary["report"])).read_text()))


def write(path: Path, entry: dict[str, str]) -> None:
    entries = cast(list[object], json.loads(path.read_text(encoding="utf-8")))
    entries.append(entry)
    path.write_text(
        json.dumps(entries, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


def run(args: argparse.Namespace, sources: card.Sources = card.Sources()) -> int:
    with card_store.Store(sources.store) as store:
        found = card.find(args.name, store)
    if isinstance(found, str):
        print(f"card: {found}", file=sys.stderr)
        return 1
    name = str(found["name"])
    version = citations.pinned(paths.REPO)
    rules, missing = citations.step_rules(version, True)
    if rules is None:
        print(f"card diverge: refused: {missing}", file=sys.stderr)
        return 1
    report = oracle_report(name)
    if isinstance(report, str):
        print(f"card diverge: refused: {report}", file=sys.stderr)
        return 1
    try:
        entry = judge(
            report, name, args.scenario, args.oracle, args.cr, args.reason, rules
        )
    except Refused as e:
        print(f"card diverge: refused: {e}", file=sys.stderr)
        return 1
    write(DIVERGENCES, entry)
    print(
        f"card diverge: recorded {args.oracle}'s fault in {args.scenario!r} of {name}"
    )
    return 0


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = commands.add_parser(
        "diverge",
        help="record that XMage or Forge is wrong about a scenario, when the other agrees",
    )
    p.add_argument("name", help="the card's name or slug")
    p.add_argument("scenario", help="the scenario's name, as `card check` prints it")
    p.add_argument("oracle", choices=ORACLES, help="the oracle that is wrong")
    p.add_argument("--cr", required=True, help="the rule that says so, e.g. 701.23b")
    p.add_argument("--reason", required=True, help="what that oracle does instead")
    p.set_defaults(func=run)
