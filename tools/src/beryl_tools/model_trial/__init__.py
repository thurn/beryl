"""`by model-trial` (`just model-trial`): run a model trial, in which a worker on one model works
a copy of real tickets that the pool works on another, and the two results are compared.
Every command takes `--trial <epic>`: the trial's epic holds its spec in its notes, and its
records are in `$BERYL_CACHE/model-trial/<epic>/`.
"""

from __future__ import annotations

import argparse
import sys
from collections.abc import Callable

from pathlib import Path

from . import candidates, grade, io, judge, new, pair, report, review
from .spec import Spec, SpecError, from_file, from_notes

Command = Callable[[argparse.Namespace, Spec], int]


def load_spec(trial: str) -> Spec:
    return from_notes(str(io.ticket(trial).get("notes") or ""))


def run(command: Command, args: argparse.Namespace) -> int:
    draft: str | None = getattr(args, "spec", None)
    try:
        spec = from_file(Path(draft), draft=True) if draft else load_spec(args.trial)
    except SpecError as error:
        where = draft or f"{args.trial}'s trial spec"
        print(f"model-trial: {where}: {error}", file=sys.stderr)
        return 2
    try:
        return command(args, spec)
    except (pair.PairError, KeyError, SpecError, io.IgniteError) as error:
        print(f"model-trial: {error}", file=sys.stderr)
        return 1


def _command(
    sub: argparse._SubParsersAction[argparse.ArgumentParser],
    name: str,
    help: str,
    command: Command,
    *,
    draft: bool = False,
) -> argparse.ArgumentParser:
    parser = sub.add_parser(name, help=help)
    if draft:
        source = parser.add_mutually_exclusive_group(required=True)
        source.add_argument("--trial", help="the trial's epic")
        source.add_argument(
            "--spec",
            help="a spec file, for a trial not yet filed; `tickets` may be empty",
        )
    else:
        parser.add_argument("--trial", required=True, help="the trial's epic")
    parser.set_defaults(func=lambda args: run(command, args))
    return parser


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    trial = sub.add_parser(
        "model-trial", help="file and grade model trial pairs (see tools/AGENTS.md)"
    )
    verbs = trial.add_subparsers(dest="verb", required=True)
    found = _command(
        verbs,
        "candidates",
        "list tickets that could be the source arm",
        candidates.run,
        draft=True,
    )
    found.add_argument("--epic", action="append", help="only tickets under this epic")
    found.add_argument("--label", action="append", help="only tickets with this label")
    _command(verbs, "grade", "measure every finished pair", grade.cmd_grade)
    _command(
        verbs,
        "review",
        "run the blind reviewer on every graded pair",
        review.cmd_review,
    )
    shown = verbs.add_parser("report", help="print the trial's table")
    shown.add_argument(
        "--trial",
        action="append",
        required=True,
        help="the trial's epic; give two or more to compare trials",
    )
    shown.add_argument("--comment", help="also post the table on this ticket")
    shown.set_defaults(func=report.cmd_report)
    listed = verbs.add_parser("list", help="every trial with records on this machine")
    listed.set_defaults(func=report.cmd_list)
    new.register(verbs)
    page = _command(verbs, "judge", "serve the blind judging page", judge.cmd_judge)
    page.add_argument(
        "--port", type=int, default=judge.PORT, help="0 for any free port"
    )

    pairs = verbs.add_parser("pair", help="list, mark, plant, title and file pairs")
    steps = pairs.add_subparsers(dest="step", required=True)
    add = _command(steps, "add", "record and park listed tickets", pair.cmd_add)
    add.add_argument("tickets", nargs="+")
    block = _command(
        steps, "next", "mark the next block of listed tickets", pair.cmd_next
    )
    block.add_argument("--block", type=int, required=True)
    block.add_argument("--count", type=int, help="how many (the spec's block size)")
    plant = _command(
        steps, "plant", "write a false premise into a pair", pair.cmd_plant
    )
    plant.add_argument("pair", help="the pair's token, source or copy")
    plant.add_argument("--find", required=True, help="the true sentence")
    plant.add_argument("--replace-with", required=True, help="the false one")
    plant.add_argument(
        "--expect", required=True, help="what a good response looks like"
    )
    title = _command(steps, "title", "set the copy's reworded title", pair.cmd_title)
    title.add_argument("pair")
    title.add_argument("title")
    title.add_argument(
        "--arm", help="the copy arm this title is for (the spec's first copy arm)"
    )
    audit = _command(
        steps, "audit", "record what a planted source landed", pair.cmd_audit
    )
    audit.add_argument("pair")
    audit.add_argument("--harm", required=True, help="`none` or `fixed:<ticket>`")
    audit.add_argument("--note", required=True)
    filing = _command(steps, "file", "file every marked pair", pair.cmd_file)
    filing.add_argument(
        "--then", help="the next block or report ticket, which filing defers"
    )
    wait = _command(
        steps, "wait", "exit 3 while a filed pair has an arm unfinished", pair.cmd_wait
    )
    wait.add_argument("--defer", help="a released ticket to defer while that is so")
    taint = _command(
        steps, "contamination", "void or clear a suspect pair", pair.cmd_contamination
    )
    taint.add_argument("pair")
    choice = taint.add_mutually_exclusive_group(required=True)
    choice.add_argument("--void", action="store_true")
    choice.add_argument("--clear", action="store_true")
    taint.add_argument("--note", required=True)
