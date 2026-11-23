"""Adapters that list a check family's checks and report their results to Ignite (see
docs/ignite.md, "Declaring a family" and "Reporting results").

An adapter imports no module of the orchestration that Ignite replaces; the families' commands
are declared in `ignite/checks/`.

`report` is the shared half of an adapter: a family's `list` command prints its checks with
`listed`, and its `run` command reports with `report`.
"""

from __future__ import annotations

import json
import os
import sys
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path

# The variable naming the file a `run` command appends its results to. When it is unset, as in
# a run by hand, results go to standard output.
RESULTS_ENV = "IGNITE_RESULTS"


@dataclass(frozen=True)
class Listed:
    """One check a `list` command prints: its name, `<family>:<name>`, and any inputs of its
    own, written as a manifest's `inputs` entries are (a path pattern or `crate:<package>`).
    """

    check: str
    inputs: list[str] = field(default_factory=list)
    # The workspace package the check belongs to, for a family whose build products are built
    # per package (`crate-closure` in a product's inputs).
    package: str = ""
    # The local calibration for a machine-relative check.
    digest: str = ""


@dataclass(frozen=True)
class Result:
    """One check's outcome, `pass` or `fail`, with what failed and where (`file:line`)."""

    check: str
    outcome: str
    message: str = ""
    at: str = ""


def listed(checks: Iterable[Listed]) -> None:
    """Prints `checks`, one JSON object per line: `{"check": ..., "inputs": [...]}`."""
    for entry in checks:
        line: dict[str, object] = {"check": entry.check}
        if entry.inputs:
            line["inputs"] = entry.inputs
        if entry.package:
            line["package"] = entry.package
        if entry.digest:
            line["digest"] = entry.digest
        print(json.dumps(line), flush=True)


def report(results: Iterable[Result]) -> None:
    """Appends `results` to the file `IGNITE_RESULTS` names, one JSON object per line, or prints
    them. A check asked for and never reported is left `incomplete` by Ignite."""
    lines: list[str] = []
    for result in results:
        line: dict[str, str] = {"check": result.check, "outcome": result.outcome}
        if result.message:
            line["message"] = result.message
        if result.at:
            line["at"] = result.at
        lines.append(json.dumps(line) + "\n")
    target = os.environ.get(RESULTS_ENV)
    if target:
        with Path(target).open("a", encoding="utf-8") as out:
            out.writelines(lines)
    else:
        sys.stdout.writelines(lines)
        sys.stdout.flush()
