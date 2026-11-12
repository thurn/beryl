import json
import os
import re
import subprocess
import sys
import time
from collections.abc import Iterable
from datetime import datetime, timezone
from pathlib import Path

import pytest

from beryl_tools import bisect, checks, runlog, verdicts

# A browser lane's step over a repository whose `version` file counts its commits: "plays a
# land" and "casts a spell" fail from version 3, "draws a card" from version 5, and "untaps"
# only outside the bisection's tree, where the untracked `ci-only` marker lies. With $HANG set,
# version 3 hangs. Each run's arguments go to $FAKE_RUNS.
STEP = """
import json, os, pathlib, sys, time
version = int(pathlib.Path("version").read_text())
starts = {"plays a land": 3, "casts a spell": 3, "draws a card": 5}
titles = sys.argv[1:] or [*starts, "untaps"]
with open(os.environ["FAKE_RUNS"], "a") as f:
    f.write(json.dumps([version, titles]) + "\\n")
if os.environ.get("HANG") and version == 3:
    time.sleep(600)
failed = []
for title in titles:
    fails = version >= starts[title] if title in starts else pathlib.Path("ci-only").exists()
    with open(os.environ["BERYL_CHECK_RESULTS"], "a") as f:
        outcome = "fail" if fails else "pass"
        f.write(json.dumps({"check": "ui-test:" + title, "outcome": outcome, "seconds": 0}))
        f.write("\\n")
    if fails:
        print("FAILED: " + title)
        failed.append(title)
sys.exit(1 if failed else 0)
"""


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, capture_output=True, text=True, check=True
    ).stdout.strip()


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Master at version 0, then five commits to version 5 checked out, with the run log of the
    fake step in $FAKE_RUNS."""
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q", "-b", "staging")
    git(repo, "config", "user.email", "ci@example.com")
    git(repo, "config", "user.name", "CI")
    (repo / ".gitignore").write_text("ci-only\n.venv\n")
    for version in range(6):
        (repo / "version").write_text(f"{version}\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", f"feat: version {version}")
        if version == 0:
            git(repo, "update-ref", "refs/remotes/origin/master", "HEAD")
    (repo / "ci-only").write_text("")
    monkeypatch.setenv("FAKE_RUNS", str(tmp_path / "runs"))
    return repo


def lane(repo: Path) -> checks.Lane:
    """The fake step, run by an interpreter in the repository's ignored `.venv`, as `by ci`'s
    is: the bisection's tree has none."""
    venv = repo / ".venv"
    if not venv.exists():
        venv.mkdir()
        # The base interpreter: a Windows venv's python.exe is a launcher that runs only
        # beside its own pyvenv.cfg.
        base = vars(sys).get("_base_executable", sys.executable)
        (venv / "python").symlink_to(base)
    python = str(venv / "python")
    return checks.Lane("ui-test").step(
        "ui-test", [python, "-c", STEP], repo, rerun=checks.failed_titles
    )


def probes(tmp_path: Path) -> list[tuple[int, list[str]]]:
    """The fake step's runs in the bisection's tree, after the run's own two."""
    runs = [json.loads(line) for line in (tmp_path / "runs").read_text().splitlines()]
    return [(v, titles) for v, titles in runs[2:]]


def run(repo: Path, capsys: pytest.CaptureFixture[str]) -> list[str]:
    """Slow CI on the fake lane, failed and bisected; the log's lines."""
    report = checks.run_all([lane(repo)], "ci", 60, strict=True)
    assert report.code == 1
    capsys.readouterr()
    bisect.after(report.outcomes, time.monotonic(), repo)
    return capsys.readouterr().out.splitlines()


def test_bisection_names_each_culprit_flake_and_record_last(
    repo: Path, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    lines = run(repo, capsys)
    commits = git(repo, "rev-list", "--reverse", "origin/master..HEAD").split()
    third, fifth = commits[2][:10], commits[4][:10]
    answer = [
        line
        for line in lines
        if line.startswith(("bisect: ui-test:", "miss: ", "record: "))
    ]
    assert lines[-len(answer) :] == answer
    since = "(5 commits since master)"
    for title, culprit, version in (
        ("plays a land", third, 3),
        ("casts a spell", third, 3),
        ("draws a card", fifth, 5),
    ):
        said = f"bisect: ui-test:{title}  culprit {culprit} feat: version {version}  {since}"
        at = lines.index(said)
        # No fast check of the culprit is in this run log.
        assert lines[at + 1].startswith(f"miss: ui-test:{title} unknown (no fast check")
        assert lines[at + 2] == (
            f"record: ui-test:{title}  runs 2, failures 2, flaky 0, stale 0, catch 0,"
            " miss 0 (30 days)"
        )
    assert (
        "bisect: ui-test:untaps  flaky: passed at the tip on its bisection rerun"
        in lines
    )
    assert len(answer) <= bisect.LINES
    # The tip with all four, then the halves: the two with one culprit always run together.
    found = probes(tmp_path)
    assert found[0] == (5, ["plays a land", "casts a spell", "draws a card", "untaps"])
    for _, titles in found:
        assert ("plays a land" in titles) == ("casts a spell" in titles)
    assert len(found) == 4
    # Each probe's checks reached the run log, with the probe's commit.
    records = [
        json.loads(line)
        for line in checks.runlog.log_path().read_text().splitlines()
        if json.loads(line)["script"] == "bisect"
    ]
    assert {r["commit"][:10] for r in records} == {
        c[:10] for c in (commits[4], commits[2], commits[1], commits[3])
    }


def test_a_culprit_at_the_first_commit_is_checked_against_master(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    git(repo, "update-ref", "refs/remotes/origin/master", "HEAD~1")
    lines = run(repo, capsys)
    tip = git(repo, "rev-parse", "HEAD")[:10]
    master = git(repo, "rev-parse", "HEAD~1")[:10]
    blameless = f"fails at {master} too: no commit since master to blame"
    assert f"bisect: ui-test:plays a land  {blameless}" in lines
    assert f"bisect: ui-test:casts a spell  {blameless}" in lines
    assert (
        f"bisect: ui-test:draws a card  culprit {tip} feat: version 5  (1 commit since"
        " master)" in lines
    )


def test_out_of_time_reports_the_range_left(
    repo: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(bisect, "time_left", lambda *args: 0.0)
    lines = run(repo, capsys)
    commits = git(repo, "rev-list", "--reverse", "origin/master..HEAD").split()
    span = f"{commits[0][:10]}..{commits[4][:10]}"
    said = f"out of time: culprit among 5 commits {span}  (5 commits since master)"
    for title in ("plays a land", "casts a spell", "draws a card", "untaps"):
        assert f"bisect: ui-test:{title}  {said}" in lines


def test_a_probe_that_hangs_dies_at_the_deadline_and_keeps_its_range(
    repo: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(bisect, "time_left", lambda *args: 3.0)
    monkeypatch.setattr(bisect, "LEAST_PROBE", 0.0)
    monkeypatch.setenv("HANG", "1")
    report = checks.run_all([lane(repo)], "ci", 60, strict=True)
    capsys.readouterr()
    bisect.after(report.outcomes, time.monotonic(), repo)
    lines = capsys.readouterr().out.splitlines()
    commits = git(repo, "rev-list", "--reverse", "origin/master..HEAD").split()
    third = commits[2][:10]
    # However far it got before the version that hangs, the range still holds the culprit.
    said = [line for line in lines if line.startswith("bisect: ui-test:plays a land  ")]
    assert len(said) == 1 and "out of time: culprit among" in said[0]
    span = re.search(r"commits (\w+)\.\.(\w+)  ", said[0])
    assert span
    first, last = span.groups()
    order = [c[:10] for c in commits]
    assert order.index(first) <= order.index(third) <= order.index(last)


@pytest.fixture
def deadline(monkeypatch: pytest.MonkeyPatch) -> None:
    """A budget of 0 s, which every run is over, but a deadline the step makes."""
    monkeypatch.setenv("CHECK_DEADLINE_S", "60")


def over_budget_lane() -> checks.Lane:
    return checks.Lane("slow").step("true", [sys.executable, "-c", "pass"])


def test_over_budget_under_load_reruns_once_then_is_excused(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch, deadline: None
) -> None:
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (40.0, 40.0, 40.0))
    assert checks.slow_run([over_budget_lane()], 0, time.monotonic()) == 0
    out = capsys.readouterr().out
    assert "ci: over budget under load: rerunning the whole run once" in out
    assert out.count("ci: passed in") == 2
    assert out.rstrip().endswith("excused: over budget under load")


def test_over_budget_on_a_cold_cache_reruns_and_still_fails_when_quiet(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch, deadline: None
) -> None:
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (2.0, 2.0, 2.0))
    monkeypatch.setattr(checks.warm, "ensure", lambda: True)
    cold = checks.Lane("build", warm=True).step("true", [sys.executable, "-c", "pass"])
    assert checks.slow_run([cold], 0, time.monotonic()) == 1
    out = capsys.readouterr().out
    assert "ci: over budget on a cold cache: rerunning the whole run once" in out
    assert "excused" not in out


def test_over_budget_on_a_quiet_warm_machine_fails_without_a_rerun(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch, deadline: None
) -> None:
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (2.0, 2.0, 2.0))
    assert checks.slow_run([over_budget_lane()], 0, time.monotonic()) == 1
    out = capsys.readouterr().out
    assert "rerunning" not in out and "excused" not in out


def test_over_budget_under_load_with_no_time_to_rerun_is_excused(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch, deadline: None
) -> None:
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (40.0, 40.0, 40.0))
    # Time for the run, which asks first, and none left after it for a rerun.
    left = iter([600.0])
    monkeypatch.setattr(bisect, "time_left", lambda *args: next(left, 0.0))
    assert checks.slow_run([over_budget_lane()], 0, time.monotonic()) == 0
    out = capsys.readouterr().out
    assert "rerunning" not in out
    assert "excused: over budget under load, with no time left to rerun" in out


def test_a_rerun_under_load_cut_off_by_the_wall_limit_is_excused(
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    deadline: None,
    tmp_path: Path,
) -> None:
    # by-5fnf: the first run passed over budget; its rerun ran into Enact's wall clock.
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (40.0, 40.0, 40.0))
    monkeypatch.setattr(bisect, "after", lambda *args: pytest.fail("bisected"))
    # The first run's limit, the gate on rerunning, then the rerun's limit, all but spent.
    left = iter([600.0, 600.0, 1.0])
    monkeypatch.setattr(bisect, "time_left", lambda *args: next(left, 0.0))
    # The step passes at once the first time and outlasts the wall limit the second.
    count = tmp_path / "runs"
    script = (
        "import pathlib, sys, time; p = pathlib.Path(sys.argv[1]);"
        " n = int(p.read_text()) if p.exists() else 0; p.write_text(str(n + 1));"
        " time.sleep(30 * n)"
    )
    lane = checks.Lane("slow").step("sleep", [sys.executable, "-c", script, str(count)])
    started = time.monotonic()
    assert checks.slow_run([lane], 0, started) == 0
    assert time.monotonic() - started < 20
    out = capsys.readouterr().out
    assert "ci: over budget under load: rerunning the whole run once" in out
    assert "the wall limit of 1 s before Enact's kill" in out
    assert "excused: the rerun under load reached the wall limit (slow)" in out


def test_a_wall_limit_kill_on_a_quiet_machine_still_fails(
    monkeypatch: pytest.MonkeyPatch, deadline: None
) -> None:
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (2.0, 2.0, 2.0))
    monkeypatch.setattr(bisect, "time_left", lambda *args: 1.0)
    monkeypatch.setattr(bisect, "after", lambda *args: None)
    lane = checks.Lane("slow").step(
        "sleep", [sys.executable, "-c", "import time; time.sleep(30)"]
    )
    assert checks.slow_run([lane], 300, time.monotonic()) == 1


def test_narrowing_arguments_name_their_checks() -> None:
    assert bisect.names("plays a land", "ui-test:plays a land")
    assert not bisect.names("plays a land", "ui-test:plays a land twice")
    assert bisect.names(
        "beryl_rules::t combat::blocks", "rust:beryl_rules::t::combat::blocks"
    )
    assert bisect.names("verb:fly/yours/desktop", "verb:fly/yours/desktop")
    assert not bisect.names("verb:fly/yours/desktop", "verb:fly/theirs/desktop")
    assert bisect.names("Brainstorm", "oracle:Brainstorm/draws three/xmage")
    assert not bisect.names("Brainstorm", "oracle:Brainstorms/x/xmage")


class Store(verdicts.Beads):
    """The ci-check beads and the Enact beads' delivered commits, in memory."""

    def __init__(self, delivered: dict[str, list[str]]) -> None:
        self.delivered_by = delivered
        self.recorded: list[tuple[str, str, verdicts.Entry]] = []

    def history(self, checks: Iterable[str]) -> dict[str, list[verdicts.Entry]]:
        found: dict[str, list[verdicts.Entry]] = {}
        for check, _, entry in self.recorded:
            if check in checks:
                found.setdefault(check, []).append(dict(entry))
        return found

    def record(self, check: str, lane: str, entry: verdicts.Entry) -> bool:
        self.recorded.append((check, lane, dict(entry)))
        return True

    def delivering(self, commits: set[str], *only: str) -> tuple[str, list[str]]:
        for bead, delivered in self.delivered_by.items():
            if commits & set(delivered):
                return bead, delivered
        return "", []


def fast_check(commit: str, *results: tuple[str, str, str]) -> None:
    """A fast check's records at `commit` in the run log: (check, outcome, chosen) each."""
    when = datetime.now(timezone.utc).isoformat(timespec="seconds")
    records: list[dict[str, object]] = []
    for check, outcome, chosen in results:
        record: dict[str, object] = {
            "time": when,
            "commit": commit,
            "tree": "",
            "script": "check",
            "lane": check.split(":")[0],
            "check": check,
            "outcome": outcome,
            "seconds": 1.0,
            "chosen": chosen,
        }
        records.append(record)
    runlog.append(records)


def test_a_culprit_whose_fast_check_left_a_check_out_records_a_miss(
    repo: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    commits = git(repo, "rev-list", "--reverse", "origin/master..HEAD").split()
    # Versions 2 to 4 were one push, so its fast check ran at version 4 and saw version 3's
    # change; version 5 was pushed from another host, whose run log this is not.
    store = Store({"by-push": commits[1:4]})
    monkeypatch.setattr(verdicts, "STORE", store)
    fast_check(
        commits[3],
        ("ui-test:plays a land", "pass", "coverage:ui/src/game/Board.tsx:12"),
        ("ui-test:casts a spell", "skip", "cut"),
    )
    lines = run(repo, capsys)
    tip = commits[3][:10]
    assert (
        f"miss: ui-test:casts a spell (fast check at {tip} did not choose it: cut)"
        in (lines)
    )
    assert not any(line.startswith("miss: ui-test:plays a land") for line in lines)
    assert (
        f"miss: ui-test:draws a card unknown (no fast check of {commits[4][:10]} in this"
        " host's run log)" in lines
    )
    # The miss sits under its check's bisect line, before its record, which counts it.
    at = lines.index(
        f"miss: ui-test:casts a spell (fast check at {tip} did not choose it: cut)"
    )
    assert lines[at - 1].startswith("bisect: ui-test:casts a spell  culprit")
    assert lines[at + 1].endswith("catch 0, miss 1 (30 days)")
    [(check, lane, entry)] = store.recorded
    assert (check, lane) == ("ui-test:casts a spell", "ui-test")
    assert entry["verdict"] == "miss" and entry["commit"] == commits[2]
    assert (entry["fast"], entry["chosen"], entry["bead"]) == (
        commits[3],
        "cut",
        "by-push",
    )
    # The next slow CI blames the same culprit: the miss is not recorded twice.
    run(repo, capsys)
    assert len(store.recorded) == 1


def test_a_culprit_s_fast_check_is_found_only_within_its_own_push(repo: Path) -> None:
    commits = git(repo, "rev-list", "--reverse", "origin/master..HEAD").split()
    fast_check(commits[2], ("ui-test:untaps", "pass", "record"))
    log = runlog.read_log(datetime.fromtimestamp(0, timezone.utc))
    # A later push's fast check did not see the culprit's change.
    assert bisect.fast_run(commits[1], commits, [commits[1]], log, repo) == ("", [])
    fast, records = bisect.fast_run(commits[1], commits, commits[1:3], log, repo)
    assert fast == commits[2] and len(records) == 1
    assert bisect.unchosen("ui-test:untaps", "ui-test", records) == ""
    assert bisect.unchosen("ui-test:plays a land", "ui-test", records) == "not chosen"
    assert (
        bisect.unchosen("shots:board", "shots", records) == "its shots lane did not run"
    )
    # A step that failed though none of its checks did is run whenever its lane is.
    assert bisect.unchosen("ui-test:ui-test", "ui-test", records, whole=True) == ""
    assert bisect.unchosen("shots:shots", "shots", records, whole=True) != ""
    # A fast check run before the commit was made is matched by its tree.
    tree = git(repo, "rev-parse", f"{commits[0]}^{{tree}}")
    runlog.append([{**log[0], "commit": "", "tree": tree}])
    log = runlog.read_log(datetime.fromtimestamp(0, timezone.utc))
    assert bisect.fast_run(commits[0], commits, [], log, repo)[0] == commits[0]
