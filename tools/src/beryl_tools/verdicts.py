"""Verdicts and quarantine (docs/project-50k.md, K18 and K21): every failure of a check is given
a verdict the tool infers, and a check that flakes twice in seven days is quarantined.

- **flaky**: the check failed and then passed on the same tree, as on the rerun `run_lane` gives
  a failed step (checks.py).
- **stale** or **catch**: a check that failed passed in a later run from the same place (the
  branch, or slow CI), on another tree. The paths that changed between the two trees say which
  (for slow CI, those the Steward's commits changed, when a Steward fixed it):
  only the check's own side ([`CHECK_SIDE`]) is a stale test; only product code is a caught
  bug; both is **mixed**, which prints a line asking for `by verdict <check> stale|catch` and
  blocks nothing. The same command corrects any verdict.
- **miss**: slow CI failed a check that the fast check did not choose (selection.py), at the
  commit bisection blamed (bisect.py). It is recorded beside the stale or catch verdict that the
  fix later brings, not instead of it.

Each verdict is a comment on the check's bead (one closed bead per check that has ever failed,
labeled `ci-check`), so the record is in git, shared by every machine and survives a cache wipe;
passes and timings stay in the run log (runlog.py). Quarantine reads the verdicts from those
beads, so it is the same on every host. A quarantined check still runs and is logged, but its
failure fails no run and so files no bead. A check on the [`PENDING`] list is a banked UI report
the stage must pass one day: it runs and is logged as pending-fail or pending-pass, and fails no
run either.

Nothing here fails a check: a git or bd call that fails is reported and the run goes on.
"""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

from . import paths, system

QUARANTINE_FLAKES = 2
QUARANTINE_DAYS = 7
# An unresolved failure whose check has not passed since is forgotten after this long: the
# check was deleted or renamed, or the diff to a pass would be too wide to say anything.
UNRESOLVED_DAYS = 14
# Set to "off" to keep verdicts out of the beads (the tests do).
BEADS_ENV = "BERYL_CI_BEADS"
# The verdicts `by verdict` sets; `mixed` is only ever inferred, as a question.
VERDICTS = ("flaky", "stale", "catch", "miss")


def warn(e: BaseException | str) -> None:
    print(f"verdicts: {e}", file=sys.stderr)


# What a fix touched, by lane.


@dataclass(frozen=True)
class Side:
    """A lane's checks' own side: the paths (fnmatch globs, `*` crossing folders) that are only
    checks, and in Rust files the items that are: `tests` for `#[cfg(test)]` items and a card's
    `SCENARIOS`, `scenarios` for `SCENARIOS` alone."""

    globs: tuple[str, ...]
    rust: str = ""


# A fix that touched only its check's side made a stale test pass again; one that touched only
# product code caught a bug. One table, so a lane's rule is read in one place.
CHECK_SIDE: dict[str, Side] = {
    "rust": Side(("rust/*/tests/*", "rust/.config/nextest.toml"), rust="tests"),
    "ui-test": Side(("ui/scripts/*", "ui/fixtures.json")),
    "shots": Side(("ui/scripts/*", "ui/shots.json", "ui/fixtures.json")),
    "oracle": Side(("data/oracle-divergences.json",), rust="scenarios"),
    "python": Side(("tools/tests/*",)),
    "names": Side(("data/card-names-allowlist.json",)),
}
# Neither side of any check: prose and the beads.
NEUTRAL = ("docs/*", ".beads/*", "*.md")


@dataclass(frozen=True)
class Pending:
    """A banked UI report (plan K30): the check that shows it, the board in beryl_cli's
    boards.rs that reproduces it, and the report's bead. The bugfix skill names the board, its
    fixture and its shot `banked-<bead>`, so the check is `shots:banked-<bead>`. The stage's
    bead that makes it pass removes the entry, and the check becomes a normal one."""

    check: str
    board: str
    bead: str


PENDING: tuple[Pending, ...] = ()


def pending_checks() -> set[str]:
    return {p.check for p in PENDING}


@dataclass(frozen=True)
class SlowOnly:
    """A check the pruning rule moved to slow CI alone (prune.py: never failed, and in the
    costliest tenth of the fast check by seconds per run), on `date`: checks.fast() leaves it out
    and slow CI runs it as before. `seconds` (a run) and `runs` are its fast-check record when it
    moved, which keeps it in the ranking of later prunes."""

    check: str
    date: str
    seconds: float
    runs: int


SLOW_ONLY: tuple[SlowOnly, ...] = ()


def matches(path: str, globs: Iterable[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, g) for g in globs)


ITEM_START: dict[str, re.Pattern[str]] = {
    "scenarios": re.compile(r"^\s*(pub(\([a-z]+\))? )?(const|static) SCENARIOS\b"),
    "tests": re.compile(
        r"^\s*(#\[cfg\(test\)\]|(pub(\([a-z]+\))? )?(const|static) SCENARIOS\b)"
    ),
}


def item_end(lines: list[str], start: int) -> int:
    """The last line of the item starting at `start` (0-based), past its attributes: where its
    brackets close on a `}` or a `;`. Strings and line comments are skipped."""
    i = start
    while i < len(lines) - 1 and lines[i].lstrip().startswith("#["):
        i += 1
    depth = 0
    for n in range(i, len(lines)):
        line = lines[n]
        k = 0
        while k < len(line):
            ch = line[k]
            if line.startswith("//", k):
                break
            if ch == '"':
                k += 1
                while k < len(line) and line[k] != '"':
                    k += 2 if line[k] == "\\" else 1
            elif ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
                if depth == 0 and ch == "}":
                    return n
            elif ch == ";" and depth == 0:
                return n
            k += 1
    return len(lines) - 1


def regions(text: str, kind: str) -> list[tuple[int, int]]:
    """The 1-based line ranges of a Rust file's `kind` items (`Side.rust`), with the doc
    comments and attributes above them."""
    lines = text.splitlines()
    found: list[tuple[int, int]] = []
    n = 0
    while n < len(lines):
        if ITEM_START[kind].match(lines[n]):
            first = n
            while first > 0 and lines[first - 1].lstrip().startswith(("///", "#[")):
                first -= 1
            end = item_end(lines, n)
            found.append((first + 1, end + 1))
            n = end + 1
        else:
            n += 1
    return found


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=paths.REPO, capture_output=True, text=True, check=False
    )


HUNK: re.Pattern[str] = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def changed_lines(old: str, new: str, path: str) -> tuple[list[int], list[int]]:
    """The lines of `path` the diff from tree `old` to tree `new` removes and adds."""
    out = git("diff", "-U0", "--no-renames", "--no-color", old, new, "--", path).stdout
    removed: list[int] = []
    added: list[int] = []
    for m in map(HUNK.match, out.splitlines()):
        if not m:
            continue
        a, b, c, d = m.groups()
        removed += range(int(a), int(a) + (1 if b is None else int(b)))
        added += range(int(c), int(c) + (1 if d is None else int(d)))
    return removed, added


def rust_sides(old: str, new: str, path: str, lane: str) -> set[str]:
    """Which sides a Rust file's changed lines are on, for a check of `lane`."""
    removed, added = changed_lines(old, new, path)
    own = CHECK_SIDE.get(lane, Side(())).rust
    sides: set[str] = set()
    for tree, lines in ((old, removed), (new, added)):
        if not lines:
            continue
        text = git("show", f"{tree}:{path}").stdout
        tests = regions(text, "tests")
        mine = regions(text, own) if own else []
        for line in lines:
            if any(a <= line <= b for a, b in mine):
                sides.add("check")
            elif not any(a <= line <= b for a, b in tests):
                sides.add("product")
            # Another lane's test code is no side of this check.
    return sides


def sides(lane: str, old: str, new: str) -> set[str] | None:
    """Which sides of a `lane` check the change from tree `old` to tree `new` touched: `check`,
    `product`, both or neither; None when git cannot say (a tree gone)."""
    diff = git("diff", "--name-only", "--no-renames", old, new)
    if diff.returncode:
        return None
    own = CHECK_SIDE.get(lane, Side(()))
    others = [g for name, s in CHECK_SIDE.items() if name != lane for g in s.globs]
    touched: set[str] = set()
    for path in diff.stdout.splitlines():
        if matches(path, NEUTRAL):
            continue
        if matches(path, own.globs):
            touched.add("check")
        elif matches(path, others):
            continue
        elif path.endswith(".rs"):
            touched |= rust_sides(old, new, path, lane)
        else:
            touched.add("product")
    return touched


def classify(lane: str, old: str, new: str) -> str | None:
    """The verdict on a failure at tree `old` that passed at tree `new`."""
    if old == new:
        return "flaky"
    touched = sides(lane, old, new)
    return None if touched is None else judge(touched)


def classify_commits(lane: str, commits: Iterable[str]) -> str | None:
    """The verdict on a failure that `commits` fixed, judged by what they alone touched: on
    staging, others' work lands between a failure and the pass, and would read as both sides.
    """
    touched: set[str] = set()
    for c in commits:
        found = sides(lane, f"{c}^", c)
        if found is None:
            return None
        touched |= found
    return judge(touched)


def judge(touched: set[str]) -> str:
    """The verdict on a fix that touched these sides of a check."""
    if touched == {"check"}:
        return "stale"
    if touched == {"product"}:
        return "catch"
    return "mixed" if touched else "flaky"


# The record: one closed bead per check that has ever failed, one comment per verdict.

Entry = dict[str, object]
DESCRIPTION = (
    "The failures of this check that reached a verdict (docs/project-50k.md, K18), one JSON"
    " comment each, written by `by check` and `by ci` (tools/src/beryl_tools/verdicts.py)."
    " Created closed: nothing schedules it."
)


def bead_id(check: str) -> str:
    """The check's bead, named after the check so that every machine finds the same one and a
    second create of it fails instead of making a twin."""
    return "by-ci-" + hashlib.sha256(check.encode()).hexdigest()[:10]


def now() -> datetime:
    return datetime.now(timezone.utc)


def stamp(when: datetime) -> str:
    return when.isoformat(timespec="seconds")


def entries(comments: Iterable[object]) -> list[Entry]:
    """A check bead's verdicts, oldest first, each corrected by any later `corrects` comment
    naming it; comments that are not verdicts are skipped."""
    found: list[Entry] = []
    by_id: dict[str, Entry] = {}
    for c in comments:
        if not isinstance(c, dict):
            continue
        try:
            entry = json.loads(str(c.get("text", "")))
        except json.JSONDecodeError:
            continue
        if not isinstance(entry, dict) or not isinstance(entry.get("verdict"), str):
            continue
        if "corrects" in entry:
            corrected = by_id.get(str(entry["corrects"]))
            if corrected is not None:
                corrected["verdict"] = entry["verdict"]
                corrected["corrected"] = True
            continue
        entry["comment"] = str(c.get("id", ""))
        by_id[str(entry["comment"])] = entry
        found.append(entry)
    return found


def parse_json(out: str) -> object:
    """The JSON array bd printed, past any warning before it."""
    start = out.find("[")
    if start < 0:
        return []
    try:
        return json.JSONDecoder().raw_decode(out[start:])[0]
    except json.JSONDecodeError:
        return []


class Beads:
    """The `ci-check` beads, through bd, best effort: off when $BERYL_CI_BEADS is `off` or bd
    is not installed, and a call that fails is reported and returns nothing."""

    def bd(self, *args: str) -> tuple[int, str]:
        if os.environ.get(BEADS_ENV) == "off" or shutil.which("bd") is None:
            return -1, ""
        try:
            run = subprocess.run(
                ["bd", *args],
                cwd=paths.REPO,
                capture_output=True,
                text=True,
                timeout=120,
                check=False,
            )
        except (OSError, subprocess.SubprocessError) as e:
            warn(f"bd {args[0]}: {e}")
            return -1, ""
        return run.returncode, run.stdout

    def history(self, checks: Iterable[str]) -> dict[str, list[Entry]]:
        """Each check's verdicts, for the checks that have a bead."""
        ids = sorted({bead_id(c) for c in checks})
        found: dict[str, list[Entry]] = {}
        for i in range(0, len(ids), 100):
            # Exits 1 when an id has no bead, still printing the ones that do.
            _, out = self.bd("show", *ids[i : i + 100], "--json", "--include-comments")
            issues = parse_json(out)
            for issue in issues if isinstance(issues, list) else []:
                if isinstance(issue, dict):
                    found[str(issue.get("title"))] = entries(
                        issue.get("comments") or []
                    )
        return found

    def checks(self) -> list[str]:
        """Every check that has a bead."""
        _, out = self.bd("list", "--label", "ci-check", "--all", "-n", "0", "--json")
        issues = parse_json(out)
        return [
            str(i["title"])
            for i in (issues if isinstance(issues, list) else [])
            if isinstance(i, dict) and "title" in i
        ]

    def record(self, check: str, lane: str, entry: Entry) -> bool:
        bead = bead_id(check)
        # Fails harmlessly when the bead exists.
        self.bd(
            "create",
            "--id",
            bead,
            "--title",
            check,
            "--status",
            "closed",
            "--labels",
            f"ci-check,ci-lane:{lane}",
            "--description",
            DESCRIPTION,
            "--silent",
        )
        code, _ = self.bd("comments", "add", bead, json.dumps(entry))
        if code:
            warn(f"{check}: verdict {entry.get('verdict')} not recorded in {bead}")
        return code == 0

    def steward(self, commits: set[str]) -> tuple[str, list[str]]:
        """The Steward bead that delivered one of `commits`, with those of its commits among
        them; empty when none did."""
        return self.delivered(commits, "--label", "role:steward")

    def delivered(self, commits: set[str], *only: str) -> tuple[str, list[str]]:
        """The bead whose Enact metadata lists one of `commits` as delivered, of those `bd list`
        selects with `only`, with those of its commits among them; empty when none does.
        """
        bead, theirs = self.delivering(commits, *only)
        return bead, [c for c in theirs if c in commits]

    def delivering(self, commits: set[str], *only: str) -> tuple[str, list[str]]:
        """As `delivered`, but with every commit the bead delivered."""
        if not commits:
            return "", []
        _, out = self.bd("list", *only, "--all", "-n", "0", "--json")
        beads = parse_json(out)
        for b in beads if isinstance(beads, list) else []:
            meta = b.get("metadata") if isinstance(b, dict) else None
            delivered = (
                str(meta.get("enact_commits", "")) if isinstance(meta, dict) else ""
            ).split()
            if any(c in commits for c in delivered):
                return str(b.get("id", "")), delivered
        return "", []


STORE: Beads = Beads()


def history_path() -> Path:
    return paths.cache_root() / "ci" / "history.json"


def save_history(history: dict[str, list[Entry]]) -> None:
    """Keeps the checks' verdicts for the fast check's ordering (selection.py), which cannot wait
    on bd: `by ci-report` and the nightly write it."""
    try:
        path = history_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".tmp")
        tmp.write_text(json.dumps(history))
        tmp.replace(path)
    except OSError as e:
        warn(e)


def cached_history() -> dict[str, list[Entry]]:
    """The verdicts `save_history` last kept, or none."""
    try:
        found = json.loads(history_path().read_text())
    except (OSError, ValueError):
        return {}
    return found if isinstance(found, dict) else {}


def refresh_history() -> int:
    """Fetches every check's verdicts from the beads and keeps them (`save_history`)."""
    history = STORE.history(STORE.checks())
    save_history(history)
    print(f"verdicts: kept the verdicts of {len(history)} checks in {history_path()}")
    return 0


# Quarantine.


def flakes(found: Iterable[Entry], at: datetime) -> list[str]:
    """The times of the flaky verdicts in the quarantine window before `at`. A verdict
    backfilled from the slow-CI history before the run log (`backfilled`) quarantines nothing:
    its race was fixed before the rerun existed, and quarantine is for flakes seen since.
    """
    since = at - timedelta(days=QUARANTINE_DAYS)
    times: list[str] = []
    for e in found:
        if e.get("backfilled"):
            continue
        try:
            when = datetime.fromisoformat(str(e.get("time")))
        except ValueError:
            continue
        if e.get("verdict") == "flaky" and since <= when <= at:
            times.append(str(e["time"]))
    return times


def quarantine(history: dict[str, list[Entry]]) -> dict[str, list[str]]:
    """The quarantined checks, each with its flakes' times."""
    at = now()
    found = {check: flakes(e, at) for check, e in history.items()}
    return {c: t for c, t in sorted(found.items()) if len(t) >= QUARANTINE_FLAKES}


def excused(records: list[dict[str, object]]) -> list[tuple[str, str]]:
    """Why a failed step's failure fails no run: (`quarantined` or `pending`, check) for each
    check it failed; none unless each is quarantined or pending."""
    failing = [str(r["check"]) for r in records if r["outcome"] == "fail"]
    pending = [str(r["check"]) for r in records if r["outcome"] == "pending-fail"]
    if not failing and not pending:
        return []
    held = quarantine(STORE.history(failing)) if failing else {}
    if any(c not in held for c in failing):
        return []
    return [("quarantined", c) for c in failing] + [("pending", c) for c in pending]


# Settling a run's failures.


def unresolved_path() -> Path:
    return paths.cache_root() / "ci" / "unresolved.json"


def settle(
    records: list[dict[str, object]],
    script: str,
    place: str,
    tree: str,
    commit: str,
    host: str,
) -> list[str]:
    """Gives a verdict to each failure this run resolves: its flakes, and the earlier failures
    from the same `place` of checks that now pass. Keeps the failures that stand for a later
    run, records each verdict on its check's bead, and returns a line for each."""
    found: list[tuple[str, str, Entry]] = []
    flaked = {str(r["check"]) for r in records if r.get("flaky")}

    def entry(failed: dict[str, object], verdict: str) -> Entry:
        same = failed.get("commit") == commit
        return {
            "time": failed.get("time"),
            "host": failed.get("host", host),
            "commit": failed.get("commit"),
            "script": script,
            "lane": failed.get("lane"),
            "verdict": verdict,
            "fixed": f"tree {tree}" if same else commit,
            "seconds": failed.get("seconds"),
        }

    for r in records:
        if r.get("flaky"):
            found.append((str(r["check"]), str(r["lane"]), entry(r, "flaky")))
    path = unresolved_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    with system.file_lock(path.with_suffix(".lock")):
        try:
            state = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            state = {}
        expiry = stamp(now() - timedelta(days=UNRESOLVED_DAYS))
        state = {k: v for k, v in state.items() if str(v.get("time")) >= expiry}
        for r in records:
            key = f"{script}\t{place}\t{r['check']}"
            if r["outcome"] != "pass":
                continue
            failed = state.pop(key, None)
            if failed is None or r["check"] in flaked or not tree:
                continue
            verdict = classify(str(r["lane"]), str(failed.get("tree")), tree)
            if verdict:
                found.append((str(r["check"]), str(r["lane"]), entry(failed, verdict)))
        for r in records:
            if r["outcome"] == "fail" and not r.get("flaky") and tree:
                state[f"{script}\t{place}\t{r['check']}"] = {
                    k: r.get(k)
                    for k in ("time", "host", "commit", "tree", "lane", "seconds")
                }
        path.write_text(json.dumps(state, indent=1, sort_keys=True))
    lines: list[str] = []
    for check, lane, e in found:
        if script == "ci" and e["verdict"] != "flaky" and e["commit"] != commit:
            fixes = git("rev-list", f"{e['commit']}..{commit}").stdout.split()
            steward, delivered = STORE.steward(set(fixes))
            if steward:
                e["steward"] = steward
                e["verdict"] = classify_commits(lane, delivered) or e["verdict"]
        STORE.record(check, lane, e)
        line = f"verdict: {lane} {check} {e['verdict']}"
        if e["verdict"] == "mixed":
            line += (
                ": the fix touched the check and the product;"
                f" say which with `just verdict '{check}' stale|catch`"
            )
        lines.append(line)
    return lines


# `by verdict`.


def correct(args: argparse.Namespace) -> int:
    """Corrects the latest verdict on a check, with a comment naming the one it corrects."""
    found = STORE.history([args.check]).get(args.check, [])
    if not found:
        print(f"verdict: {args.check} has no verdict to correct", file=sys.stderr)
        return 1
    last = found[-1]
    fix = {
        "corrects": last["comment"],
        "verdict": args.verdict,
        "time": stamp(now()),
    }
    lane = str(last.get("lane") or args.check.split(":")[0])
    if not STORE.record(args.check, lane, fix):
        return 1
    print(
        f"verdict: {args.check} at {last.get('commit')}: {last.get('verdict')} -> {args.verdict}"
    )
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "verdict", help="correct the latest verdict on a check's failure"
    )
    p.add_argument("check", help="the check's id, as `by ci-report` prints it")
    p.add_argument("verdict", choices=VERDICTS)
    p.set_defaults(func=correct)
    p = sub.add_parser(
        "ci-history",
        help="keep the checks' verdicts from the beads for the fast check's ordering",
    )
    p.set_defaults(func=lambda args: refresh_history())
