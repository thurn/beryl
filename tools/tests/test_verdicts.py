import argparse
import json
import subprocess
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from beryl_tools import checks, cli, paths, runlog, verdicts


class FakeBd(verdicts.Beads):
    """bd as the ci-check beads use it, in memory."""

    def __init__(self) -> None:
        self.issues: dict[str, dict[str, object]] = {}
        self.stewards: list[dict[str, object]] = []
        # The beads flaky.py filed, by id.
        self.filed: dict[str, dict[str, object]] = {}

    def bd(self, *args: str) -> tuple[int, str]:
        if args[0] == "create" and "--id" not in args:
            bead = f"by-f{len(self.filed) + 1}"
            self.filed[bead] = {
                "id": bead,
                "status": "open",
                "title": args[args.index("--title") + 1],
                "priority": args[args.index("--priority") + 1],
                "metadata": json.loads(args[args.index("--metadata") + 1]),
                "comments": [],
            }
            return 0, bead + "\n"
        if args[:2] == ("comments", "add") and args[2] in self.filed:
            comments = self.filed[args[2]]["comments"]
            assert isinstance(comments, list)
            comments.append(args[3])
            return 0, ""
        if args[:3] == ("list", "--label", "flaky-check"):
            return 0, json.dumps(list(self.filed.values()))
        if args[0] == "create":
            bead = args[args.index("--id") + 1]
            if bead in self.issues:
                return 1, ""
            labels = args[args.index("--labels") + 1].split(",")
            title = args[args.index("--title") + 1]
            self.issues[bead] = {"title": title, "labels": labels, "comments": []}
            return 0, bead
        if args[:2] == ("comments", "add"):
            comments = self.issues[args[2]]["comments"]
            assert isinstance(comments, list)
            comments.append({"id": str(uuid.uuid4()), "text": args[3]})
            return 0, ""
        if args == ("export",):
            return 0, "\n".join(
                json.dumps({"id": bead, **issue}) for bead, issue in self.issues.items()
            )
        if args[:3] == ("list", "--label", "ci-check"):
            return 0, json.dumps(list(self.issues.values()))
        if args[:3] == ("list", "--label", "role:steward"):
            return 0, json.dumps(self.stewards)
        raise AssertionError(args)

    def verdicts(self, check: str) -> list[object]:
        return [e["verdict"] for e in self.history([check]).get(check, [])]


@pytest.fixture
def bd(monkeypatch: pytest.MonkeyPatch) -> FakeBd:
    fake = FakeBd()
    monkeypatch.setattr(verdicts, "STORE", fake)
    return fake


@pytest.fixture
def repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A repository of its own, so trees can be told apart."""
    root = tmp_path / "repo"
    root.mkdir()
    git(root, "init", "-q", "-b", "work")
    write(root, "ui/src/Board.tsx", "board\n")
    write(root, "ui/scripts/kept-table.mjs", "checks\n")
    git(root, "add", "-A")
    git(root, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a")
    monkeypatch.setattr(paths, "REPO", root)
    return root


def git(root: Path, *args: str) -> str:
    run = subprocess.run(
        ["git", *args], cwd=root, check=True, capture_output=True, text=True
    )
    return run.stdout.strip()


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


def tree(root: Path) -> str:
    git(root, "add", "-A")
    return git(root, "write-tree")


def reports(check: str, outcome: str, then: str = "true") -> list[str]:
    """A step that reports one check's result, as a test binary or browser script would."""
    line = json.dumps({"check": check, "outcome": outcome, "seconds": 1.0})
    return ["sh", "-c", f"echo '{line}' >> \"$BERYL_CHECK_RESULTS\"; {then}"]


def one(check: str, passes: bool) -> checks.Lane:
    lane = check.split(":")[0]
    step = reports(check, "pass") if passes else reports(check, "fail", "false")
    return checks.Lane(lane).step(lane, step)


def flaky(check: str, root: Path, whole: bool = False) -> checks.Lane:
    """A lane whose one check fails, and passes on the rerun; with `whole`, a step that reports
    no checks of its own."""
    fails, passes = reports(check, "fail", "false")[2], reports(check, "pass")[2]
    if whole:
        fails, passes = "false", "true"
    once = f"if [ -e ran ]; then {passes}; else touch ran; {fails}; fi"
    lane = check.split(":")[0]
    return checks.Lane(lane).step(lane, ["sh", "-c", once], root, rerun=checks.whole)


def test_a_check_that_passes_on_the_rerun_is_flaky(
    tmp_path: Path, bd: FakeBd, capsys: pytest.CaptureFixture[str]
) -> None:
    lane = flaky("ui-test:scry", tmp_path)
    assert checks.run_lanes([lane], "check", 60) == 0
    assert bd.verdicts("ui-test:scry") == ["flaky"]
    assert "verdict: ui-test ui-test:scry flaky" in capsys.readouterr().out
    issue = bd.issues[verdicts.bead_id("ui-test:scry")]
    assert issue["labels"] == ["ci-check", "ci-lane:ui-test"]
    entry = bd.history(["ui-test:scry"])["ui-test:scry"][0]
    assert entry["script"] == "check" and str(entry["fixed"]).startswith("tree ")


def test_a_fix_to_the_check_alone_is_stale_and_to_the_product_a_catch(
    repo: Path, bd: FakeBd, capsys: pytest.CaptureFixture[str]
) -> None:
    assert checks.run_lanes([one("ui-test:drag", False)], "check", 60) == 1
    write(repo, "ui/scripts/kept-table.mjs", "checks, fixed\n")
    assert checks.run_lanes([one("ui-test:drag", True)], "check", 60) == 0
    assert bd.verdicts("ui-test:drag") == ["stale"]

    assert checks.run_lanes([one("ui-test:drag", False)], "check", 60) == 1
    write(repo, "ui/src/Board.tsx", "board, fixed\n")
    write(repo, "docs/notes.md", "prose touches no side\n")
    assert checks.run_lanes([one("ui-test:drag", True)], "check", 60) == 0
    assert bd.verdicts("ui-test:drag") == ["stale", "catch"]

    # A pass with nothing changed since the failure: a flake no rerun saw.
    assert checks.run_lanes([one("ui-test:drag", False)], "check", 60) == 1
    assert checks.run_lanes([one("ui-test:drag", True)], "check", 60) == 0
    assert bd.verdicts("ui-test:drag") == ["stale", "catch", "flaky"]
    capsys.readouterr()

    assert checks.run_lanes([one("ui-test:drag", False)], "check", 60) == 1
    write(repo, "ui/src/Board.tsx", "board, fixed twice\n")
    write(repo, "ui/scripts/kept-table.mjs", "checks, fixed twice\n")
    assert checks.run_lanes([one("ui-test:drag", True)], "check", 60) == 0
    assert bd.verdicts("ui-test:drag")[-1] == "mixed"
    assert "just verdict 'ui-test:drag' stale|catch" in capsys.readouterr().out
    args = argparse.Namespace(check="ui-test:drag", verdict="catch")
    assert verdicts.correct(args) == 0
    assert bd.verdicts("ui-test:drag") == ["stale", "catch", "flaky", "catch"]


def test_a_pass_settles_only_failures_from_its_own_place(
    repo: Path, bd: FakeBd
) -> None:
    assert checks.run_lanes([one("shots:board", False)], "check", 60) == 1
    git(repo, "checkout", "-qb", "other")
    write(repo, "ui/src/Board.tsx", "board, elsewhere\n")
    assert checks.run_lanes([one("shots:board", True)], "check", 60) == 0
    git(repo, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "b")
    assert bd.verdicts("shots:board") == []
    git(repo, "checkout", "-q", "work")
    write(repo, "ui/shots.json", "{}\n")
    assert checks.run_lanes([one("shots:board", True)], "check", 60) == 0
    assert bd.verdicts("shots:board") == ["stale"]


def test_a_slow_ci_failure_names_the_steward_that_fixed_it(
    repo: Path, bd: FakeBd
) -> None:
    assert checks.run_lanes([one("shots:board", False)], "ci", 60) == 1
    write(repo, "ui/src/Board.tsx", "board, repaired\n")
    git(repo, "add", "-A")
    git(repo, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "fix")
    bd.stewards = [
        {"id": "by-other", "metadata": {"enact_commits": "0" * 40}},
        {
            "id": "by-steward",
            "metadata": {"enact_commits": git(repo, "rev-parse", "HEAD")},
        },
    ]
    assert checks.run_lanes([one("shots:board", True)], "ci", 60) == 0
    [entry] = bd.history(["shots:board"])["shots:board"]
    assert entry["verdict"] == "catch" and entry["steward"] == "by-steward"
    assert entry["fixed"] == git(repo, "rev-parse", "HEAD")


def test_a_steward_fix_is_judged_by_its_own_commits_alone(
    repo: Path, bd: FakeBd
) -> None:
    assert checks.run_lanes([one("shots:board", False)], "ci", 60) == 1
    commit = ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm"]
    write(repo, "ui/src/Board.tsx", "board, someone else's feature\n")
    git(repo, "add", "-A")
    git(repo, *commit, "feature")
    write(repo, "ui/shots.json", "{}\n")
    git(repo, "add", "-A")
    git(repo, *commit, "fix")
    fix = git(repo, "rev-parse", "HEAD")
    bd.stewards = [{"id": "by-steward", "metadata": {"enact_commits": fix}}]
    assert checks.run_lanes([one("shots:board", True)], "ci", 60) == 0
    [entry] = bd.history(["shots:board"])["shots:board"]
    # The range from the failure holds the feature too, which alone would read as mixed.
    assert entry["verdict"] == "stale" and entry["steward"] == "by-steward"


RUST = """\
pub fn product() -> u32 {
    1
}

/// The card's scenarios.
pub const SCENARIOS: &[Scenario] = &[Scenario::new("a land's ) in a string")
    .at(2),
];

#[cfg(test)]
mod tests {
    #[test]
    fn product_is_one() {
        assert_eq!(super::product(), 1);
    }
}
"""


def test_rust_test_items_and_scenarios_are_the_checks_side(repo: Path) -> None:
    write(repo, "rust/crates/beryl_cards/src/land.rs", RUST)
    before = tree(repo)

    def changed(old: str, new: str) -> str:
        write(repo, "rust/crates/beryl_cards/src/land.rs", RUST.replace(old, new))
        return tree(repo)

    in_test = changed("super::product(), 1", 'super::product(), 1, "one"')
    assert verdicts.classify("rust", before, in_test) == "stale"
    # An oracle checks scenarios; a unit test is no side of it.
    assert verdicts.classify("oracle", before, in_test) == "flaky"
    in_scenario = changed(".at(2)", ".at(3)")
    assert verdicts.classify("rust", before, in_scenario) == "stale"
    assert verdicts.classify("oracle", before, in_scenario) == "stale"
    in_product = changed("    1\n}", "    2 - 1\n}")
    assert verdicts.classify("rust", before, in_product) == "catch"
    assert verdicts.classify("ui-test", before, in_product) == "catch"
    write(repo, "rust/crates/beryl_cards/tests/cards.rs", "#[test] fn t() {}\n")
    assert verdicts.classify("rust", in_product, tree(repo)) == "stale"


def test_the_items_of_a_rust_file_end_where_their_brackets_close() -> None:
    assert verdicts.regions(RUST, "scenarios") == [(5, 8)]
    assert verdicts.regions(RUST, "tests") == [(5, 8), (10, 16)]
    assert verdicts.regions("#[cfg(test)]\nmod tests;\nfn f() {}\n", "tests") == [
        (1, 2)
    ]


def test_a_flaky_check_files_one_bead_to_delete_it(
    tmp_path: Path, bd: FakeBd, capsys: pytest.CaptureFixture[str]
) -> None:
    assert checks.run_lanes([flaky("ui-test:scry", tmp_path)], "check", 60) == 0
    [(bead, filed)] = bd.filed.items()
    assert filed["title"] == "Delete flaky check ui-test:scry"
    assert filed["priority"] == "1"
    assert filed["metadata"] == {"beryl_flaky_check": "ui-test:scry"}
    assert f"flaky: ui-test:scry: filed {bead} to delete it" in capsys.readouterr().out
    # A second flake while the bead is open comments on it instead of filing another.
    (tmp_path / "ran").unlink()
    assert checks.run_lanes([flaky("ui-test:scry", tmp_path)], "check", 60) == 0
    assert list(bd.filed) == [bead]
    [comment] = bd.filed[bead]["comments"]  # pyre-ignore[23]: a list, from FakeBd
    assert str(comment).startswith("Flaked again in `by check`")
    assert (
        f"flaky: ui-test:scry: {bead} is open to delete it" in capsys.readouterr().out
    )
    # A step that names no checks of its own asks for the flaky test inside it.
    (tmp_path / "ran").unlink()
    assert (
        checks.run_lanes([flaky("ui-test:drag", tmp_path, whole=True)], "check", 60)
        == 0
    )
    assert "Delete the flaky test in step ui-test:ui-test" in [
        f["title"] for f in bd.filed.values()
    ]
    # Once it is closed, the next flake files a new one.
    bd.filed[bead]["status"] = "closed"
    (tmp_path / "ran").unlink()
    assert checks.run_lanes([flaky("ui-test:scry", tmp_path)], "check", 60) == 0
    assert len(bd.filed) == 3


def test_a_check_that_flaked_before_still_fails_the_run(bd: FakeBd) -> None:
    for days in (1, 2, 3):
        when = datetime.now(timezone.utc) - timedelta(days=days)
        bd.record(
            "ui-test:scry", "ui-test", {"time": when.isoformat(), "verdict": "flaky"}
        )
    assert checks.run_lanes([one("ui-test:scry", False)], "check", 60) == 1
    assert bd.filed == {}


def test_a_build_failure_is_never_excused(tmp_path: Path, bd: FakeBd) -> None:
    lane = checks.Lane("app").step(
        "smoke", ["sh", "-c", f"exit {checks.CANNOT_RUN}"], rerun=checks.whole
    )
    assert checks.run_lanes([lane], "check", 60) == 1


def test_a_pending_check_is_logged_as_pending_and_fails_no_run(
    bd: FakeBd, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    pending = verdicts.Pending("shots:flight", "flight_to_stack", "by-bank1")
    monkeypatch.setattr(verdicts, "PENDING", (pending,))
    assert checks.run_lanes([one("shots:flight", False)], "check", 60) == 0
    assert "pending: shots shots:flight" in capsys.readouterr().out
    assert checks.run_lanes([one("shots:flight", True)], "check", 60) == 0
    assert [r["outcome"] for r in logged()] == ["pending-fail", "pending-pass"]
    assert bd.issues == {}
    capsys.readouterr()
    assert runlog.report(report_args(pending=True)) == 0
    [row] = json.loads(capsys.readouterr().out)["pending"]
    assert row == {
        "check": "shots:flight",
        "board": "flight_to_stack",
        "bead": "by-bank1",
        "last": "pending-pass",
    }


def test_verdicts_that_cannot_be_recorded_fail_no_run(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    class Broken(FakeBd):
        def bd(self, *args: str) -> tuple[int, str]:
            return 1, "not json"

    monkeypatch.setattr(verdicts, "STORE", Broken())
    assert checks.run_lanes([flaky("ui-test:scry", tmp_path)], "check", 60) == 0


def logged() -> list[dict[str, object]]:
    return [json.loads(line) for line in runlog.log_path().read_text().splitlines()]


def report_args(**more: object) -> argparse.Namespace:
    args = {
        "days": 30,
        "json": True,
        "pending": False,
        "prune": False,
    }
    return argparse.Namespace(**{**args, **more})


def test_the_verdict_command_corrects_through_the_cli(
    bd: FakeBd, capsys: pytest.CaptureFixture[str]
) -> None:
    bd.record(
        "rust:a::b", "rust", {"time": "2026-11-10T00:00:00+00:00", "verdict": "mixed"}
    )
    assert cli.main(["verdict", "rust:a::b", "stale"]) == 0
    assert bd.verdicts("rust:a::b") == ["stale"]
    assert cli.main(["verdict", "rust:nothing", "stale"]) == 1
    assert "has no verdict to correct" in capsys.readouterr().err
