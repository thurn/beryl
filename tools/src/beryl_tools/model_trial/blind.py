"""What the blind reviewer and the user's judging page are shown of a pair, with everything
that would tell which arm is which taken out.

Each arm is shown as side A or side B: the ticket text both read, its diff, its closing note,
the comments it left, the tickets it filed, its check verdict and its images. `scrub` removes
model names, session IDs, ticket IDs of the pair, branch names, build tree paths, dollar
amounts, times and durations from the text an arm's session wrote and from the ticket text.
The diffs and the repository snapshots are shown as they are: they are the work under review,
and a model name in them (such as in `ignite.toml`) is part of the code. Commit messages are
not shown, because Ignite's commit messages name tickets.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

# The heading where a copy's own rules start, after the text both arms read (`copytext.rules`).
COPY_RULES = "## How to start and end this ticket"

MODEL_NAME: re.Pattern[str] = re.compile(
    r"\b(claude[\w.-]*|anthropic|opus[\w.-]*|sonnet[\w.-]*|haiku[\w.-]*|fable[\w.-]*"
    r"|gpt-[\w.-]+|gemini[\w.-]*|codex|antigravity)\b",
    re.IGNORECASE,
)
SESSION: re.Pattern[str] = re.compile(
    r"\b(cli-[a-z][\w-]*-[0-9a-f]{6,}|local_[0-9a-f-]{8,}|session[ :=]+[\w-]{6,})\b",
    re.IGNORECASE,
)
BRANCH: re.Pattern[str] = re.compile(r"\btrial/[\w.<>-]+")
TREE: re.Pattern[str] = re.compile(r"\S*trees[/\\]tree-[0-9]+\S*")
DOLLARS: re.Pattern[str] = re.compile(r"(US)?\$\s?[0-9][0-9,]*(\.[0-9]+)?")
TIMESTAMP: re.Pattern[str] = re.compile(
    r"\b[0-9]{4}-[0-9]{2}-[0-9]{2}([T ][0-9:.]+(Z|[+-][0-9:]+)?)?"
)
DURATION: re.Pattern[str] = re.compile(
    r"\b[0-9]+(\.[0-9]+)?\s*(agent |model |wall )?"
    r"(minutes?|mins?|hours?|hrs?|seconds?|secs?|tokens?)\b",
    re.IGNORECASE,
)


def scrub_names(text: str, ticket_ids: list[str]) -> str:
    """`text` without model names, session IDs, build trees, trial branches or the IDs in
    `ticket_ids` (the pair's arms, their epic and the tickets either filed), each of which
    becomes "a ticket"."""
    for tid in sorted(set(ticket_ids), key=len, reverse=True):
        if tid:
            text = re.sub(
                rf"(?<![A-Za-z0-9.]){re.escape(tid)}(?![A-Za-z0-9]|\.[0-9])",
                "a ticket",
                text,
            )
    text = TREE.sub("the build tree", text)
    text = BRANCH.sub("a branch", text)
    text = SESSION.sub("a session", text)
    return MODEL_NAME.sub("a model", text)


def scrub(text: str, ticket_ids: list[str]) -> str:
    """`text` as `scrub_names` leaves it, and also without dollar amounts, times or durations:
    for the text an arm's session wrote."""
    text = scrub_names(text, ticket_ids)
    text = DOLLARS.sub("some dollars", text)
    text = TIMESTAMP.sub("a time", text)
    return DURATION.sub("some time", text)


def shared_text(copy_body: str) -> str:
    """The text both arms read, from the copy's body: the source's text, rewritten where it
    named the source, and its epic's context, without the copy's own rules."""
    return copy_body.split(COPY_RULES, 1)[0].rstrip()


def ticket_markdown(
    title: str, body: str, acceptance: str, ticket_ids: list[str]
) -> str:
    """`ticket.md`: the title, body and acceptance both arms read. The text is the same for
    both arms, so only names are scrubbed from it."""
    parts = [f"# {title}", shared_text(body)]
    if acceptance.strip():
        parts.append(f"## Acceptance\n\n{acceptance.strip()}")
    text = "\n\n".join(p for p in parts if p.strip()) + "\n"
    return scrub_names(text, ticket_ids)


@dataclass
class Filed:
    title: str
    body: str


@dataclass
class Side:
    """One arm as either reader sees it. `tree` and `diff` are None when the arm finished
    with no change."""

    diff: str | None
    note: str
    comments: list[str] = field(default_factory=list)
    filed: list[Filed] = field(default_factory=list)
    check: str = "unknown"
    images: list[Path] = field(default_factory=list)


def side_markdown(label: str, side: Side, ticket_ids: list[str]) -> str:
    """`A.md` or `B.md`: what the side wrote outside the code, scrubbed."""
    checks = {
        "pass": "The checks passed on its final commit.",
        "fail": "The checks failed on its final commit.",
    }.get(side.check, "No check result was recorded for its final commit.")
    lines = [f"# Side {label}", "", checks, ""]
    if side.diff is None:
        lines += ["This side finished with no change.", ""]
    lines += ["## Closing note", "", side.note.strip() or "(none)", ""]
    lines += ["## Comments it left", ""]
    lines += [f"- {c.strip()}" for c in side.comments] or ["(none)"]
    lines += ["", "## Tickets it filed", ""]
    for found in side.filed:
        lines += [f"### {found.title}", "", found.body.strip(), ""]
    if not side.filed:
        lines.append("(none)")
    return scrub("\n".join(lines).rstrip() + "\n", ticket_ids)


def image_names(images: list[Path]) -> list[str]:
    """The names a side's images are shown under: `image-1.png` and so on, in the order the
    arm attached them, so that no name gives away the arm or its ticket."""
    return [f"image-{i + 1}{p.suffix.lower()}" for i, p in enumerate(images)]


def labels(a_arm: str) -> dict[str, str]:
    """Which arm each side is, given the arm shown as A."""
    other = "copy" if a_arm == "source" else "source"
    return {"A": a_arm, "B": other}
