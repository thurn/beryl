"""The commits source and the history queries read from it (docs/efficiency-dashboard.md,
"Units" and "Rework").

The source runs two git commands in the main checkout. `git log` over `staging` and `master`
gives every commit with its parents, time, subject and changed files. The reflog of the local
`master` gives each time Enact moved `master` forward, which says when a commit reached it:
git keeps no other record of that, because Enact fast-forwards `master` to commits made
earlier on `staging`. Reflog entries are kept in the store after git expires them.

A commit's bead is not stored: it is read at query time from the beads' `enact_commits`,
which can change after the commit is ingested.
"""

from __future__ import annotations

import json
import sqlite3
import subprocess
from collections import defaultdict
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from .beads import main_checkout, run_bd
from .ingest import Row, SnapshotSource, SourceError
from .scope import parse_time

LOG: list[str] = [
    "git",
    "log",
    "--format=%x1e%H%x1f%P%x1f%cI%x1f%s",
    "--name-status",
    "-M",
    "--no-color",
    "staging",
    "master",
    "--",
]
REFLOG: list[str] = [
    "git",
    "log",
    "-g",
    "--format=%H%x1f%gd",
    "--date=unix",
    "refs/heads/master",
    "--",
]
MASTER = "refs/heads/master"
# Separates the two commands' output in one snapshot.
SEPARATOR = b"\x1d"
CARDS = "rust/cards/"


def is_card(path: str) -> bool:
    """Whether `path` is a card's definition, `rust/cards/<crate>/<set>/<card>.rs`; the
    crates' own `lib.rs` and `build.rs` sit one level up and are not cards."""
    return path.startswith(CARDS) and path.endswith(".rs") and path.count("/") == 4


def utc(moment: str) -> str:
    """An ISO time with any offset as UTC, in the form the beads use."""
    found = datetime.fromisoformat(moment.replace("Z", "+00:00"))
    if found.tzinfo is None:
        found = found.replace(tzinfo=timezone.utc)
    return found.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def commit_rows(log: str) -> tuple[list[Row], int]:
    """The `commits` rows of `git log`'s output, and how many records were malformed."""
    rows: list[Row] = []
    skipped = 0
    for record in log.split("\x1e"):
        if not record.strip():
            continue
        head, _, body = record.partition("\n")
        fields = head.split("\x1f")
        if len(fields) != 4 or not fields[0]:
            skipped += 1
            continue
        sha, parents, time, subject = fields
        paths: set[str] = set()
        added: set[str] = set()
        for change in body.splitlines():
            parts = change.split("\t")
            if len(parts) < 2 or not parts[0]:
                continue
            status = parts[0][0]
            paths.update(parts[1:])
            if status in "AC":
                added.add(parts[-1])
        try:
            when = utc(time)
        except ValueError:
            skipped += 1
            continue
        rows.append(
            (
                "commits",
                {
                    "sha": sha,
                    "time": when,
                    "subject": subject,
                    "parents": json.dumps(parents.split()),
                    "paths": json.dumps(sorted(paths)),
                    "added": json.dumps(sorted(added)),
                },
            )
        )
    return rows, skipped


def move_rows(reflog: str) -> tuple[list[Row], int]:
    """The `ref_moves` rows of `master`'s reflog, printed as `<sha>\\x1f<ref>@{<unix>}`."""
    rows: list[Row] = []
    skipped = 0
    for line in reflog.splitlines():
        if not line.strip():
            continue
        sha, _, selector = line.partition("\x1f")
        stamp = selector.rpartition("@{")[2].rstrip("}")
        try:
            moment = datetime.fromtimestamp(int(stamp), timezone.utc)
        except ValueError:
            skipped += 1
            continue
        time = moment.strftime("%Y-%m-%dT%H:%M:%SZ")
        rows.append(("ref_moves", {"ref": MASTER, "time": time, "sha": sha}))
    return rows, skipped


class Commits(SnapshotSource):
    """Every commit on `staging` and `master`, and `master`'s reflog."""

    name = "commits"
    # Only the commits: reflog entries git has expired stay in `ref_moves`.
    owns = ("commits",)

    def __init__(self, read: Callable[[], bytes]) -> None:
        self._read = read

    @staticmethod
    def here(start: Path) -> Commits:
        """The source for the repository at `start`, read in its main checkout, whose
        `staging` and `master` Enact fast-forwards."""
        checkout = main_checkout(start)
        return Commits(lambda: run_bd(checkout, LOG) + SEPARATOR + reflog(checkout))

    def read(self) -> bytes:
        return self._read()

    def parse(self, data: bytes) -> tuple[list[Row], int]:
        log, found, moves = data.partition(SEPARATOR)
        if not found:
            raise SourceError("the commits snapshot has no reflog part")
        commits, bad = commit_rows(log.decode(errors="replace"))
        refs, bad_refs = move_rows(moves.decode(errors="replace"))
        return commits + refs, bad + bad_refs


def reflog(checkout: Path) -> bytes:
    """`master`'s reflog; empty when the checkout has none."""
    try:
        done = subprocess.run(REFLOG, cwd=checkout, capture_output=True, check=False)
    except OSError:
        return b""
    return done.stdout if done.returncode == 0 else b""


@dataclass(frozen=True)
class Commit:
    sha: str
    time: str
    subject: str
    parents: tuple[str, ...]
    paths: frozenset[str]
    added: frozenset[str]

    def is_fix(self) -> bool:
        """Whether the subject is a Conventional Commits fix, such as `fix: ...` or
        `fix(ui): ...`."""
        return self.subject.lower().startswith("fix")


class History:
    """The commits, the bead that landed each, when each reached `master`, and which bead
    added each card file."""

    def __init__(
        self,
        commits: Iterable[Commit],
        landed: Mapping[str, Iterable[str]],
        moves: Iterable[tuple[str, str]],
    ) -> None:
        """`landed` is each bead's `enact_commits`; `moves` is each (time, sha) `master`
        moved to."""
        self.commits: dict[str, Commit] = {c.sha: c for c in commits}
        self.bead_of: dict[str, str] = {}
        for bead in sorted(landed):
            for sha in landed[bead]:
                if sha in self.commits:
                    self.bead_of.setdefault(sha, bead)
        self.by_bead: dict[str, list[Commit]] = defaultdict(list)
        for sha, bead in self.bead_of.items():
            self.by_bead[bead].append(self.commits[sha])
        for found in self.by_bead.values():
            found.sort(key=lambda c: (c.time, c.sha))
        self.on_master: dict[str, str] = {}
        for time, sha in sorted(moves):
            for reached in self.ancestors(sha, self.on_master):
                self.on_master[reached] = time
        # The commit that first added each card file, in commit order.
        self.card_added: dict[str, Commit] = {}
        for c in sorted(self.commits.values(), key=lambda c: (c.time, c.sha)):
            for path in sorted(c.added):
                if is_card(path):
                    self.card_added.setdefault(path, c)

    def ancestors(self, sha: str, stop: Mapping[str, object] | set[str]) -> list[str]:
        """`sha` and its ancestors in the store, not walking past a commit in `stop`."""
        found: list[str] = []
        seen: set[str] = set()
        stack = [sha]
        while stack:
            current = stack.pop()
            if current in seen or current in stop or current not in self.commits:
                continue
            seen.add(current)
            found.append(current)
            stack.extend(self.commits[current].parents)
        return found

    def range(self, spec: str) -> list[Commit]:
        """The commits of a range `A..B`: reachable from B and not from A."""
        start, dots, end = spec.partition("..")
        if not dots:
            return []
        excluded = set(self.ancestors(start, set())) if start else set()
        return [self.commits[s] for s in self.ancestors(end, excluded)]

    def adder(self, path: str) -> str | None:
        """The bead whose commit first added the card file `path`."""
        found = self.card_added.get(path)
        return self.bead_of.get(found.sha) if found else None

    def cards_added(self, bead: str) -> list[str]:
        """The card files the bead's commits added first."""
        return sorted(
            p for p, c in self.card_added.items() if self.bead_of.get(c.sha) == bead
        )


def landed(enact: Mapping[str, object]) -> list[str]:
    """The commits a bead's `enact_commits` names."""
    value = enact.get("enact_commits")
    return str(value).split() if value else []


def load(db: sqlite3.Connection, beads: Mapping[str, Mapping[str, object]]) -> History:
    """The history in the store, with each bead's `enact_*` metadata from `beads`."""
    commits = [
        Commit(
            str(r[0]),
            str(r[1]),
            str(r[2] or ""),
            tuple(json.loads(r[3])),
            frozenset(json.loads(r[4])),
            frozenset(json.loads(r[5])),
        )
        for r in db.execute(
            "SELECT sha, time, subject, parents, paths, added FROM commits"
        )
    ]
    moves = [
        (str(r[0]), str(r[1]))
        for r in db.execute("SELECT time, sha FROM ref_moves WHERE ref = ?", (MASTER,))
    ]
    return History(commits, {b: landed(e) for b, e in beads.items()}, moves)


@dataclass(frozen=True)
class Lead:
    """A bead's lead time in minutes, each part null where a time is missing: from creation
    to start (`queue`), from start to close (`work`), and from its last commit's time on
    `staging` to that commit reaching `master` (`to_master`). `whole` runs from creation to
    the last commit reaching `master`."""

    queue: float | None
    work: float | None
    to_master: float | None
    whole: float | None


def between(start: str | None, end: str | None) -> float | None:
    if start is None or end is None:
        return None
    return (parse_time(end) - parse_time(start)).total_seconds() / 60


def lead(
    history: History,
    bead: str,
    created: str | None,
    started: str | None,
    closed: str | None,
) -> Lead:
    commits = history.by_bead.get(bead, [])
    last = commits[-1] if commits else None
    reached = history.on_master.get(last.sha) if last else None
    return Lead(
        between(created, started),
        between(started, closed),
        between(last.time if last else None, reached),
        between(created, reached),
    )
