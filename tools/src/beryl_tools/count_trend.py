"""The instruction-count gate's trend: small rises file beads while they are fresh.

The gate (`bench.count_gate`, slow CI's `counts` lane) fails only past its tolerance, and its
bar is the total since the bar last moved. Between 2026-11-11 and 2026-11-13 the count rose
from 386.0M to 393.2M (+1.85%) in steps of 0.05% to 0.64%, with nothing flagging any of them,
until one more commit failed the gate and had to win back the creep along with its own cost.

Slow CI already measures each staging tip it tests, so watching the trend costs nothing: each
count it measures on a staging commit goes in [`history_path`], and `report`

- prints the change since the previous measured tip, naming the commits between them;
- files one "count creep" bead ([`CREEP_LABEL`]) when one tested range adds more than
  [`STEP_PERCENT`], or the count sits more than [`DRIFT_PERCENT`] above the lowest it reached
  since the bar last moved, so creep made of steps too small to flag is still caught;
- files one "lower the bar" bead ([`LOWER_LABEL`]) when the count is more than
  [`LOWER_PERCENT`] below the bar, so a gain isn't left as slack for later rises to use up.

While a bead of either kind is open, a later finding of that kind is added to it as a comment.
Nothing here fails a run: the gate's own verdict does that.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from . import paths, verdicts

# One tested range adding more than this files a creep bead. Unrelated changes move the count
# by inlining and layout drift of up to about 0.05%; the steps that added up to the 2026-11-13
# failure were 0.18% to 0.64% each.
STEP_PERCENT = 0.25
# The count more than this above the lowest it reached since the bar last moved files a creep
# bead, catching steps that are each under STEP_PERCENT.
DRIFT_PERCENT = 0.5
# The count more than this below the bar files a bead to lower it.
LOWER_PERCENT = 0.5

CREEP_LABEL = "count-creep"
LOWER_LABEL = "count-lower"
PRIORITY = "2"


def history_path() -> Path:
    return paths.cache_root() / "ci" / "counts.jsonl"


@dataclass(frozen=True)
class Record:
    commit: str
    instructions: int
    bar: int


def load(text: str) -> list[Record]:
    records: list[Record] = []
    for line in text.splitlines():
        try:
            d = json.loads(line)
            records.append(
                Record(str(d["commit"]), int(d["instructions"]), int(d["bar"]))
            )
        except (ValueError, KeyError, TypeError):
            continue
    return records


def dump(record: Record) -> str:
    return json.dumps(
        {
            "commit": record.commit,
            "instructions": record.instructions,
            "bar": record.bar,
        }
    )


def percent(new: int, old: int) -> float:
    return 100.0 * (new - old) / old


@dataclass(frozen=True)
class Findings:
    previous: Record | None  # the latest earlier measurement of an ancestor commit
    floor: int | None  # the lowest count measured under the same bar, before this one
    step: float | None  # percent change since `previous`
    creep: str | None  # why a creep bead is due, if one is
    lower: str | None  # why a lower-the-bar bead is due, if one is


def judge(
    records: list[Record],
    measured: int,
    bar: int,
    is_ancestor: Callable[[str], bool],
) -> Findings:
    """What `measured` says against the earlier measurements in `records`. `is_ancestor`
    says whether a commit is a proper ancestor of the one measured, so a measurement of an
    unrelated branch or of the same commit again is not compared with."""
    previous = next((r for r in reversed(records) if is_ancestor(r.commit)), None)
    same_bar = [r.instructions for r in records if r.bar == bar]
    floor = min(same_bar) if same_bar else None
    step = percent(measured, previous.instructions) if previous else None
    creep = None
    if previous is not None and step is not None and step > STEP_PERCENT:
        creep = (
            f"{step:+.2f}% since {previous.commit[:12]} "
            f"({previous.instructions:,} to {measured:,}), over the {STEP_PERCENT}% a "
            f"single tested range may add"
        )
    elif floor is not None and percent(measured, floor) > DRIFT_PERCENT:
        creep = (
            f"{percent(measured, floor):+.2f}% above {floor:,}, the lowest count since "
            f"the bar was set to {bar:,}: over the {DRIFT_PERCENT}% drift allowed"
        )
    lower = None
    if percent(measured, bar) < -LOWER_PERCENT:
        lower = (
            f"{measured:,} instructions, {percent(measured, bar):+.2f}% against the bar of "
            f"{bar:,}"
        )
    return Findings(previous, floor, step, creep, lower)


def git(*args: str) -> tuple[int, str]:
    run = subprocess.run(
        ["git", *args], cwd=paths.REPO, capture_output=True, text=True, check=False
    )
    return run.returncode, run.stdout.strip()


def on_staging() -> str | None:
    """HEAD's full hash, when it is a clean, detached commit already on origin/staging, as
    slow CI's checkouts are; None for a branch or a working tree with changes, whose
    counts say nothing about staging."""
    code, branch = git("symbolic-ref", "-q", "HEAD")
    if code == 0 and branch:
        return None
    _, dirty = git("status", "--porcelain", "--untracked-files=no")
    if dirty:
        return None
    code, _ = git("merge-base", "--is-ancestor", "HEAD", "origin/staging")
    if code != 0:
        return None
    _, head = git("rev-parse", "HEAD")
    return head or None


def proper_ancestor(head: str) -> Callable[[str], bool]:
    def check(commit: str) -> bool:
        if commit == head:
            return False
        code, _ = git("merge-base", "--is-ancestor", commit, head)
        return code == 0

    return check


def rust_commits(old: str, new: str) -> list[str]:
    _, out = git("log", "--format=%h %s", f"{old}..{new}", "--", "rust/")
    return out.splitlines()


def open_bead(label: str, store: verdicts.Beads) -> str | None:
    _, out = store.bd("list", "--label", label, "-n", "0", "--json")
    beads = verdicts.parse_json(out)
    for b in beads if isinstance(beads, list) else []:
        if isinstance(b, dict) and b.get("status") != "closed":
            return str(b.get("id"))
    return None


def creep_body(head: str, why: str, f: Findings) -> str:
    commits = rust_commits(f.previous.commit, head) if f.previous else []
    listed = "\n".join(f"- {c}" for c in commits) or "- (none under rust/)"
    since = f.previous.commit[:12] if f.previous else "the last measured tip"
    return (
        f"Slow CI's count gate measured the rollout benchmark at {head[:12]}: {why}. The "
        f"gate itself still passed (tools/src/beryl_tools/count_trend.py).\n\n"
        f"Commits under rust/ since {since}:\n{listed}\n\n"
        f"Find what added the instructions (`just bench-count` on both commits, then "
        f"`callgrind_annotate` on the two `callgrind.rollouts.corpus.out` files in the "
        f"`beryl-bench` VM) and win it back: keep paths these decks never take `#[cold]` "
        f"and out of line, and avoid compiling shared code twice. If the work itself "
        f"changed (`rollouts_never_change` was updated), say so here and close this bead. "
        f"The rules for moving the bar are in rust/crates/beryl_bench/count-gate.toml."
    )


def lower_body(head: str, why: str) -> str:
    return (
        f"Slow CI's count gate measured the rollout benchmark at {head[:12]}: {why}. Set "
        f"`instructions` in rust/crates/beryl_bench/count-gate.toml to the count measured "
        f"on current staging (`just count-gate`), so the gain isn't left as slack for later "
        f"rises. The commit body names the old and new counts."
    )


def file_or_comment(
    label: str,
    title: str,
    body: str,
    tier: str,
    store: verdicts.Beads,
) -> str:
    known = open_bead(label, store)
    if known:
        code, _ = store.bd("comments", "add", known, body)
        return f"commented on {known}" if code == 0 else f"{known} is open"
    code, out = store.bd(
        "create",
        "--title",
        title,
        "--description",
        body,
        "--priority",
        PRIORITY,
        "--labels",
        f"{label},{tier}",
        "--silent",
    )
    return f"filed {out.strip()}" if code == 0 else "bead not filed"


def report(
    measured: int, bar: int, passed: bool, store: verdicts.Beads | None = None
) -> list[str]:
    """Records a staging tip's count and files the beads its trend calls for; returns lines
    to print. A count measured elsewhere is only compared, never recorded or filed on.
    """
    store = store or verdicts.STORE
    head = on_staging()
    path = history_path()
    try:
        records = load(path.read_text()) if path.exists() else []
    except OSError:
        records = []
    if head is None:
        _, local = git("rev-parse", "HEAD")
        f = judge(records, measured, bar, proper_ancestor(local))
        return [step_line(f)] if f.previous else []
    f = judge(records, measured, bar, proper_ancestor(head))
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a") as out:
            out.write(dump(Record(head, measured, bar)) + "\n")
    except OSError as e:
        verdicts.warn(f"count history: {e}")
    lines = [step_line(f)] if f.previous else []
    # A failing gate files a Steward bead of its own.
    if passed and f.creep:
        said = file_or_comment(
            CREEP_LABEL,
            "Win back the rollout benchmark's creeping instruction count",
            creep_body(head, f.creep, f),
            "tier:opus",
            store,
        )
        lines.append(f"count creep: {f.creep}: {said}")
    if f.lower:
        said = file_or_comment(
            LOWER_LABEL,
            "Lower the count gate's bar",
            lower_body(head, f.lower),
            "tier:light",
            store,
        )
        lines.append(f"count gate: {f.lower}: {said}")
    return lines


def step_line(f: Findings) -> str:
    assert f.previous is not None and f.step is not None
    return (
        f"{f.step:+.2f}% since {f.previous.commit[:12]}, the last staging tip measured "
        f"({f.previous.instructions:,})"
    )
