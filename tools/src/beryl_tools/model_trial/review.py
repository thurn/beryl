"""`model-trial review`: run the blind reviewer on every graded pair that is not void, as many
times as the spec's `reviewer.runs_per_pair`, with A and B swapped on each run, and append one
row per run to `reviews.jsonl`.

The reviewer is a headless `claude -p` run, built the way Ignite builds a worker's command
(`Claude::command` in `ignite/crates/ignite_core/src/cli/claude.rs`), with the model ID that
the spec's `reviewer.model` names in `[pool.models.claude]` of `ignite.toml`. It works in a
fresh directory under the cache that holds what `blind.py` lets it see: the ticket text, each
side's files at its final commit, its diff, its closing note and images, under the names A and
B. It may read files and may not run commands. The directory is removed after each run.

A run whose answer is not the JSON the prompt asks for is run once more and then recorded as
failed. Every run's `total_cost_usd` counts toward the trial's cap, and `review` starts no run
once the cap is spent.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import re
import secrets
import shutil
import string
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import paths
from . import blind, grade, io, manifest
from .blind import Filed, Side
from .manifest import Grade, Pair, Review
from .spec import Spec

PROMPT_FILE: Path = Path(__file__).with_name("reviewer_prompt.txt")
# The rubric's lines, by the key the reviewer answers under, with the name the report prints.
RUBRIC: dict[str, str] = {
    "correct": "Correct",
    "fits": "Fits the codebase",
    "size": "Size",
    "tests": "Tests",
    "prose": "Prose",
    "questioned": "Questioned the ticket",
    "fixed_or_filed": "Fixed or filed what it found",
    "verified": "Verified beyond the acceptance",
    "pushed": "Pushed through obstacles",
    "overall": "Overall",
}
PICKS = ("A", "B", "tie")
WHERE: re.Pattern[str] = re.compile(r"\S+:[0-9]+")
ARMS = ("source", "copy")
# The tools the reviewer is denied: it reads and searches, and never runs or writes anything.
DENIED = "Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch,Task,Agent"

Runner = Callable[[list[str], Path], str]
Snapshot = Callable[[str, Path], None]


def prompt_sections() -> tuple[str, dict[str, str]]:
    """The prompt file's main text, and its optional paragraphs by name (`--- name ---`)."""
    parts = re.split(
        r"^--- (\w+) ---$", PROMPT_FILE.read_text(encoding="utf-8"), flags=re.M
    )
    main, rest = parts[0], parts[1:]
    return main.rstrip() + "\n", {
        rest[i]: rest[i + 1].strip() + "\n" for i in range(0, len(rest) - 1, 2)
    }


def rubric_version() -> str:
    """A hash of the prompt file: any change to the prompt is a new rubric version."""
    return hashlib.sha256(PROMPT_FILE.read_bytes()).hexdigest()[:12]


def prompt(hub: bool, plant: dict[str, str] | None) -> str:
    main, optional = prompt_sections()
    text = main
    if hub:
        text += "\n" + optional["hub"]
    if plant is not None:
        text += "\n" + string.Template(optional["planted"]).safe_substitute(
            planted=plant["replace"], truth=plant["find"], expect=plant["expect"]
        )
    return text


def reviewer_argv(program: str, model: str, effort: str, text: str) -> list[str]:
    """The reviewer's command, in the order `Claude::command` builds a worker's: `-p`, the
    tool rules, `--model`, `--effort`, the output format, then the prompt last."""
    return [
        program,
        "-p",
        "--disallowedTools",
        DENIED,
        "--model",
        model,
        "--effort",
        effort,
        "--output-format",
        "stream-json",
        "--verbose",
        text,
    ]


def a_arm(seed: int, run: int) -> str:
    """The arm shown as A on run `run`: drawn from `seed` for the first run, and swapped on
    each run after it."""
    first = random.Random(seed).choice(ARMS)
    return first if run % 2 == 0 else ARMS[1 - ARMS.index(first)]


# The reviewer's answer.


def read_stream(stream: str) -> tuple[str | None, float, bool]:
    """The `result` event's text, its `total_cost_usd`, and whether it is an error."""
    text: str | None = None
    cost = 0.0
    error = False
    for line in stream.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict) and event.get("type") == "result":
            found = event.get("result")
            text = found if isinstance(found, str) else None
            cost = float(event.get("total_cost_usd") or 0.0)
            error = bool(event.get("is_error"))
    return text, cost, error


def decode(text: str) -> object:
    """The JSON object in the reviewer's answer, which may sit in a code fence or after a
    sentence."""
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end < start:
        raise ValueError("the answer holds no JSON object")
    return json.loads(text[start : end + 1])


def problems(answer: object, planted: bool) -> list[str]:
    """What keeps `answer` from being the JSON the prompt asks for."""
    if not isinstance(answer, dict):
        return ["the answer is not a JSON object"]
    found: list[str] = []
    rubric = answer.get("rubric")
    if not isinstance(rubric, dict):
        found.append("it has no `rubric` object")
    else:
        for key in RUBRIC:
            line = rubric.get(key)
            if (
                not isinstance(line, dict)
                or line.get("pick") not in PICKS
                or not isinstance(line.get("evidence"), str)
            ):
                found.append(f"`rubric.{key}` needs a pick of A, B or tie and evidence")
    defects = answer.get("defects", [])
    if not isinstance(defects, list) or not all(
        isinstance(d, dict)
        and d.get("side") in ("A", "B")
        and WHERE.search(str(d.get("where") or ""))
        and str(d.get("input") or "").strip()
        for d in defects
    ):
        found.append("each defect needs a side, a `file:line` and an input")
    shared = answer.get("shared")
    if shared != "no" and not (
        isinstance(shared, list) and shared and all(isinstance(s, dict) for s in shared)
    ):
        found.append('`shared` must be "no" or a list of cases')
    if planted:
        marked = answer.get("planted")
        if not isinstance(marked, dict) or not all(
            isinstance(marked.get(side), bool) for side in ("A", "B")
        ):
            found.append("`planted` needs true or false for A and for B")
    return found


# Reading a review row in arm terms.


def arm_of(review: Review, side: str) -> str:
    return blind.labels(review.a_arm)[side]


def picks(review: Review) -> dict[str, str]:
    """Each rubric line's pick as `source`, `copy` or `tie`."""
    rubric = review.answer.get("rubric") or {}
    found: dict[str, str] = {}
    for key in RUBRIC:
        pick = str((rubric.get(key) or {}).get("pick") or "tie")
        found[key] = arm_of(review, pick) if pick in ("A", "B") else "tie"
    return found


def defects(review: Review) -> dict[str, int]:
    found = {arm: 0 for arm in ARMS}
    for d in review.answer.get("defects") or []:
        found[arm_of(review, str(d["side"]))] += 1
    return found


def shared(review: Review) -> list[dict[str, Any]]:
    """The code or wording the reviewer found both sides share, or nothing."""
    cases = review.answer.get("shared")
    return list(cases) if isinstance(cases, list) else []


def planted(review: Review) -> dict[str, bool] | None:
    marked = review.answer.get("planted")
    if not isinstance(marked, dict):
        return None
    return {arm_of(review, side): bool(marked.get(side)) for side in ("A", "B")}


# Running it.


@dataclass
class Material:
    """What a pair's reviews show, by arm. `commits` holds each arm's final commit, or None
    for an arm that finished with no change."""

    ticket: str
    sides: dict[str, Side]
    commits: dict[str, str | None]
    ticket_ids: list[str]
    hub: bool
    plant: dict[str, str] | None


def write_material(
    material: Material, first: str, folder: Path, snapshot: Snapshot
) -> None:
    """Lays out `material` in `folder` with `first` as side A."""
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "ticket.md").write_text(material.ticket, encoding="utf-8")
    for label, arm in blind.labels(first).items():
        side = material.sides[arm]
        diff = side.diff
        if diff is not None:
            (folder / f"{label}.diff").write_text(diff, encoding="utf-8")
        md = blind.side_markdown(label, side, material.ticket_ids)
        (folder / f"{label}.md").write_text(md, encoding="utf-8")
        if side.images:
            images = folder / f"{label}-images"
            images.mkdir(exist_ok=True)
            for path, name in zip(side.images, blind.image_names(side.images)):
                shutil.copyfile(path, images / name)
        commit = material.commits[arm]
        if commit is not None:
            snapshot(commit, folder / label)


def review_run(
    material: Material,
    pair: Pair,
    run: int,
    seed: int,
    model: str,
    effort: str,
    program: str,
    folder: Path,
    runner: Runner,
    snapshot: Snapshot,
) -> Review:
    """One reviewer run, retried once on an answer that is not the asked-for JSON."""
    first = a_arm(seed, run)
    text = prompt(material.hub, material.plant)
    row = Review(
        pair=pair.pair,
        trial=pair.trial,
        run=run,
        seed=seed,
        a_arm=first,
        reviewer_model=model,
        rubric_version=rubric_version(),
        ok=False,
    )
    try:
        write_material(material, first, folder, snapshot)
        for _ in range(2):
            row.attempts += 1
            result, cost, is_error = read_stream(
                runner(reviewer_argv(program, model, effort, text), folder)
            )
            row.cost_usd = round(row.cost_usd + cost, 4)
            try:
                if result is None or is_error:
                    raise ValueError("the reviewer gave no result")
                answer = decode(result)
            except ValueError as error:
                row.error = str(error)
                continue
            found = problems(answer, material.plant is not None)
            if found:
                row.error = "; ".join(found)
                continue
            assert isinstance(answer, dict)
            row.ok, row.answer, row.error = True, answer, None
            break
    finally:
        shutil.rmtree(folder, ignore_errors=True)
    return row


def gather(pair: Pair, arms: dict[str, Grade]) -> Material:
    """Reads what a pair's reviews show from the tickets, git and the grading directory."""
    copy_id = str(pair.copy)
    tickets = io.show([pair.source, copy_id], history=True, comments=True)
    by_id = {str(t["id"]): t for t in tickets}
    elsewhere = sorted(
        {c["ticket"] for g in arms.values() for c in g.comments}
        | {t for g in arms.values() for t in g.filed_tickets}
    )
    others = {str(t["id"]): t for t in io.show(elsewhere, comments=True)}
    source = by_id[pair.source]
    ids = [pair.source, copy_id, pair.trial, str(source.get("parent") or "")]
    ids += [t for g in arms.values() for t in g.filed_tickets]
    sides: dict[str, Side] = {}
    commits: dict[str, str | None] = {}
    folder = manifest.trial_dir(pair.trial) / pair.pair
    for arm, g in arms.items():
        ticket = by_id[g.ticket]
        claim = grade.first_claim(ticket)
        session = claim.session if claim else ""
        own = [
            str(c.get("text") or "")
            for t in (ticket, *(others[c["ticket"]] for c in g.comments))
            for c in t.get("comments") or []
            if str(c.get("author")) == session
        ]
        has_change = bool(g.start_commit and g.end_commit)
        sides[arm] = Side(
            diff=(
                io.diff_text(str(g.start_commit), str(g.end_commit))
                if has_change
                else None
            ),
            note=str(ticket.get("close_reason") or "") if g.finished else "",
            comments=list(dict.fromkeys(own)),
            filed=[
                Filed(
                    str(others[t].get("title") or ""), str(others[t].get("body") or "")
                )
                for t in g.filed_tickets
                if t in others
            ],
            check=g.check,
            images=[folder / arm / name for name in g.attachments],
        )
        commits[arm] = g.end_commit if has_change else None
    copy = by_id[copy_id]
    return Material(
        ticket=blind.ticket_markdown(
            str(copy.get("title") or ""),
            str(copy.get("body") or ""),
            str(copy.get("acceptance") or ""),
            ids,
        ),
        sides=sides,
        commits=commits,
        ticket_ids=ids,
        hub="hub" in (source.get("labels") or []),
        plant=pair.plant,
    )


def spent(grades: list[Grade], reviews: list[Review]) -> float:
    """What the cap counts: the copies' sessions and the reviewer runs."""
    latest = manifest.latest_grades(grades)
    copies = sum((float(arms["copy"].dollars or 0.0) for arms in latest.values()), 0.0)
    return copies + sum((r.cost_usd for r in reviews), 0.0)


def is_void(pair: Pair, arms: dict[str, Grade]) -> bool:
    decided = (pair.contamination or {}).get("decision") == "void"
    return bool(arms["source"].void) or decided


def cmd_review(
    args: argparse.Namespace,
    spec: Spec,
    runner: Runner = io.run_reviewer,
    snapshot: Snapshot = io.snapshot,
) -> int:
    trial: str = args.trial
    pairs = {p.pair: p for p in manifest.read_pairs(trial)}
    for ticket in getattr(args, "redo", None) or []:
        # A pair is named by its copy's ID, which is unique; a source has a copy per arm.
        token = next((p.pair for p in pairs.values() if p.copy == ticket), None)
        if token is None:
            print(f"{ticket} is no copy of this trial.", file=sys.stderr)
            return 2
        moved = manifest.retire_reviews(trial, token)
        print(f"{ticket}: retired {moved} review runs, to run again.")
    grades = manifest.read_grades(trial)
    reviews = manifest.read_reviews(trial)
    model = io.model_ids("claude").get(spec.reviewer.model, spec.reviewer.model)
    program = io.claude_program()
    todo = 0
    for token, arms in manifest.latest_grades(grades).items():
        pair = pairs.get(token)
        if pair is None or is_void(pair, arms):
            continue
        done = [r for r in reviews if r.pair == token]
        runs = [
            n
            for n in range(spec.reviewer.runs_per_pair)
            if n not in {r.run for r in done}
        ]
        if not runs:
            continue
        todo += 1
        seed = done[0].seed if done else secrets.randbits(32)
        material = gather(pair, arms)
        for run in runs:
            if spent(grades, reviews) >= spec.cap_usd:
                print(f"The ${spec.cap_usd:.0f} cap is spent: no more reviews.")
                return 0
            folder = paths.cache_root() / "blind-review" / secrets.token_hex(6)
            row = review_run(
                material,
                pair,
                run,
                seed,
                model,
                spec.reviewer.effort,
                program,
                folder,
                runner,
                snapshot,
            )
            manifest.append_review(row)
            reviews.append(row)
            verdict = "ok" if row.ok else f"failed: {row.error}"
            print(
                f"{pair.source} / {pair.copy} run {run}: {verdict}, ${row.cost_usd:.2f}"
            )
            for case in shared(row):
                print(f"  suspect: both sides share {case.get('what', case)}")
    if not todo:
        print("No graded pair waits for a review.")
    return 0
