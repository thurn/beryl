"""Choosing the browser checks a change can break, for the fast check (plan K19; docs/project-50k.md,
Target architecture, Checks, "Selection"). `by ui-test --select` and `by ui-shots --select` run
only the checks chosen here, worst failure record first, as many as fit a time box, so the
author repairs what they broke before slow CI sees it.

A check is chosen, and its run-log record's `chosen` says why, when:

- `coverage:<file>:<line>`: a changed line of ui/src (JavaScript or CSS) is one it ran, by the
  nightly coverage file (coverage.py), mapped across the commits between it and the branch;
- `body`: its own body changed: its `check(...)` call in ui/scripts/interactions.mjs, or a
  top-level block of them, or its entry in ui/shots.json; for a verb check
  (`verb:<verb>/<seat>/<layout>`, ui/scripts/verbs.mjs), that script or its list, ui/verbs.json;
  for a shape check (`shape:<shape>/<seat>/<layout>`, ui/scripts/shapes.mjs), that script or its
  list, ui/shapes.json;
- `import:<file>`: code changed in a ui/src file interactions.mjs imports into Node, which no
  coverage sees: as if each line of interactions.mjs that uses one of its names had changed;
- `fixture:<name>`: a Rust or fixture change changed the recording of a fixture it loads, as the
  step recorded it against the recording the coverage run made;
- `harness`: the harness changed (capture.mjs, audit.mjs, Vite's config, interactions.mjs outside
  any check): every check is chosen;
- `record`: a change no check covers (new code, a file absent from coverage, coverage too old or
  without recordings): the rest of the time box goes to the worst failure records.

Chosen checks run catches first, then failures per run, cheapest first within a tie. Those the
box cuts are logged `skip` with `chosen: cut`; slow CI runs them. Comments select nothing.
"""

from __future__ import annotations

import fnmatch
import hashlib
import json
import os
import re
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import checks, paths, runlog, system, verdicts

# Coverage older than this is not trusted to choose: checks fall back to their records.
STALE_DAYS = 3
# How long a selected step may take, from its start: with the rust lane beside it, the fast
# check stays inside its 60 s.
BOX = 45.0
# Serving the UI and launching the browser, before the first check starts.
STARTUP = 4.0
# The seconds of a check with no timing in the run log or the coverage file.
DEFAULT_SECONDS = 3.0
# How far back the run log's records count (runlog.tally).
RECORD_DAYS = 30
# The pages a capture script runs at once (capture.mjs' workers).
WORKERS = 4
# What `--select` tells interactions.mjs to run: a file of titles, one a line, matched exactly.
CHOSEN_ENV = "BERYL_CHECKS"

# The paths every check of a lane runs through (fnmatch, `*` crossing folders). Lines of
# interactions.mjs outside any check are too (`body_reasons`).
COMMON_HARNESS = (
    "ui/scripts/capture.mjs",
    "ui/scripts/audit.mjs",
    "ui/index.html",
    "ui/package.json",
    "ui/package-lock.json",
    "ui/vite.config.ts",
    "ui/tsconfig.json",
    "ui/public/*",
)
HARNESS: dict[str, tuple[str, ...]] = {
    "ui-test": COMMON_HARNESS,
    "shots": (*COMMON_HARNESS, "ui/scripts/shots.mjs"),
}
INTERACTIONS = "ui/scripts/interactions.mjs"
# The verb suite, which `ui-test` runs beside interactions.mjs: its script, its cases, the list
# of its checks, and the prefix of their ids in the run log (the lane's others are `ui-test:<title>`).
VERB_SCRIPT = "ui/scripts/verbs.mjs"
VERB_CASES = (
    "ui/scripts/verb-cases.mjs",
    "ui/scripts/verb-cases-place.mjs",
    "ui/scripts/verb-cases-piles.mjs",
    "ui/scripts/verb-cases-spans.mjs",
    "ui/scripts/verb-cases-rest.mjs",
    "ui/scripts/verb-spans.mjs",
    "ui/scripts/verb-rest.mjs",
    "ui/scripts/written-marks.mjs",
)
VERBS = "ui/verbs.json"
VERB_PREFIX = "verb:"
# The shape suite, which `ui-test` runs after it: its script, its cases, the list of its checks,
# and the prefix of their ids in the run log.
SHAPE_SCRIPT = "ui/scripts/shapes.mjs"
SHAPE_CASES = ("ui/scripts/shape-cases.mjs",)
SHAPES = "ui/shapes.json"
SHAPE_PREFIX = "shape:"
SHOTS = "ui/shots.json"
FIXTURES = "ui/fixtures.json"
COMMENT = ("//", "/*", "*")


# What changed.


@dataclass
class Hunk:
    """One hunk of a `git diff -U0`: the old side's lines from `old`, `old_count` of them, and
    the new side's from `new`, with the text each side had."""

    path: str
    old: int
    old_count: int
    new: int
    new_count: int
    removed: list[str] = field(default_factory=list)
    added: list[str] = field(default_factory=list)

    def comment_only(self) -> bool:
        """Whether it changed only comments and blank lines."""
        return all(
            not line.strip() or line.strip().startswith(COMMENT)
            for line in self.removed + self.added
        )

    @staticmethod
    def side(start: int, count: int) -> list[int]:
        """A side's lines, or for an empty side (a pure insertion or deletion) the two lines it
        sits between: what runs around new code runs it."""
        return list(range(start, start + count)) if count else [start, start + 1]

    def old_lines(self) -> list[int]:
        return Hunk.side(self.old, self.old_count)

    def new_lines(self) -> list[int]:
        return Hunk.side(self.new, self.new_count)


HUNK: re.Pattern[str] = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def hunks(diff: str) -> dict[str, list[Hunk]]:
    """A `git diff -U0 --no-renames` parsed, per path (the new side's, or the old's when the file
    was deleted)."""
    found: dict[str, list[Hunk]] = {}
    path = ""
    current: Hunk | None = None
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            current = None
            path = line.split(" b/", 1)[-1]
            found.setdefault(path, [])
        elif m := HUNK.match(line):
            counts = [1 if c is None else int(c) for c in (m[2], m[4])]
            current = Hunk(path, int(m[1]), counts[0], int(m[3]), counts[1])
            found[path].append(current)
        elif current is not None and line.startswith("-"):
            current.removed.append(line[1:])
        elif current is not None and line.startswith("+"):
            current.added.append(line[1:])
    return found


def shift(between: list[Hunk], line: int) -> int | None:
    """Where the new side's `line` was on the old side of `between` (one file's hunks), or None
    when those hunks changed it."""
    offset = 0
    for h in between:
        if h.new_count and h.new <= line < h.new + h.new_count:
            return None
        # A hunk at or before the line moved it; an empty new side sits after `h.new`.
        if h.new + h.new_count <= line if h.new_count else h.new < line:
            offset += h.old_count - h.new_count
    return line + offset


def diff(*revisions: str, under: str = "ui/") -> dict[str, list[Hunk]]:
    return hunks(
        checks.git("diff", "-U0", "--no-renames", "--no-color", *revisions, "--", under)
    )


# The coverage file.


@dataclass
class Coverage:
    commit: str
    time: datetime
    checks: dict[str, dict[str, object]]
    # Each fixture's recording's hash at the commit, when the coverage run recorded them.
    recordings: dict[str, str] | None

    @staticmethod
    def load() -> Coverage | None:
        from . import coverage  # It imports shots, which imports this module.

        try:
            latest = json.loads((coverage.folder() / "latest.json").read_text())
            document = json.loads((coverage.folder() / latest["file"]).read_text())
            recordings = document.get("recordings")
            return Coverage(
                str(document["commit"]),
                datetime.fromisoformat(str(document["time"])),
                dict(document["checks"]),
                dict(recordings) if isinstance(recordings, dict) else None,
            )
        except (OSError, KeyError, TypeError, ValueError):
            return None

    def files(self) -> set[str]:
        return {f for e in self.checks.values() for f in files_of(e)}

    def covering(self, path: str, lines: Iterable[int]) -> list[str]:
        """The checks that ran any of `lines` of `path`."""
        wanted = set(lines)
        return [
            check
            for check, entry in self.checks.items()
            if any(
                first <= line <= last
                for first, last in files_of(entry).get(path, [])
                for line in wanted
            )
        ]


def files_of(entry: Mapping[str, object]) -> dict[str, list[list[int]]]:
    found = entry.get("files")
    return found if isinstance(found, dict) else {}


def fixtures_of(entry: Mapping[str, object]) -> list[str]:
    found = entry.get("fixtures")
    return [str(f) for f in found] if isinstance(found, list) else []


def stale(found: Coverage | None, now: datetime) -> str:
    """Why coverage cannot choose (it is missing, too old, of a commit this clone lacks, or
    this host has none), or empty when it can."""
    if system.WINDOWS:
        return "this host records no coverage"
    if found is None:
        return "no coverage recorded (`just coverage`, or `just nightly-install`)"
    if now - found.time > timedelta(days=STALE_DAYS):
        return f"the coverage of {found.commit[:8]} is over {STALE_DAYS} days old"
    if not checks.git("cat-file", "-t", found.commit):
        return f"the coverage's commit {found.commit[:8]} is not in this clone"
    return ""


# Where each check's body is.


@dataclass
class Region:
    """A `check(...)` call's lines in interactions.mjs and the titles it can give: a pattern,
    None when its title is computed where this cannot follow, and the title itself when it is
    spelled out."""

    first: int
    last: int
    title: re.Pattern[str] | None
    literal: str | None = None


CALL: re.Pattern[str] = re.compile(r"^(\s*)(?:await\s+)?check\((.*)$")
TITLE: re.Pattern[str] = re.compile(
    r'^\s*(?:"((?:[^"\\]|\\.)*)"|`([^`]*)`|([A-Za-z_]\w*))'
)


def title_pattern(text: str, source: str) -> tuple[re.Pattern[str] | None, str | None]:
    """The titles a call's first argument `text` gives, and the title when it is one string: a
    string, a template whose `${...}` parts match anything, or a constant defined as either last
    in `source` (the file up to the call)."""
    m = TITLE.match(text)
    if not m:
        return None, None
    if text[m.end() :].lstrip().startswith("+"):
        # A concatenation: what follows the first part matches anything.
        first, _ = title_pattern(text[: m.end()], source)
        return (re.compile(first.pattern + ".*") if first else None), None
    if m[1] is not None:
        title = str(json.loads(f'"{m[1]}"'))
        return re.compile(re.escape(title)), title
    if m[2] is not None:
        parts = re.split(r"\$\{[^}]*\}", m[2])
        literal = m[2] if len(parts) == 1 else None
        return re.compile(".+".join(re.escape(p) for p in parts)), literal
    defined = re.findall(rf"^\s*const {m[3]}\s*=\s*([\"`].*)", source, re.M)
    return title_pattern(defined[-1], "") if defined else (None, None)


def regions(source: str) -> list[Region]:
    """The `check(...)` calls of interactions.mjs, 1-based. Prettier formats the file, so a call
    closes on the first line at its own indentation that ends `);`."""
    lines = source.splitlines()
    found: list[Region] = []
    for n, line in enumerate(lines):
        m = CALL.match(line)
        if not m:
            continue
        indent = m[1]
        argument = m[2] or (lines[n + 1] if n + 1 < len(lines) else "")
        last = n
        if not line.rstrip().endswith(");"):
            for k in range(n + 1, len(lines)):
                text = lines[k]
                if (
                    text.startswith(indent)
                    and not text[len(indent) :].startswith((" ", "\t"))
                    and text.rstrip().endswith(");")
                ):
                    last = k
                    break
        above = "\n".join(lines[:n])
        found.append(Region(n + 1, last + 1, *title_pattern(argument, above)))
    return found


def statements(source: str) -> list[tuple[int, int]]:
    """The file's top-level statements, 1-based: each from a line at column 0 to the line before
    the next, comments joining the statement after them."""
    lines = source.splitlines()
    starts = [
        n
        for n, line in enumerate(lines)
        if line
        and not line[0].isspace()
        and line[0] not in "})]"
        and not (n and lines[n - 1].startswith(COMMENT))
    ]
    return [
        (s + 1, (starts[i + 1] if i + 1 < len(starts) else len(lines)))
        for i, s in enumerate(starts)
    ]


def body_reasons(
    source: str, changed: list[Hunk], universe: Iterable[str]
) -> tuple[set[str], bool]:
    """The checks whose bodies `changed` (interactions.mjs' hunks) touched, and whether it touched
    the harness: a line in no check and in no top-level block of checks, or in a check whose
    title cannot be followed."""
    calls = regions(source)
    blocks = statements(source)
    titles = [c.removeprefix("ui-test:") for c in universe]
    chosen: set[str] = set()
    harness = False
    for h in changed:
        if h.comment_only():
            continue
        for line in h.new_lines():
            inside = [r for r in calls if r.first <= line <= r.last]
            if not inside:
                block = next((b for b in blocks if b[0] <= line <= b[1]), None)
                inside = [r for r in calls if block and block[0] <= r.first <= block[1]]
            if not inside or any(r.title is None for r in inside):
                harness = True
                continue
            for r in inside:
                pattern = r.title
                if pattern is not None:
                    chosen |= {f"ui-test:{t}" for t in titles if pattern.fullmatch(t)}
    return chosen, harness


NODE_IMPORT: re.Pattern[str] = re.compile(
    r'^import\s*\{([^}]*)\}\s*from\s*"\.\./(src/[^"]+)";', re.M
)


def import_reasons(
    source: str, path: str, changed: list[Hunk], universe: Iterable[str]
) -> tuple[set[str], bool]:
    """body_reasons for a change to `path`, a ui/src file, as interactions.mjs (`source`) runs it
    in Node, where the browser's coverage does not see it: each line that uses a name the file
    imports from `path` counts as changed. Nothing when it imports nothing from there or the
    change is to comments."""
    if all(h.comment_only() for h in changed):
        return set(), False
    names: list[str] = []
    imports: set[int] = set()
    for m in NODE_IMPORT.finditer(source):
        if f"ui/{m[2]}" != path:
            continue
        first = source.count("\n", 0, m.start()) + 1
        imports |= set(range(first, first + m[0].count("\n") + 1))
        for part in m[1].split(","):
            words = part.split()
            if words and words[0] != "type":
                names.append(words[-1])
    if not names:
        return set(), False
    use = re.compile(rf"\b(?:{'|'.join(re.escape(n) for n in names)})\b")
    uses = [
        Hunk(INTERACTIONS, n, 1, n, 1, [text], [text])
        for n, text in enumerate(source.splitlines(), 1)
        if n not in imports and use.search(text)
    ]
    return body_reasons(source, uses, universe)


def interaction_titles(source: str) -> tuple[list[str], list[re.Pattern[str]]]:
    """The checks interactions.mjs has: the titles it spells out, and the patterns of all of
    them (a computed title it cannot follow matches anything)."""
    found = regions(source)
    patterns = [r.title if r.title is not None else re.compile(".*") for r in found]
    return [f"ui-test:{r.literal}" for r in found if r.literal is not None], patterns


def verb_suite() -> dict[str, object]:
    """ui/verbs.json: the verb suite's verbs, layouts and the fixtures of its moment by seat.
    Empty where a checkout has none."""
    try:
        found = json.loads((paths.REPO / VERBS).read_text())
    except (OSError, ValueError):
        return {}
    return found if isinstance(found, dict) else {}


def verb_checks() -> list[str]:
    """The verb suite's checks, by their run-log ids."""
    suite = verb_suite()
    verbs, layouts, moment = (suite.get(k) for k in ("verbs", "layouts", "moment"))
    if not (
        isinstance(verbs, list)
        and isinstance(layouts, list)
        and isinstance(moment, dict)
    ):
        return []
    return [
        f"{VERB_PREFIX}{verb}/{seat}/{layout}"
        for verb in verbs
        for seat in moment
        for layout in layouts
    ]


def verb_fixtures() -> list[str]:
    """The fixtures the verb suite plays: its first moment's, whole and cut, and those of the
    moments its verbs watch besides (`more`), one a seat."""
    suite = verb_suite()
    moment, more = suite.get("moment"), suite.get("more")
    seats = list(moment.values()) if isinstance(moment, dict) else []
    seats += list(more.values()) if isinstance(more, dict) else []
    return sorted({str(f) for s in seats if isinstance(s, dict) for f in s.values()})


def shape_suite() -> dict[str, object]:
    """ui/shapes.json: the shape suite's shapes, each with its fixture by seat, and its layouts.
    Empty where a checkout has none."""
    try:
        found = json.loads((paths.REPO / SHAPES).read_text())
    except (OSError, ValueError):
        return {}
    return found if isinstance(found, dict) else {}


def shape_checks() -> list[str]:
    """The shape suite's checks, by their run-log ids."""
    suite = shape_suite()
    shapes, layouts = suite.get("shapes"), suite.get("layouts")
    if not (isinstance(shapes, dict) and isinstance(layouts, list)):
        return []
    return [
        f"{SHAPE_PREFIX}{shape}/{seat}/{layout}"
        for shape, seats in shapes.items()
        if isinstance(seats, dict)
        for seat in seats
        for layout in layouts
    ]


def shape_fixtures() -> list[str]:
    """The fixtures the shape suite plays: each shape's, one a seat."""
    shapes = shape_suite().get("shapes")
    seats = list(shapes.values()) if isinstance(shapes, dict) else []
    return sorted({str(f) for s in seats if isinstance(s, dict) for f in s.values()})


def scripts(names: list[str]) -> dict[str, list[str]]:
    """Which of `ui-test`'s scripts run, each with the checks of `names` that are its own: a
    verb check's name goes to verbs.mjs, a shape check's to shapes.mjs and any other to
    interactions.mjs, and a script with none does not run. With no names all run every check.
    """
    if not names:
        return {INTERACTIONS: [], VERB_SCRIPT: [], SHAPE_SCRIPT: []}
    suites = {VERB_PREFIX: VERB_SCRIPT, SHAPE_PREFIX: SHAPE_SCRIPT}
    found: dict[str, list[str]] = {INTERACTIONS: [], VERB_SCRIPT: [], SHAPE_SCRIPT: []}
    for name in names:
        script = next(
            (s for p, s in suites.items() if name.startswith(p)), INTERACTIONS
        )
        found[script].append(name)
    return {script: own for script, own in found.items() if own}


def shot_entries(text: str) -> dict[str, object]:
    try:
        return {s["name"]: s for s in json.loads(text)["shots"]}
    except (ValueError, KeyError, TypeError):
        return {}


# The failure record.


@dataclass(frozen=True)
class Record:
    catches: int
    failures: int
    runs: int
    seconds: float

    def key(self) -> tuple[float, float, float]:
        """Catches first, then failures per run, then the cheapest."""
        rate = self.failures / self.runs if self.runs else 0.0
        return (-self.catches, -rate, self.seconds)


def records(
    log: list[dict[str, object]],
    history: dict[str, list[verdicts.Entry]],
    found: Coverage | None,
    since: datetime,
) -> dict[str, Record]:
    """Each check's record from the run log and its bead's verdicts, timed by its mean run in the
    log, or the coverage run's."""
    tallies, _ = runlog.tally(log, history, since)
    measured = found.checks if found else {}
    out: dict[str, Record] = {}
    for check in set(tallies) | set(measured):
        t = tallies.get(check, runlog.Tally())
        timed = measured.get(check, {}).get("seconds")
        seconds = (
            t.seconds / t.runs
            if t.runs
            else float(timed) if isinstance(timed, (int, float)) else DEFAULT_SECONDS
        )
        out[check] = Record(t.verdicts.get("catch", 0), t.failures, t.runs, seconds)
    return out


def order(chosen: Iterable[str], known: Mapping[str, Record]) -> list[str]:
    default = Record(0, 0, 0, DEFAULT_SECONDS)
    return sorted(chosen, key=lambda c: (known.get(c, default).key(), c))


def fit(
    ordered: list[str], known: Mapping[str, Record], left: float, workers: int
) -> tuple[list[str], list[str]]:
    """The checks that fit `left` seconds of `workers` pages at once, in order, and those cut."""
    spent = 0.0
    for n, check in enumerate(ordered):
        seconds = known[check].seconds if check in known else DEFAULT_SECONDS
        if spent + seconds / workers > left:
            return ordered[:n], ordered[n:]
        spent += seconds / workers
    return ordered, []


# Choosing.


@dataclass
class Choice:
    """A lane's chosen checks with why (`reasons`), whether the rest of the box goes to the
    worst records (`fill`), and why coverage could not choose (`fallback`). Only checks of
    `universe`, the lane's checks as they are now, are chosen."""

    universe: list[str]
    fallback: str = ""
    reasons: dict[str, str] = field(default_factory=dict)
    fill: bool = False
    # Whether a Rust or fixture change waits on the step's recordings to choose.
    recorded: bool = False

    def add(self, checks: Iterable[str], reason: str) -> None:
        known = set(self.universe)
        for c in checks:
            if c in known:
                self.reasons.setdefault(c, reason)

    def idle(self) -> bool:
        """Whether nothing is chosen, or can be once the step records its fixtures."""
        return not (self.reasons or self.fill or self.recorded)


def matches(path: str, globs: Iterable[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, g) for g in globs)


def rewritten(path: str, changed: list[str], hunks_now: dict[str, list[Hunk]]) -> bool:
    """Whether `path` changed in more than its comments; a file git does not track yet did."""
    found = hunks_now.get(path)
    return path in changed and (
        found is None or not all(h.comment_only() for h in found)
    )


def choose(
    lane: str,
    changed: list[str],
    hunks_now: dict[str, list[Hunk]],
    universe: list[str],
    found: Coverage | None,
    base: str,
    fallback: str = "",
) -> Choice:
    """A lane's choice from what changed since `base`: the `changed` paths, and `hunks_now`, the
    diff of ui/ from `base` to the working tree, against `found` coverage (None to fall back to
    the records, saying why in `fallback`)."""
    choice = Choice(universe, fallback)
    if any(matches(p, HARNESS[lane]) for p in changed):
        choice.add(universe, "harness")
    if lane == "ui-test" and INTERACTIONS in changed:
        source = (paths.REPO / INTERACTIONS).read_text()
        bodies, harness = body_reasons(
            source, hunks_now.get(INTERACTIONS, []), universe
        )
        choice.add(sorted(bodies), "body")
        if harness:
            choice.add(universe, "harness")
    if lane == "ui-test" and any(
        rewritten(p, changed, hunks_now) for p in (VERB_SCRIPT, *VERB_CASES, VERBS)
    ):
        choice.add((c for c in universe if c.startswith(VERB_PREFIX)), "body")
    if lane == "ui-test" and any(
        rewritten(p, changed, hunks_now) for p in (SHAPE_SCRIPT, *SHAPE_CASES, SHAPES)
    ):
        choice.add((c for c in universe if c.startswith(SHAPE_PREFIX)), "body")
    if lane == "shots" and SHOTS in changed:
        old = shot_entries(checks.git("show", f"{base}:{SHOTS}")) if base else {}
        new = shot_entries((paths.REPO / SHOTS).read_text())
        choice.add((f"shots:{n}" for n, e in new.items() if old.get(n) != e), "body")
    if any(p.startswith("rust/") or p == FIXTURES for p in changed):
        if found is None or found.recordings is None:
            choice.fill = True
        else:
            choice.recorded = True
    sources = {p: h for p, h in hunks_now.items() if p.startswith("ui/src/")}
    if any(p.startswith("ui/src/") and p not in hunks_now for p in changed):
        choice.fill = True  # A new file git does not track yet.
    if sources and found is None:
        choice.fill = True
    elif sources and found is not None:
        coverage_reasons(choice, sources, found, base)
    if lane == "ui-test" and sources:
        source = (paths.REPO / INTERACTIONS).read_text()
        for path, hs in sources.items():
            bodies, harness = import_reasons(source, path, hs, universe)
            choice.add(sorted(bodies), f"import:{path}")
            if harness:
                choice.add(universe, "harness")
    return choice


def coverage_reasons(
    choice: Choice, changed: dict[str, list[Hunk]], found: Coverage, base: str
) -> None:
    """Chooses the checks that ran a changed line, by its line at the coverage's commit; a line
    no check ran, or one changed since that commit, fills the box by record instead."""
    since = diff(found.commit, base, under="ui/src/") if base != found.commit else {}
    covered = found.files()
    for path, hs in changed.items():
        if path not in covered:
            choice.fill = True
            continue
        for h in hs:
            if h.comment_only():
                continue
            lines = [shift(since.get(path, []), n) for n in h.old_lines()]
            hits = found.covering(path, (n for n in lines if n is not None))
            if not hits:
                choice.fill = True
            choice.add(sorted(hits), f"coverage:{path}:{h.new}")


def fixture_reasons(choice: Choice, recorded: Iterable[Path], found: Coverage) -> None:
    """Chooses the checks that load a fixture whose recording differs from the coverage run's."""
    then = found.recordings or {}
    now: dict[str, str] = {}
    for folder in recorded:
        now.update(hashes(folder))
    for name in sorted(n for n, h in now.items() if then.get(n) != h):
        choice.add(
            sorted(c for c, e in found.checks.items() if name in fixtures_of(e)),
            f"fixture:{name}",
        )


def hashes(folder: Path) -> dict[str, str]:
    """Each recorded fixture's hash, by name."""
    return {
        f.stem: hashlib.sha256(f.read_bytes()).hexdigest()
        for f in sorted(folder.glob("*.json"))
    }


def lane_checks(
    lane: str, log: list[dict[str, object]], found: Coverage | None
) -> list[str]:
    """The lane's checks as they are now, but those the prune moved to slow CI alone
    (verdicts.SLOW_ONLY): shots.json's names, or the checks interactions.mjs spells out with
    those the log and coverage name that it can still give, and the verb and shape suites'.
    """
    slow = {s.check for s in verdicts.SLOW_ONLY}
    return [c for c in every_check(lane, log, found) if c not in slow]


def every_check(
    lane: str, log: list[dict[str, object]], found: Coverage | None
) -> list[str]:
    prefix = f"{lane}:"
    if lane == "shots":
        return [f"{prefix}{n}" for n in shot_entries((paths.REPO / SHOTS).read_text())]
    source = (paths.REPO / INTERACTIONS).read_text()
    literal, patterns = interaction_titles(source)
    named = {str(r.get("check")) for r in log if r.get("lane") == lane}
    named |= set(found.checks) if found else set()
    current = {
        c
        for c in named
        if c.startswith(prefix)
        and (
            any(p.fullmatch(c.removeprefix(prefix)) for p in patterns)
            # A check that records itself rather than through check().
            or json.dumps(c.removeprefix(prefix)) in source
        )
    }
    return sorted(current | set(literal)) + verb_checks() + shape_checks()


class Chooser:
    """The choice for one lane's step: made from the diff when the step starts, finished with
    the fixtures it records (`run`), and written into its run-log records after (`label`).
    """

    def __init__(self, lane: str) -> None:
        self.lane: str = lane
        self.clock: checks.Clock = checks.Clock()
        self.started: float = self.clock.now()
        now = datetime.now(timezone.utc)
        self.since: datetime = now - timedelta(days=RECORD_DAYS)
        self.log: list[dict[str, object]] = runlog.read_log(self.since)
        loaded = Coverage.load()
        fallback = stale(loaded, now)
        self.coverage: Coverage | None = None if fallback else loaded
        base = checks.branch_base()
        self.choice: Choice = choose(
            lane,
            checks.changed_paths(base) or [],
            diff(base) if base else {},
            lane_checks(lane, self.log, self.coverage),
            self.coverage,
            base,
            fallback,
        )
        self.cut: list[str] = []

    def run(self, recorded: Iterable[Path], only: list[str]) -> list[str]:
        """What to run, by name, once the step recorded its fixtures into `recorded`: `only`, a
        rerun's, or the chosen checks that fit the box. Prints what it chose and why."""
        choice = self.choice
        if choice.recorded and self.coverage is not None:
            fixture_reasons(choice, recorded, self.coverage)
        known = records(self.log, verdicts.cached_history(), self.coverage, self.since)
        ranked = order(choice.reasons, known)
        if choice.fill:
            rest = order((c for c in choice.universe if c not in choice.reasons), known)
            choice.add(rest, "record")
            ranked += rest
        if only:
            return only
        if choice.fallback:
            print(f"{self.lane}: choosing by failure record: {choice.fallback}")
        left = BOX - STARTUP - (self.clock.now() - self.started)
        workers = int(os.environ.get("BERYL_UI_WORKERS") or WORKERS)
        chosen, self.cut = fit(ranked, known, left, workers)
        why: dict[str, int] = {}
        for c in chosen:
            kind = choice.reasons[c].split(":")[0]
            why[kind] = why.get(kind, 0) + 1
        print(
            f"{self.lane}: running {len(chosen)} of {len(choice.universe)} checks"
            f" ({', '.join(f'{n} by {k}' for k, n in why.items()) or 'none chosen'});"
            f" {len(self.cut)} cut by the {max(left, 0):.0f} s left"
        )
        return [c.removeprefix(f"{self.lane}:") for c in chosen]

    def label(self) -> None:
        """Writes each check's reason, and a `skip` for each one the box cut, into the step's
        results (runlog.RESULTS_ENV)."""
        path = os.environ.get(runlog.RESULTS_ENV)
        if path:
            runlog.label(Path(path), self.choice.reasons, self.cut)
