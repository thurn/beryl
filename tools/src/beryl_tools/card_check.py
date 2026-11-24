"""`card check <name>`: whether a card works, in steps.

The steps run in order, each through the command that already does it, and the first failure
stops the rest:

1. `build`: `cargo check` of the one set crate that holds the card, with its scenarios, not the
   workspace.
2. `tests`: that crate's tests in the card's own module, if its file has any.
3. `beryl`: the card's scenarios in Beryl's strict runner, `beryl_scenario run --card`: the
   hand-written ones of its file and the rules library's, and the ones generated for it (K3).
4. `generated`: mutation coverage from generated scenarios alone. A file with no
   scenarios of its own must have generated scenarios that kill every mutant.
5. `oracles`: the same scenarios in XMage and Forge, `by oracle <name> --no-sim`, which holds
   the oracle lock and applies data/oracle-divergences.json and data/oracle-exporter-gaps.json.
   A card without scenarios of its own must have a generated scenario that passes in at
   least one oracle. `--no-oracles` skips the oracle step for a fast loop.
6. `render`: the rules text the card's definition renders to against its Oracle text in the
   card store, normalized (render_diff.py). A difference fails the step and prints the
   paragraphs that differ, since a definition that reads differently from the card most often
   does something different, unless data/card-text-allowlist.json pins exactly that difference
   (card_text.py).

Each step prints one line, its name, `pass`, `fail` or `skip`, and what it found; a failure
then prints the command's output. The command exits 1 when a step fails.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from . import card, card_store, card_text, paths, render_diff, set_groups, store_cards

Json = card_store.Json


@dataclass(frozen=True)
class Card:
    """What the steps need to know of the card being checked."""

    name: str
    # Its set crate, e.g. `cards_1993`.
    crate: str
    # The path of its module in that crate, e.g. `lea::counterspell`; None for a card complete
    # from the store, which has no file.
    module: str | None
    # Its faces' Oracle text, from the card store.
    oracle: tuple[render_diff.Face, ...] = ()


@dataclass(frozen=True)
class Outcome:
    status: str  # "pass", "fail" or "skip"
    said: str
    # The command's output, printed when the step fails.
    output: str = ""


def command(
    args: list[str], cwd: Path = paths.ENGINE
) -> subprocess.CompletedProcess[str]:
    """Runs a step's command with the binaries' target directory, capturing what it says."""
    return subprocess.run(
        args,
        cwd=cwd,
        env=paths.run_env(),
        capture_output=True,
        text=True,
        check=False,
    )


def first_error(output: str) -> str:
    """The line of `output` that says what went wrong: rustc's first error, else its last
    line."""
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    errors = [line for line in lines if line.startswith("error")]
    return (errors or lines or ["no output"])[0 if errors else -1]


def build(target: Card) -> Outcome:
    result = command(
        ["cargo", "check", "--quiet", "-p", target.crate, "--features", "scenarios"]
    )
    output = result.stdout + result.stderr
    if result.returncode != 0:
        return Outcome("fail", first_error(output), output)
    return Outcome("pass", f"cargo check -p {target.crate}")


def tests(target: Card) -> Outcome:
    if target.module is None:
        return Outcome("skip", "no file, so no tests of its own")
    result = command(
        [
            "cargo",
            "test",
            "--quiet",
            "-p",
            target.crate,
            "--features",
            "scenarios",
            "--lib",
            "--",
            f"{target.module}::",
        ]
    )
    output = result.stdout + result.stderr
    if result.returncode != 0:
        return Outcome("fail", first_error(output), output)
    passed = sum(
        int(line.split()[3])
        for line in output.splitlines()
        if line.startswith("test result: ok.")
    )
    return Outcome("pass", f"{passed} passed" if passed else f"none in {target.module}")


def scenario_command(action: str, target: Card) -> subprocess.CompletedProcess[str]:
    return command(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "beryl_cards",
            "--features",
            "scenarios",
            "--bin",
            "beryl_scenario",
            "--",
            action,
            "--card",
            target.name,
        ]
    )


def beryl(target: Card) -> Outcome:
    result = scenario_command("run", target)
    output = result.stdout + result.stderr
    lines = output.strip().splitlines()
    summary = lines[-1] if lines else "no output"
    if result.returncode != 0:
        failures = [line for line in lines if line.startswith("fail ")]
        return Outcome("fail", failures[0] if failures else summary, output)
    if summary.startswith("0 hand-written, 0 generated"):
        return Outcome("fail", "no scenarios", output)
    return Outcome("pass", summary)


def generated(target: Card) -> tuple[Outcome, bool]:
    """Reports kills from generated scenarios and whether the card relies on them."""
    result = scenario_command("generated", target)
    if result.returncode != 0:
        output = result.stdout + result.stderr
        return Outcome("fail", first_error(output), output), False
    report = cast(Json, json.loads(result.stdout))
    mutants = cast(int, report["mutants"])
    survivors = cast(list[str], report["survivors"])
    count = cast(int, report["generated"])
    requires_generated = report["written"] == 0
    eligible = count > 0 and not survivors
    if not count:
        decision = "no generated scenarios"
    elif survivors:
        decision = "needs hand-written scenarios"
    else:
        decision = "eligible for generated-only coverage pending oracle checks"
    said = (
        f"{mutants - len(survivors)}/{mutants} mutants killed, {report['cut']} answers cut, "
        f"{decision}"
    )
    if survivors:
        said += f"; survivors: {'; '.join(survivors)}"
    return (
        Outcome("fail" if requires_generated and not eligible else "pass", said),
        requires_generated,
    )


def oracle_counts(engine: str, result: Json) -> str:
    """One oracle's counts and each unsupported scenario's reason: `xmage: 3 pass, 1
    unsupported (no exporter for ...)`."""
    counts = [
        f"{result[status]} {status}"
        for status in ("pass", "fail", "allowlisted", "unsupported")
        if result.get(status)
    ]
    reasons = sorted(
        {
            str(s["reason"])
            for s in cast(list[Json], result.get("scenarios", []))
            if s.get("status") == "unsupported"
        }
    )
    why = f" ({'; '.join(reasons)})" if reasons else ""
    return f"{engine}: {', '.join(counts) or 'no scenarios'}{why}"


def oracles(target: Card, requires_generated: bool) -> Outcome:
    result = command(
        [sys.executable, "-m", "beryl_tools.cli", "oracle", target.name, "--no-sim"],
        paths.REPO,
    )
    output = result.stdout + result.stderr
    # The summary is the JSON object the run prints last, from its first line to the end.
    start = result.stdout.find("\n{")
    text = result.stdout[start + 1 :] if start >= 0 else result.stdout
    try:
        summary = cast(Json, json.loads(text)) if text.startswith("{") else None
    except json.JSONDecodeError:
        summary = None
    if summary is None:
        return Outcome("fail", first_error(output), output)
    said = "; ".join(
        oracle_counts(engine, cast(Json, summary[engine]))
        for engine in ("xmage", "forge")
        if engine in summary
    )
    if result.returncode != 0:
        return Outcome("fail", said, output)
    generated_passes = sum(
        cast(int, cast(Json, summary[engine]).get("generated_pass", 0))
        for engine in ("xmage", "forge")
        if engine in summary
    )
    if not generated_passes:
        said = f"no generated scenario passed either oracle; {said}"
        if requires_generated:
            return Outcome("fail", said, output)
    else:
        checks = "check" if generated_passes == 1 else "checks"
        said += f"; generated scenarios passed {generated_passes} oracle {checks}"
    return Outcome("pass", said)


def render(target: Card) -> Outcome:
    found = render_diff.parse(command(render_diff.render_command([target.name])))
    if isinstance(found, str):
        return Outcome("fail", first_error(found), found)
    if len(found) != 1:
        return Outcome("fail", f"rendered {len(found)} cards, not one")
    diff = render_diff.compare(target.name, list(target.oracle), found[0][1])
    if diff.matches:
        return Outcome("pass", "matches its Oracle text")
    allowlist = card_text.load(card_text.ALLOWLIST)
    escalations = card_text.load(card_text.ESCALATIONS)
    pinned = {target.name: allowlist[target.name]} if target.name in allowlist else {}
    if not card_text.problems({target.name: diff}, pinned, escalations):
        reason = cast(Json, pinned[target.name])["reason"]
        return Outcome("pass", f"differs as {card_text.ALLOWLIST.name} pins ({reason})")
    return Outcome(
        "fail",
        f"{len(diff.lines)} {'paragraph differs' if len(diff.lines) == 1 else 'paragraphs differ'}"
        " from its Oracle text",
        "\n".join(diff.lines),
    )


def steps(target: Card, no_oracles: bool) -> Iterator[tuple[str, Outcome]]:
    """Run one step at a time so a failure can stop the check."""
    yield "build", build(target)
    yield "tests", tests(target)
    yield "beryl", beryl(target)
    report, requires_generated = generated(target)
    yield "generated", report
    yield "oracles", (
        Outcome("skip", "--no-oracles")
        if no_oracles
        else oracles(target, requires_generated)
    )
    yield "render", render(target)


def check(target: Card, no_oracles: bool) -> int:
    """Runs the steps on `target`, printing a line for each, until one fails."""
    for name, outcome in steps(target, no_oracles):
        print(f"{name}: {outcome.status}: {outcome.said}", flush=True)
        if outcome.status == "fail":
            if outcome.output.strip():
                print(outcome.output.rstrip())
            print(f"card check: {target.name} failed at {name}")
            return 1
        if name == "oracles" and outcome.status == "pass":
            from . import oracle_discovery

            oracle_discovery.card_diagnostics(target.name)
    print(f"card check: {target.name} passed")
    return 0


def run(args: argparse.Namespace, sources: card.Sources = card.Sources()) -> int:
    with card_store.Store(sources.store) as store:
        found = card.find(args.name, store)
    if isinstance(found, str):
        print(f"card: {found}", file=sys.stderr)
        return 1
    name = str(found["name"])
    groups = set_groups.SetGroups()
    path = groups.card_path(found, sources.cards)
    if path.is_file():
        module: str | None = f"{path.parent.name}::{path.stem}"
    elif store_cards.complete(found) is not None:
        module = None
    else:
        print(
            f'card check: {name} has no file; `just card new "{name}"` starts one',
            file=sys.stderr,
        )
        return 1
    oracle = tuple(render_diff.oracle_faces(found))
    return check(Card(name, groups.group_of(found), module, oracle), args.no_oracles)


def register(commands: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = commands.add_parser(
        "check",
        help="build the card's set crate and play its scenarios in Beryl, XMage and Forge",
    )
    p.add_argument("name", help="the card's name or slug, e.g. 'Lightning Bolt'")
    p.add_argument(
        "--no-oracles",
        action="store_true",
        help="skip XMage and Forge, for a fast loop",
    )
    p.set_defaults(func=run)
