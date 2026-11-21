"""Record the native frame counters and optional Callgrind counts for each gated board."""

from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
import tempfile
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from pathlib import Path

from .. import paths as repo_paths
from .. import system
from . import gate, paths
from .command import Command

Boards = dict[str, dict[str, float]]
OUTPUT_LIMIT = 24
WEEK = timedelta(days=7)


@dataclass(frozen=True)
class Measurement:
    commit: str
    time: datetime
    boards: Boards


def series_path() -> Path:
    return paths.cache() / "trend" / "frames.jsonl"


def current_commit() -> str:
    return subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=repo_paths.REPO, text=True
    ).strip()


def load(path: Path) -> list[Measurement]:
    """Read the series in recording order. Every board of a measurement shares its timestamp."""
    measurements: dict[tuple[str, datetime], Boards] = {}
    try:
        with path.open(encoding="utf-8") as series:
            for line in series:
                row = json.loads(line)
                key = row["commit"], datetime.fromisoformat(row["time"])
                measurements.setdefault(key, {})[row["board"]] = row["counts"]
    except FileNotFoundError:
        pass
    return [
        Measurement(commit, time, boards)
        for (commit, time), boards in measurements.items()
    ]


def through_commit(history: list[Measurement], commit: str) -> list[Measurement]:
    """The series as it stood when this commit first got measured."""
    for index, measurement in enumerate(history):
        if measurement.commit == commit:
            return history[: index + 1]
    return []


def record(
    path: Path, commit: str, collect: Callable[[], Boards], *, reuse: bool = False
) -> list[Measurement]:
    """Append a complete batch atomically so readers never see half of a commit's boards.
    Exploration can reuse a measurement left by a prior run, including after interruption.
    """
    if reuse and (already := through_commit(load(path), commit)):
        return already
    boards = collect()
    if not boards:
        raise ValueError("the gate reported no boards")
    path.parent.mkdir(parents=True, exist_ok=True)
    with system.file_lock(path.with_suffix(".lock")):
        history = load(path)
        if reuse and (already := through_commit(history, commit)):
            return already
        if current_commit() != commit:
            raise ValueError(f"HEAD moved while measuring {commit}")
        time = datetime.now(UTC)
        pending = tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=path.parent, delete=False
        )
        temporary = Path(pending.name)
        try:
            with pending:
                if path.exists():
                    with path.open(encoding="utf-8") as existing:
                        for line in existing:
                            pending.write(line)
                for board, counts in sorted(boards.items()):
                    row = {
                        "commit": commit,
                        "time": time.isoformat(),
                        "board": board,
                        "counts": counts,
                    }
                    pending.write(json.dumps(row, sort_keys=True) + "\n")
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    return [*history, Measurement(commit, time, boards)]


def rises(
    newest: Measurement, baseline: Measurement, limit: float, period: str
) -> list[str]:
    """Compare only counters present in both samples. A nonzero count after zero is a rise."""
    lines: list[str] = []
    for board in sorted(newest.boards.keys() & baseline.boards.keys()):
        before, after = baseline.boards[board], newest.boards[board]
        for counter in sorted(before.keys() & after.keys()):
            old, new = before[counter], after[counter]
            increase = Decimal(str(new)) - Decimal(str(old))
            allowed = Decimal(str(old)) * Decimal(str(limit)) / 100
            if increase > allowed:
                lines.append(
                    f"{board} {counter} rose from {old:,} ({baseline.commit}) "
                    f"to {new:,} ({newest.commit}), over the {limit:g}% {period} threshold"
                )
    return lines


def regressions(history: list[Measurement], limits: gate.TrendLimits) -> list[str]:
    if len(history) < 2:
        return []
    newest, previous = history[-1], history[-2]
    oldest = next(
        (
            entry
            for entry in history[:-1]
            if newest.time - WEEK <= entry.time <= newest.time
        ),
        newest,
    )
    lines = rises(newest, previous, limits["adjacent_percent"], "previous measurement")
    if oldest != newest and (
        oldest != previous or limits["weekly_percent"] < limits["adjacent_percent"]
    ):
        lines += rises(newest, oldest, limits["weekly_percent"], "seven-day")
    return lines


def with_instructions(native: Boards, instructions: Boards) -> Boards:
    """Keep the native board set even when the optional run omitted some boards."""
    return {
        board: counts | instructions.get(board, {}) for board, counts in native.items()
    }


def gate_output(*args: str, optional: bool = False) -> Boards:
    command = ["just", "b3d", "gate", *args, "--json"]
    done = subprocess.run(
        command, cwd=repo_paths.REPO, stdout=subprocess.PIPE, text=True, check=False
    )
    # The gate prints its measurements even when a budget failed.
    lines = done.stdout.splitlines()
    if done.returncode in (0, 1) and lines:
        return json.loads(lines[-1])["boards"]
    if optional:
        return {}
    raise ValueError(
        f"{shlex.join(command)} produced no measurements (exit {done.returncode})"
    )


def collect_gates() -> Boards:
    native = gate_output()
    instructions = gate_output("--counts", "--if-free", optional=True)
    return with_instructions(native, instructions)


def show(history: list[Measurement], show_all: bool) -> None:
    rows = [
        (entry.time.isoformat(timespec="seconds"), entry.commit, board, counts)
        for entry in history
        for board, counts in sorted(entry.boards.items())
    ]
    hidden = 0 if show_all else max(0, len(rows) - OUTPUT_LIMIT)
    if hidden:
        print(
            f"{hidden} earlier board rows hidden. Run `just b3d trend --all` to see them."
        )
    if not rows:
        print("No measurements. Run `just b3d trend record` to start the series.")
        return
    print(f"{'Time (UTC)':25}  {'Commit':40}  {'Board':24}  Counts")
    for time, commit, board, counts in rows[hidden:]:
        summary = ", ".join(
            f"{name}={value:,}" for name, value in sorted(counts.items())
        )
        print(f"{time:25}  {commit:40}  {board:24}  {summary}")


def run(args: argparse.Namespace) -> int:
    path = series_path()
    try:
        if args.operation == "record":
            commit = current_commit()
            newest = record(path, commit, collect_gates)[-1]
            print(f"Recorded {len(newest.boards)} boards for {commit} in {path}")
        elif args.operation == "check":
            lines = regressions(load(path), gate.read_budgets()["trend"])
            if lines:
                print("\n".join(lines if args.all else lines[:OUTPUT_LIMIT]))
                if not args.all and len(lines) > OUTPUT_LIMIT:
                    print(
                        f"{len(lines) - OUTPUT_LIMIT} more findings. Run `just b3d trend check --all`."
                    )
                return 1
        else:
            show(load(path), args.all)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"trend: {error}", file=sys.stderr)
        return 2
    return 0


def arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("operation", choices=("record", "check"), nargs="?")
    parser.add_argument("--all", action="store_true", help="show all rows or findings")


COMMANDS = (
    Command(
        "trend",
        "records, displays and checks the frame counts by board and commit",
        (
            ("just b3d trend record", "record the gated boards for the current commit"),
            (
                "just b3d trend check",
                "report frame counts that rose past a trend threshold",
            ),
        ),
        run,
        arguments,
    ),
)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=COMMANDS[0].summary)
    arguments(parser)
    sys.exit(run(parser.parse_args()))
