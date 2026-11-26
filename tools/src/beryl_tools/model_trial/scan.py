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
from collections.abc import Sequence
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
# A quoted argument, such as a closing note given to `ignite close -m`. Sessions write what
# they did not do in these ("Did not run just bugs done"), so the command rules skip them.
QUOTED: re.Pattern[str] = re.compile(r'"(?:[^"\\]|\\.)*"|\'[^\']*\'')
# The `bead:` line of a bug report's front matter, which names the ticket filed for it. The
# copy of a bug ticket has to read the report, so seeing the source's ID there is not a route
# between the arms. A Claude transcript's tool output keeps its line breaks as the two
# characters `\n`, so either kind of break ends the line.
REPORT_BEAD: re.Pattern[str] = re.compile(
    r"(?:^|\\n)[ \t]*(?:[0-9]+:[ \t]*)?bead:[ \t]*[A-Za-z0-9.]+[ \t]*(?=\\n|$)", re.M
)
# A line of `git branch` output that holds only a branch name, with an optional `* ` or two
# spaces before it. A copy that lists branches after committing sees every other copy's
# `trial/` branch this way without having looked for it.
BRANCH_LISTING: re.Pattern[str] = re.compile(r"^(?:\* |  )?trial/[A-Za-z0-9.]+[ \t]*$")
# A line of `ignite ticket show` (`waits on:`, `waited on by:`) or `ignite start` (`needs`)
# that lists a ticket's dependencies. The trial's next block ticket depends on every arm, so
# the source's output can list every copy's ID there without the source looking for it.
DEPENDENCY_LINE: re.Pattern[str] = re.compile(
    r"^\s*(?:waits on:|waited on by:|needs\b)"
)
# A Claude transcript's tool output keeps its line breaks as the two characters `\n`.
LINE_BREAK: re.Pattern[str] = re.compile(r"\r?\n|\\n")


def unquoted(text: str) -> str:
    """`text` with every quoted argument emptied. A command hidden inside quotes
    (`sh -c "git fetch"`) escapes the command rules, but the suspect scan still reads it.
    """
    return QUOTED.sub('""', text)


def without_report_bead(trace: Trace) -> str:
    return REPORT_BEAD.sub("", trace.text) if trace.kind == "output" else trace.text


def without_lines(trace: Trace, drop: re.Pattern[str]) -> str:
    """An output's text without the lines that match `drop`. Only output is filtered: a
    command that names a `trial/` branch or a ticket still counts."""
    if trace.kind != "output":
        return trace.text
    return "\n".join(
        line for line in LINE_BREAK.split(trace.text) if not drop.search(line)
    )


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
        if names_source.search(without_report_bead(trace)):
            found.append(
                f"the copy's {trace.kind} names the source {sid} ({trace.where})"
            )
        # A branch listing is suspect evidence (see `suspect`), not a void reason.
        for branch in TRIAL_BRANCH.findall(without_lines(trace, BRANCH_LISTING)):
            if branch.rstrip(".") not in (cid, "<this", ""):
                found.append(
                    f"the copy's {trace.kind} shows trial/{branch} ({trace.where})"
                )
        if trace.kind != "command":
            # Output can show another tree without the copy looking for it: the newest log
            # under `ignite/runs` may be another session's. The suspect scan still reports
            # the source's own tree wherever it appears.
            continue
        bare = unquoted(text)
        for tree in TREE_PATH.findall(bare):
            if copy.tree and tree != copy.tree:
                found.append(
                    f"the copy's command shows another tree, {tree} ({trace.where})"
                )
        if FETCH.search(bare):
            found.append(f"the copy ran git fetch ({trace.where})")
        if ORIGIN_REF.search(bare):
            found.append(f"the copy read a ref under origin/ ({trace.where})")
        if FLAKY.search(bare):
            found.append(f"the copy switched a check or a flaky record ({trace.where})")
        if BUGS_DONE.search(bare):
            found.append(f"the copy ran just bugs done ({trace.where})")
        for verb, target in TICKET_WRITE.findall(bare):
            if target.rstrip(".") != cid:
                found.append(
                    f"the copy ran ignite ticket {verb} on {target} ({trace.where})"
                )
    names_copy = mention(cid)
    for trace in source.traces:
        # A dependency list is suspect evidence (see `suspect`), not a void reason.
        if names_copy.search(without_lines(trace, DEPENDENCY_LINE)):
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
    siblings: Sequence[tuple[str, str]] = (),
) -> list[Found]:
    """Every sign that one arm saw the other's work or knew it was in a trial. `siblings` are
    the (ID, title) of the source's copies on other arms, which neither arm should see.
    """
    found: list[Found] = []
    for this, name in ((source, "source"), (copy, "copy")):
        for trace in this.traces:
            for sid, title in siblings:
                if mention(sid).search(trace.text) or (title and title in trace.text):
                    found.append(
                        Found(f"the {name} saw another arm's copy {sid}", trace.where)
                    )
    for this, other, name in ((source, copy, "source"), (copy, source, "copy")):
        oid = str(other.ticket["id"])
        own_title = str(this.ticket.get("title") or "")
        titles = [
            t
            for t in {str(other.ticket.get("title") or ""), copy_title}
            if t and t != own_title
        ]
        for trace in this.traces:
            if mention(oid).search(without_report_bead(trace)):
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
    cid = str(copy.ticket["id"])
    for trace in copy.traces:
        # Branch listings are not void reasons, so the block session reads them here.
        for line in LINE_BREAK.split(trace.text) if trace.kind == "output" else []:
            if BRANCH_LISTING.search(line) and f"trial/{cid}" not in line:
                found.append(
                    Found(
                        f"the copy's branch listing shows {line.strip(' *')}",
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
