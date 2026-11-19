"""Command kinds: what a tool call was doing, for the time split (docs/efficiency-dashboard.md,
"Command kinds").

A shell command's kind comes from the ordered table in `data/efficiency-command-kinds.json`,
so that a new command can be named without changing code. Each entry has a `kind`, a
`pattern` (a Python regular expression searched for in the command) and `heavy`, true for
commands that take real time or CPU, such as builds, tests and waits.

The command is split into the parts it chains with `&&`, `||` and `;`, and each part is
matched after removing a leading `cd <path>`, environment assignments and wrappers such as
`timeout 600`. A path that happens to contain a word such as `enact` therefore does not
decide the kind. When any part is heavy, the command takes the kind of the first heavy part,
and is flagged as chained when there are two or more heavy parts, since they cannot be timed
apart. Otherwise it takes the kind of the first part that matches. A command that matches
nothing is `other`. Tools other than the shell are their own kinds, named by the tool.
"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

OTHER = "other"
# The shell tools whose `command` is classified by the table.
SHELL_TOOLS: frozenset[str] = frozenset({"Bash"})
# A part's leading `cd <path>`, `NAME=value`, `export NAME=value`, `eval`, `time`, `nice` or
# `timeout <n>`, a shell keyword such as `do` or `then`, or an opening `(`, removed one at a time before matching.
PREFIX: re.Pattern[str] = re.compile(
    r"^\s*(?:cd(?:\s+(?:\"[^\"]*\"|'[^']*'|\S+))?\s*$"
    r"|cd\s+(?:\"[^\"]*\"|'[^']*'|\S+)\s+"
    r"|(?:export\s+)?[A-Za-z_][A-Za-z0-9_]*=(?:\"[^\"]*\"|'[^']*'|\S*)(?:\s+|$)"
    r"|(?:eval|time|nice|command|env|do|then|else|if|while|until|!)\s+"
    r"|[({]\s*"
    r"|timeout\s+\S+\s+)"
)
# Where a command chains its parts. A separator inside quotes splits too: the parts are only
# matched, never run, and a quoted `&&` is rare enough not to matter.
SEPARATOR: re.Pattern[str] = re.compile(r"&&|\|\||;|\n")


@dataclass(frozen=True)
class Kind:
    kind: str
    pattern: re.Pattern[str]
    heavy: bool


def table_path() -> Path:
    """`data/efficiency-command-kinds.json` in the checkout this package is in."""
    return (
        Path(__file__).resolve().parents[3] / "data" / "efficiency-command-kinds.json"
    )


def parse(data: bytes) -> list[Kind]:
    """The table in `data`, checked: a list of objects, each with a unique `kind`, a
    `pattern` that compiles and an optional boolean `heavy`."""
    entries = json.loads(data)
    if not isinstance(entries, list):
        raise ValueError("the command kinds table is not a list")
    found: list[Kind] = []
    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError(f"entry {entry!r} is not an object")
        kind, pattern, heavy = (
            entry.get("kind"),
            entry.get("pattern"),
            entry.get("heavy"),
        )
        if not isinstance(kind, str) or not kind or kind == OTHER:
            raise ValueError(f"entry {entry!r} needs a kind other than {OTHER!r}")
        if kind in seen:
            raise ValueError(f"kind {kind!r} appears twice")
        if not isinstance(pattern, str):
            raise ValueError(f"kind {kind!r} has no pattern")
        if heavy is not None and not isinstance(heavy, bool):
            raise ValueError(f"kind {kind!r}: heavy must be true or false")
        seen.add(kind)
        found.append(Kind(kind, re.compile(pattern), bool(heavy)))
    return found


def load(path: Path | None = None) -> tuple[list[Kind], str]:
    """The table and a digest of its bytes, which changes whenever it does."""
    data = (path or table_path()).read_bytes()
    return parse(data), hashlib.sha256(data).hexdigest()[:16]


# The start of a here-document: `<<EOF`, `<<-EOF`, `<<'EOF'` or `<<"EOF"`, and its delimiter.
HEREDOC: re.Pattern[str] = re.compile(r"<<-?\s*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1")


def without_heredocs(command: str) -> str:
    """`command` without the bodies of its here-documents, which are data such as a commit
    message, not commands. A body that never reaches its delimiter, as in a command the store
    cut short, runs to the end. Without this, a long commit message pushed the commands after
    it past the characters the store keeps, and a body line such as `cargo test passes` was
    read as a command."""
    lines = command.split("\n")
    kept: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        for found in HEREDOC.finditer(line):
            delimiter = found.group(2)
            while index < len(lines) and lines[index].strip() != delimiter:
                index += 1
            index += 1  # the delimiter line
        # The operator is rewritten so that a command already without its bodies, as the
        # store keeps it, is not read as opening one again.
        kept.append(HEREDOC.sub(lambda m: f"<<({m.group(2)} body left out)", line))
    return "\n".join(kept)


def strip(part: str) -> str:
    """`part` without its leading `cd <path>`, assignments and wrappers."""
    while True:
        found = PREFIX.match(part)
        if not found or not found.group(0):
            return part.strip()
        part = part[found.end() :]


def match(table: list[Kind], part: str) -> Kind | None:
    return next((k for k in table if k.pattern.search(part)), None)


def classify(table: list[Kind], tool: str, command: str | None) -> tuple[str, bool]:
    """The kind of a tool call and whether it chains two or more heavy commands."""
    if tool not in SHELL_TOOLS:
        return tool or OTHER, False
    if not command:
        return OTHER, False
    found = [
        k
        for k in (
            match(table, strip(p))
            for p in SEPARATOR.split(without_heredocs(command))
            if p.strip()
        )
        if k is not None
    ]
    heavy = [k for k in found if k.heavy]
    if heavy:
        return heavy[0].kind, len(heavy) > 1
    return (found[0].kind if found else OTHER), False
