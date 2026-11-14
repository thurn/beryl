"""`ui-counts`: the work the UI does to play recorded fixtures, counted in units that do not
depend on the machine's speed or load (ui/scripts/counts.mjs): React components rendered, DOM
mutations, and Chromium's style and layout work. Timing frames (`ui-perf`) measures this
machine, which is much faster than the laptops Beryl targets and is never quiet; counts measure
the tree.

`by ui-counts <fixtures>` prints the counts. `by ui-counts --gate`, slow CI's `ui-counts` lane,
plays the gated fixtures and fails when a count rises more than the tolerance in
`data/ui-counts.json` above its limit there. `--gate --update` writes the measured counts as
the new limits.

The counts repeat closely but not exactly from run to run: some events (scroll events among
them) reach the page only in the browser's real rendering steps, which can move a React update
into a separate commit. Across repeated runs, renders and mutations vary by about 0.1% and the
style and layout work by up to 1.5%, so the tolerance sits above that noise. The limits were
measured on macOS. Windows leaves the lane to the Mac, like the other browser lanes, because
its text measures differently and would need limits of its own.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

from . import checks, paths
from .shots import UiError, browser_env, record_fixtures, served_ui

GATE_FILE: Path = paths.REPO / "data" / "ui-counts.json"
# The metrics the gate holds. `commits`, `styles` and `layouts` count passes, not the work
# done in them, and are printed only.
GATED_METRICS: tuple[str, ...] = ("renders", "mutations", "styled", "laidOut")


@dataclass
class Gate:
    fixtures: list[str]
    layouts: list[str]
    tolerance_percent: float
    limits: dict[str, dict[str, int]]  # "<fixture>/<layout>" -> metric -> count


def load_gate(text: str) -> Gate:
    data = json.loads(text)
    return Gate(
        fixtures=[str(f) for f in data["fixtures"]],
        layouts=[str(l) for l in data["layouts"]],
        tolerance_percent=float(data["tolerance_percent"]),
        limits={
            str(k): {str(m): int(n) for m, n in v.items()}
            for k, v in data["limits"].items()
        },
    )


def updated(text: str, measured: dict[str, dict[str, int]]) -> str:
    """The gate file's text with `measured`'s gated metrics as its limits."""
    data = json.loads(text)
    data["limits"] = {
        key: {m: counts[m] for m in GATED_METRICS} for key, counts in measured.items()
    }
    return json.dumps(data, indent=2) + "\n"


def judge(gate: Gate, measured: dict[str, dict[str, int]]) -> tuple[bool, list[str]]:
    """Fails a count that rose past the tolerance above its limit, or a play with no limit or
    no count; a count that fell past it passes, asking for lower limits."""
    passed = True
    lines: list[str] = []
    lower = False
    for key in sorted(set(gate.limits) | set(measured)):
        limits = gate.limits.get(key)
        counts = measured.get(key)
        if limits is None or counts is None:
            passed = False
            lines.append(
                f"{key}: {'no limits in' if limits is None else 'not measured, but limited in'}"
                f" {GATE_FILE.name}"
            )
            continue
        for metric in GATED_METRICS:
            limit, count = limits[metric], counts[metric]
            change = 100.0 * (count - limit) / limit if limit else 0.0
            said = f"{key} {metric}: {count:,} against {limit:,} ({change:+.1f}%)"
            if change > gate.tolerance_percent:
                passed = False
                lines.append(f"{said}: over the {gate.tolerance_percent}% tolerance")
            elif change < -gate.tolerance_percent:
                lower = True
                lines.append(f"{said}: under the limit")
            else:
                lines.append(said)
    if not passed:
        lines.append(
            "Find the extra work with `just ui-counts <fixture> --components`. Raise a limit "
            "(`just ui-counts --gate --update`) only when the change needs the extra work, "
            "and say why in the commit's body."
        )
    elif lower:
        lines.append(
            "Run `just ui-counts --gate --update` and commit the lower limits to keep the gain."
        )
    return passed, lines


def measure(
    fixtures: list[str], layouts: list[str], components: bool
) -> int | dict[str, dict[str, int]]:
    """Plays `fixtures` in `layouts` and returns their counts, or an exit code when the play
    failed or could not run."""
    recorded = paths.cache_root() / "ui-counts" / paths.REPO.name
    try:
        env = browser_env("chromium")
        if record_fixtures(recorded, fixtures):
            return checks.CANNOT_RUN
        with tempfile.TemporaryDirectory() as tmp, served_ui(env) as url:
            out = Path(tmp) / "counts.json"
            code = subprocess.run(
                ["node", "scripts/counts.mjs", url, str(recorded)]
                + ["--json", str(out), "--layouts", ",".join(layouts)]
                + (["--components"] if components else [])
                + fixtures,
                cwd=paths.UI,
                env=env,
                stdin=subprocess.DEVNULL,
                check=False,
            ).returncode
            if code != 0:
                return code
            return json.loads(out.read_text())
    except UiError as e:
        print(f"ui-counts: {e}", file=sys.stderr)
        return checks.CANNOT_RUN


def ui_counts(args: argparse.Namespace) -> int:
    if not args.gate:
        layouts = ["desktop", "phone"] if args.phone else ["desktop"]
        if not args.fixtures:
            print("ui-counts: name fixtures, or pass --gate", file=sys.stderr)
            return 2
        result = measure(list(args.fixtures), layouts, args.components)
        return result if isinstance(result, int) else 0
    text = GATE_FILE.read_text()
    gate = load_gate(text)
    result = measure(gate.fixtures, gate.layouts, args.components)
    if isinstance(result, int):
        return result
    if args.update:
        GATE_FILE.write_text(updated(text, result))
        print(f"ui-counts: wrote the measured counts to {GATE_FILE.name}")
        return 0
    passed, lines = judge(gate, result)
    for line in lines:
        print(f"ui-counts: {line}")
    return 0 if passed else 1


def lane() -> checks.Lane:
    """Holds the gated fixtures' counts to `data/ui-counts.json` (`by ui-counts --gate`)."""
    from .reruns import whole

    return checks.Lane("ui-counts", warm=True).step(
        "ui-counts", [*checks.BY, "ui-counts", "--gate"], rerun=whole
    )


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "ui-counts",
        help="count the renders, DOM mutations and style and layout work of fixtures",
    )
    p.add_argument("fixtures", nargs="*", help="fixtures to play, by name")
    p.add_argument("--phone", action="store_true", help="the phone's layout too")
    p.add_argument(
        "--components",
        action="store_true",
        help="list the components that rendered most",
    )
    p.add_argument(
        "--gate",
        action="store_true",
        help="play the fixtures in data/ui-counts.json and fail a rise past its limits",
    )
    p.add_argument(
        "--update",
        action="store_true",
        help="with --gate, write the counts as the limits",
    )
    p.set_defaults(func=ui_counts)
