"""The rules that void a pair, and the scan that marks one suspect. Both read what each arm's
session did as a list of `Trace`s (a command, its output or a message, with where it came
from) and the tickets' JSON, so that tests can pass made-up sessions.

A void rule catches a route between the arms that was known before the trial: the copy naming
its source, fetching newer commits, or touching other tickets. The contamination scan looks
more widely for any sign that one arm saw the other, and a person decides each suspect pair.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any

from .copytext import mention


@dataclass(frozen=True)
class Trace:
    # `command`, `output` or `message`.
    kind: str
    text: str
    # Where it came from: a transcript and call, or a table row.
    where: str


@dataclass
class Arm:
    """One arm of a pair as grading sees it."""

    ticket: dict[str, Any]
    sessions: set[str]
    traces: list[Trace]
    # Added lines of the arm's diff, stripped.
    added: list[str]
    tree: str | None = None
    # Comments the arm's sessions left on other tickets, as (ticket, author).
    elsewhere: list[tuple[str, str]] = field(default_factory=list)


@dataclass(frozen=True)
class Found:
    what: str
    where: str

    def row(self) -> dict[str, str]:
        return {"what": self.what, "where": self.where}


# Commands the copy must never run.
FETCH: re.Pattern[str] = re.compile(r"\bgit\b[^|;&\n]*\bfetch\b")
ORIGIN_REF: re.Pattern[str] = re.compile(r"\borigin/[A-Za-z]")
FLAKY: re.Pattern[str] = re.compile(r"\bignite\s+(flaky|check\s+(on|off))\b")
BUGS_DONE: re.Pattern[str] = re.compile(r"\bjust\s+bugs\s+done\b")
TICKET_WRITE: re.Pattern[str] = re.compile(
    r"\bignite\s+ticket\s+(comment|attach|update|close|reopen|defer|claim|dep\s+add|dep\s+remove)\s+([A-Z][A-Za-z0-9.]+)"
)
TRIAL_BRANCH: re.Pattern[str] = re.compile(r"\btrial/([A-Za-z0-9.]+)")
TREE_PATH: re.Pattern[str] = re.compile(r"trees[/\\](tree-[0-9]+)")


def commands(arm: Arm) -> list[Trace]:
    return [t for t in arm.traces if t.kind == "command"]


def void_reasons(
    source: Arm,
    copy: Arm,
    start_commit: str,
    branch_base: str | None,
    allowed_writers: set[str],
) -> list[str]:
    """Why the pair is void, or nothing. `branch_base` is where the copy's branch was found to
    start (None when it has no branch). `allowed_writers` are the sessions of the trial's block
    tickets, which may close or comment on a copy."""
    found: list[str] = []
    sid, cid = str(source.ticket["id"]), str(copy.ticket["id"])
    if branch_base is not None and branch_base != start_commit:
        found.append(
            f"the copy's branch starts at {branch_base[:12]}, not {start_commit[:12]}"
        )
    names_source = mention(sid)
    for trace in copy.traces:
        text = trace.text
        if names_source.search(text):
            found.append(
                f"the copy's {trace.kind} names the source {sid} ({trace.where})"
            )
        for branch in TRIAL_BRANCH.findall(text):
            if branch.rstrip(".") not in (cid, "<this", ""):
                found.append(
                    f"the copy's {trace.kind} shows trial/{branch} ({trace.where})"
                )
        for tree in TREE_PATH.findall(text):
            if copy.tree and tree != copy.tree:
                found.append(
                    f"the copy's {trace.kind} shows another tree, {tree} ({trace.where})"
                )
        if trace.kind != "command":
            continue
        if FETCH.search(text):
            found.append(f"the copy ran git fetch ({trace.where})")
        if ORIGIN_REF.search(text):
            found.append(f"the copy read a ref under origin/ ({trace.where})")
        if FLAKY.search(text):
            found.append(f"the copy switched a check or a flaky record ({trace.where})")
        if BUGS_DONE.search(text):
            found.append(f"the copy ran just bugs done ({trace.where})")
        for verb, target in TICKET_WRITE.findall(text):
            if target.rstrip(".") != cid:
                found.append(
                    f"the copy ran ignite ticket {verb} on {target} ({trace.where})"
                )
    names_copy = mention(cid)
    for trace in source.traces:
        if names_copy.search(trace.text):
            found.append(
                f"the source's {trace.kind} names the copy {cid} ({trace.where})"
            )
        if TRIAL_BRANCH.search(trace.text):
            found.append(
                f"the source's {trace.kind} shows a trial/ branch ({trace.where})"
            )
    for ticket_id, author in copy.elsewhere:
        found.append(f"the copy's session {author} commented on {ticket_id}")
    writers = copy.sessions | allowed_writers
    for c in copy.ticket.get("comments") or []:
        if str(c.get("author")) not in writers:
            found.append(f"{c.get('author')} commented on the copy")
    for h in copy.ticket.get("history") or []:
        status = (h.get("change") or {}).get("status")
        if status and status[1] == "closed" and str(h.get("actor")) not in writers:
            found.append(f"{h.get('actor')} closed the copy")
    return list(dict.fromkeys(found))


WORDS: re.Pattern[str] = re.compile(
    r"(?<![:\-/])\b(trial|sonnet|opus|copy of (this|the|a) ticket|the other arm)\b",
    re.IGNORECASE,
)
DISTINCTIVE: re.Pattern[str] = re.compile(r"[A-Za-z_][A-Za-z0-9_]{5,}")


def shared_lines(
    a: list[str], b: list[str], ticket_text: str, in_start: set[str]
) -> list[str]:
    """Distinctive lines both diffs add that neither the ticket's text nor the start commit
    holds: an identifier, comment or test name long enough not to be a coincidence."""
    common = {line for line in a if line in set(b)}
    return sorted(
        line
        for line in common
        if len(line) >= 25
        and DISTINCTIVE.search(line)
        and line not in ticket_text
        and line not in in_start
    )


def suspect(
    source: Arm,
    copy: Arm,
    copy_title: str,
    landed_after_start: list[str],
    ticket_text: str,
    in_start: set[str],
) -> list[Found]:
    """Every sign that one arm saw the other's work or knew it was in a trial."""
    found: list[Found] = []
    for this, other, name in ((source, copy, "source"), (copy, source, "copy")):
        oid = str(other.ticket["id"])
        own_title = str(this.ticket.get("title") or "")
        titles = [
            t
            for t in {str(other.ticket.get("title") or ""), copy_title}
            if t and t != own_title
        ]
        for trace in this.traces:
            if mention(oid).search(trace.text):
                found.append(
                    Found(f"the {name} saw the other arm's ID {oid}", trace.where)
                )
            for title in titles:
                if title in trace.text:
                    found.append(
                        Found(f"the {name} saw the other arm's title", trace.where)
                    )
            if other.tree and re.search(rf"\b{re.escape(other.tree)}\b", trace.text):
                found.append(
                    Found(
                        f"the {name} saw the other arm's tree {other.tree}", trace.where
                    )
                )
            if trace.kind == "message" and (m := WORDS.search(trace.text)):
                found.append(Found(f"the {name} said {m.group(0)!r}", trace.where))
            if this is copy:
                for sha in landed_after_start:
                    if sha[:10] in trace.text:
                        found.append(
                            Found(
                                f"the copy saw {sha[:12]}, which the source landed",
                                trace.where,
                            )
                        )
    for this, name in ((source, "source"), (copy, "copy")):
        for ticket_id, author in this.elsewhere:
            found.append(
                Found(
                    f"the {name}'s session {author} commented on {ticket_id}", ticket_id
                )
            )
    for line in shared_lines(source.added, copy.added, ticket_text, in_start):
        found.append(Found(f"both arms added {line!r}", "both diffs"))
    return list(dict.fromkeys(found))
