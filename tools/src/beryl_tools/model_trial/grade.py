"""`model-trial grade`: measure every filed pair whose two arms have both ended, one row per arm
in `grades.jsonl`, with the pair's void reasons and contamination evidence on both rows.

Where each measure comes from:

- How an arm ended, its session and its first landing: the ticket's history
  (`ignite ticket show --history`). The source arm is the source's first claim, graded on the
  commits of its first landing (`landed.commits`, which `ignite land` writes to the ticket):
  its diff runs from the parent of the first of them to the last. A later session that finishes
  the ticket after the first was released does not count.
- The copy's diff: the branch `trial/<copy id>` in the main checkout, against the recorded
  start commit, which must be where the branch meets `origin/staging`.
- Dollars, tokens and minutes: `just efficiency ticket <id> --json`.
- Check verdicts: the efficiency store's `lane_runs` table (from the hub's `records`), by
  commit: the `check` runs of the copy's branch head and the `land` runs of the source's
  landed commit.
- Commands: the store's `tool_calls` table (each command's first 300 characters). Output and
  messages: the session's Claude transcript, whose path the store's `sources` table keeps,
  read for the ticket's windows when the transcript is on this machine.
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from collections.abc import Sequence
from datetime import datetime
from pathlib import Path
from typing import Any

from . import candidates, io, manifest, scan
from .manifest import FILED, Grade, Pair
from .scan import Trace
from .spec import Arm, Spec

BEAD: re.Pattern[str] = re.compile(r"[A-Za-z0-9.\-]+")
SHA: re.Pattern[str] = re.compile(r"[0-9a-f]{7,64}")
TEST_PATH: re.Pattern[str] = re.compile(
    r"(^|/)(tests?/|test_[^/]*$|[^/]*_tests?\.(rs|py)$|[^/]*_test\.py$)"
)


@dataclass
class Claim:
    """A ticket's first claim: who took it, when, how it ended and what it first landed."""

    session: str
    claimed_at: str
    ended: str | None = None  # landed, closed or released; None while it runs
    ended_at: str | None = None
    landed: list[str] | None = None


def first_claim(ticket: dict[str, Any]) -> Claim | None:
    claim: Claim | None = None
    for entry in sorted(ticket.get("history") or [], key=lambda h: h.get("seq", 0)):
        change = entry.get("change") or {}
        at = str(entry.get("at"))
        if claim is None:
            assignee = change.get("assignee")
            if assignee and assignee[1]:
                claim = Claim(session=str(assignee[1]), claimed_at=at)
            continue
        landed = change.get("landed")
        if landed and landed[1] and claim.landed is None:
            claim.landed = [str(c) for c in landed[1].get("commits", [])]
        status = change.get("status")
        if status and status[1] == "closed":
            claim.ended, claim.ended_at = ("landed" if claim.landed else "closed"), at
            break
        assignee = change.get("assignee")
        if assignee and assignee[1] != claim.session:
            claim.ended, claim.ended_at = "released", at
            break
    return claim


def minutes_between(start: str, end: str) -> float:
    def parse(text: str) -> datetime:
        return datetime.fromisoformat(text.replace("Z", "+00:00"))

    return round((parse(end) - parse(start)).total_seconds() / 60, 1)


@dataclass
class Diff:
    files: list[str]
    added: int
    removed: int
    test_added: int
    added_lines: list[str]


def diff(start: str, end: str) -> Diff:
    files: list[str] = []
    added = removed = test_added = 0
    for line in io.git("diff", "--numstat", start, end).splitlines():
        plus, minus, path = line.split("\t", 2)
        files.append(path)
        if plus != "-":
            added += int(plus)
            removed += int(minus)
            if TEST_PATH.search(path):
                test_added += int(plus)
    lines = [
        l[1:].strip()
        for l in io.git("diff", "--unified=0", "--no-color", start, end).splitlines()
        if l.startswith("+") and not l.startswith("+++")
    ]
    return Diff(files, added, removed, test_added, [l for l in lines if l])


def check_verdict(commit: str | None, tree: str) -> str:
    """`pass`, `fail` or `unknown` from the stored runs of `commit` by the given verb."""
    if not commit or not SHA.fullmatch(commit):
        return "unknown"
    rows = io.sql(
        f"SELECT SUM(fails) AS fails, COUNT(*) AS runs FROM lane_runs "
        f"WHERE \"commit\" = '{commit}' AND tree = '{tree}'"
    )
    if not rows or not rows[0].get("runs"):
        return "unknown"
    return "fail" if rows[0].get("fails") else "pass"


def cost(ticket_id: str) -> dict[str, Any]:
    try:
        return io.efficiency("ticket", ticket_id, "--no-ingest") or {}
    except io.IgniteError:
        return {}


def traces(cost_row: dict[str, Any]) -> list[Trace]:
    """The ticket's commands from the store, and its output and messages from the transcripts
    of its windows that are on this machine."""
    bead = str(cost_row.get("bead") or "")
    if not BEAD.fullmatch(bead):
        return []
    found = [
        Trace("command", str(r["command"] or ""), f"{r['transcript']}:{r['call']}")
        for r in io.sql(
            f"SELECT transcript, call, command FROM tool_calls WHERE bead = '{bead}' "
            "ORDER BY start"
        )
    ]
    for window in cost_row.get("sessions") or []:
        transcript = str(window.get("transcript") or "")
        provider = str(window.get("provider") or "")
        if not BEAD.fullmatch(transcript):
            continue
        # An Antigravity (`agy`) conversation keeps its transcript at
        # `<conversation>/.system_generated/logs/transcript.jsonl`.
        if provider == "claude":
            source, pattern, read = "claude", f"%{transcript}.jsonl", transcript_traces
        elif provider.startswith("agy-"):
            source, pattern, read = "agy", f"%/{transcript}/%", agy_traces
        else:
            continue
        rows = io.sql(
            f"SELECT path FROM sources WHERE source = '{source}' AND path LIKE '{pattern}'"
        )
        for row in rows:
            path = Path(str(row["path"]))
            if path.is_file():
                found += read(
                    path, str(window.get("start") or ""), str(window.get("end") or "~")
                )
    return found


def transcript_traces(path: Path, start: str, end: str) -> list[Trace]:
    """Commands, tool output and the model's own words from a Claude transcript, between
    `start` and `end` (ISO times, compared as text)."""
    found: list[Trace] = []
    for number, line in enumerate(
        path.read_text(encoding="utf-8", errors="replace").splitlines()
    ):
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        at = str(record.get("timestamp") or "")
        if not (start <= at <= end):
            continue
        where = f"{path.name}:{number + 1}"
        content = (record.get("message") or {}).get("content")
        for part in content if isinstance(content, list) else []:
            if not isinstance(part, dict):
                continue
            if part.get("type") == "text" and record.get("type") == "assistant":
                found.append(Trace("message", str(part.get("text") or ""), where))
            elif part.get("type") == "tool_use":
                found.append(
                    Trace("command", json.dumps(part.get("input") or {}), where)
                )
            elif part.get("type") == "tool_result":
                found.append(
                    Trace("output", json.dumps(part.get("content") or ""), where)
                )
    return found


def agy_traces(path: Path, start: str, end: str) -> list[Trace]:
    """Commands, tool output and the model's own words from an Antigravity transcript, between
    `start` and `end`. A `PLANNER_RESPONSE` step holds the model's words and its tool calls,
    and a `GENERIC` step holds a tool's output."""
    found: list[Trace] = []
    for number, line in enumerate(
        path.read_text(encoding="utf-8", errors="replace").splitlines()
    ):
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(record, dict):
            continue
        at = str(record.get("created_at") or "")
        if not (start <= at <= end):
            continue
        where = f"{path.parent.parent.parent.name}:{number + 1}"
        content = record.get("content")
        text = content if isinstance(content, str) else ""
        if record.get("type") == "PLANNER_RESPONSE":
            if text:
                found.append(Trace("message", text, where))
            for call in record.get("tool_calls") or []:
                if isinstance(call, dict):
                    found.append(
                        Trace("command", json.dumps(call.get("args") or {}), where)
                    )
        elif record.get("type") == "GENERIC" and text:
            found.append(Trace("output", text, where))
    return found


def attachments(pair: Pair, ticket_id: str, arm: str, since: str | None) -> list[str]:
    """Downloads the images the arm's session attached into the pair's directory: those
    attached after `since`, when the arm's ticket was filed, and not copied from the source.
    """
    folder = manifest.trial_dir(pair.trial) / pair.pair / arm
    names: list[str] = []
    for item in io.attachments(ticket_id):
        name = str(item["name"])
        if name in pair.copied_attachments or str(item.get("created_at", "")) < str(
            since or ""
        ):
            continue
        io.get_attachment(ticket_id, name, folder / name)
        names.append(name)
    return names


def block_sessions(trial: str, everything: list[dict[str, Any]]) -> set[str]:
    """The sessions that worked the trial's own tickets, which may close a copy."""
    found: set[str] = set()
    for t in everything:
        if str(t.get("parent") or "") == trial or str(t.get("id")) == trial:
            for h in t.get("history") or []:
                assignee = (h.get("change") or {}).get("assignee")
                if assignee and assignee[1]:
                    found.add(str(assignee[1]))
    return found


def grade_pair(
    pair: Pair,
    spec: Spec,
    everything: list[dict[str, Any]],
    writers: set[str],
    main: Pair | None = None,
    siblings: Sequence[tuple[str, str]] = (),
) -> list[Grade] | None:
    """Grades both arms of `pair` once both have ended. `main` is the pair that filed the source
    (`pair` itself unless it is another arm's copy), and `siblings` are the (ID, title) of the
    source's other copies, which neither arm should have seen."""
    main = main or pair
    source, copy = io.show([pair.source, str(pair.copy)], history=True, comments=True)
    s_claim, c_claim = first_claim(source), first_claim(copy)
    if not (s_claim and s_claim.ended and c_claim and c_claim.ended):
        return None
    start_commit, copy_id = pair.start_commit, pair.copy
    assert start_commit is not None and copy_id is not None
    text = candidates.ticket_text(source)

    # The source: its first landing.
    s_start: str | None = None
    s_end: str | None = None
    s_diff = Diff([], 0, 0, 0, [])
    landed = s_claim.landed
    if landed:
        s_start, s_end = io.git("rev-parse", f"{landed[0]}^"), landed[-1]
        s_diff = diff(s_start, s_end)
    # The copy: its branch.
    branch = f"refs/heads/trial/{copy_id}"
    c_end = (
        io.git("rev-parse", branch)
        if io.git_ok("rev-parse", "--verify", "-q", branch)
        else None
    )
    base = io.git("merge-base", branch, io.STAGING) if c_end else None
    c_diff = diff(start_commit, c_end) if c_end else Diff([], 0, 0, 0, [])

    arms: dict[str, scan.Arm] = {}
    costs: dict[str, dict[str, Any]] = {}
    for name, ticket, claim, d in (
        ("source", source, s_claim, s_diff),
        ("copy", copy, c_claim, c_diff),
    ):
        tid = str(ticket["id"])
        costs[name] = cost(tid)
        sessions = {claim.session}
        arms[name] = scan.Arm(
            ticket=ticket,
            sessions=sessions,
            traces=traces(costs[name]),
            added=d.added_lines,
            tree=str(ticket.get("tree") or "") or None,
            elsewhere=[
                (str(t["id"]), str(c.get("author")))
                for t in everything
                if str(t.get("id")) != tid
                for c in t.get("comments") or []
                if str(c.get("author")) in sessions
            ],
        )
    void = scan.void_reasons(arms["source"], arms["copy"], start_commit, base, writers)
    if c_end is None:
        void.append(f"the copy left no branch trial/{pair.copy}")
    void += arm_routing(
        spec.source, s_claim, costs["source"], pair.models.get("source", ""), "source"
    )
    void += arm_routing(
        spec.arm(pair.arm), c_claim, costs["copy"], pair.models.get("copy", ""), "copy"
    )
    if pair.contamination and pair.contamination.get("decision") == "void":
        void.append(f"voided for contamination: {pair.contamination.get('note')}")
    candidates_shared = scan.shared_lines(
        s_diff.added_lines, c_diff.added_lines, text, set()
    )
    in_start = {
        l
        for l in candidates_shared
        if io.git_ok("grep", "-q", "-F", "-e", l, start_commit)
    }
    suspect = scan.suspect(
        arms["source"],
        arms["copy"],
        pair.copy_title or "",
        s_claim.landed or [],
        text,
        in_start,
        siblings,
    )
    grades: list[Grade] = []
    for name, ticket, claim, d, start, end, verb in (
        ("source", source, s_claim, s_diff, s_start, s_end, "land"),
        ("copy", copy, c_claim, c_diff, start_commit, c_end, "check"),
    ):
        row = costs[name]
        finished = (
            claim.ended == "landed"
            if name == "source"
            else bool(claim.ended == "closed" and c_end)
        )
        grades.append(
            Grade(
                pair=pair.pair,
                trial=pair.trial,
                arm=name,
                ticket=str(ticket["id"]),
                ended=str(claim.ended),
                finished=finished,
                start_commit=start,
                end_commit=end,
                check=check_verdict(end, verb),
                dollars=row.get("dollars"),
                tokens={k: int(v or 0) for k, v in (row.get("tokens") or {}).items()},
                agent_minutes=row.get("agent_minutes"),
                model_minutes=round(
                    sum(
                        float(t.get("minutes") or 0)
                        for t in row.get("time_detail") or []
                        if t.get("category") == "model"
                    ),
                    1,
                ),
                claim_to_close_minutes=minutes_between(
                    claim.claimed_at, str(claim.ended_at)
                ),
                files_changed=len(d.files),
                lines_added=d.added,
                lines_removed=d.removed,
                test_lines_added=d.test_added,
                unnamed_files=[f for f in d.files if f not in text],
                filed_tickets=sorted(
                    str(t["id"])
                    for t in everything
                    if str(t.get("created_by")) == claim.session
                ),
                comments=[{"ticket": t, "author": a} for t, a in arms[name].elsewhere],
                attachments=attachments(
                    pair,
                    str(ticket["id"]),
                    name,
                    main.filed_at if name == "source" else pair.filed_at,
                ),
                models=models_of(row),
                void=void,
                suspect=[f.row() for f in suspect],
            )
        )
    return grades


def model_requests(cost_row: dict[str, Any]) -> dict[str, int]:
    """How many requests the ticket's sessions made on each model."""
    bead = str(cost_row.get("bead") or "")
    if not BEAD.fullmatch(bead):
        return {}
    rows = io.sql(
        f"SELECT model, count(*) AS n FROM requests WHERE bead = '{bead}' GROUP BY model"
    )
    return {
        str(r["model"]): int(r["n"] or 0)
        for r in rows
        if r.get("model") and r["model"] != "<synthetic>"
    }


def models_of(cost_row: dict[str, Any]) -> list[str]:
    return sorted(model_requests(cost_row))


# A model that carries less than this share of an arm's requests does not void the pair. Agy
# runs short helper steps on a lighter model (a few `gemini-3.8-flash` requests in a session of
# hundreds on the arm's model), which says nothing about which model did the work.
HELPER_SHARE = 0.1


def other_models(requests: dict[str, int], model: str) -> list[str]:
    """The models other than `model` that did a real part of the work. Agy records a model with
    its effort (`gemini-4-neon-high` for `gemini-4-neon`), so a name that extends `model` with a
    hyphen is the same model."""
    total = sum(requests.values())
    return sorted(
        m
        for m, n in requests.items()
        if m != model
        and not m.startswith(f"{model}-")
        and total
        and n / total >= HELPER_SHARE
    )


def arm_routing(
    arm: Arm, claim: Claim, cost_row: dict[str, Any], model: str, name: str
) -> list[str]:
    """Why an arm was not worked as its spec says: by another provider or model."""
    found: list[str] = []
    provider = (
        claim.session.removeprefix("cli-").rsplit("-", 1)[0]
        if claim.session.startswith("cli-")
        else None
    )
    if provider is not None and provider != arm.provider:
        found.append(f"the {name} was worked by {provider}, not {arm.provider}")
    other = other_models(model_requests(cost_row), model)
    if model and other:
        found.append(f"the {name} was worked by {', '.join(other)}, not {model}")
    return found


def cmd_grade(args: argparse.Namespace, spec: Spec) -> int:
    trial: str = args.trial
    graded = {g.pair for g in manifest.read_grades(trial)}
    todo = [
        p
        for p in manifest.read_pairs(trial)
        if p.state == FILED and p.pair not in graded
    ]
    if not todo:
        print("No filed pair waits to be graded.")
        return 0
    everything = io.export()
    writers = block_sessions(trial, everything)
    pairs = manifest.read_pairs(trial)
    by_token = {p.pair: p for p in pairs}
    for pair in todo:
        siblings = [
            (str(p.copy), p.copy_title or "")
            for p in pairs
            if p.source == pair.source and p.pair != pair.pair and p.copy
        ]
        main = by_token.get(pair.main or pair.pair, pair)
        grades = grade_pair(pair, spec, everything, writers, main, siblings)
        if grades is None:
            print(f"{pair.source} / {pair.copy}: not both ended yet")
            continue
        for grade in grades:
            manifest.append_grade(grade)
        verdict = (
            "void" if grades[0].void else "suspect" if grades[0].suspect else "graded"
        )
        print(f"{pair.source} / {pair.copy}: {verdict}")
        for reason in grades[0].void:
            print(f"  void: {reason}")
        for found in grades[0].suspect:
            print(f"  suspect: {found['what']} ({found['where']})")
    return 0
