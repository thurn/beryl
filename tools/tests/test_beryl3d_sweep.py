"""Beryl 3D's sweep (beryl3d/sweep.py): when it is due, what it files, and that slow CI neither
pays for it nor fails with it."""

from __future__ import annotations

import json
import re
from datetime import datetime, timezone
from pathlib import Path

import pytest

from beryl_tools import budget, check_value, slow_ci, verdicts
from beryl_tools.beryl3d import lane, paths, sweep

NOW = datetime(2026, 11, 15, 6, 0, tzinfo=timezone.utc)


def write_crate(rust: Path, name: str, *deps: str) -> None:
    crate = rust / "crates" / name
    crate.mkdir(parents=True)
    lines = [f'{dep} = {{ path = "../{dep}" }}' for dep in deps]
    (crate / "Cargo.toml").write_text("[dependencies]\n" + "\n".join(lines) + "\n")


@pytest.fixture
def pattern(tmp_path: Path) -> re.Pattern[str]:
    """The due rule over a workspace where the session depends on the engine and the engine on
    the kernel, and the card tools depend on nothing Beryl 3D links."""
    rust = tmp_path / "rust"
    write_crate(rust, "beryl_session", "beryl_engine")
    write_crate(rust, "beryl_engine", "beryl_kernel")
    write_crate(rust, "beryl_kernel")
    write_crate(rust, "beryl_cli", "beryl_session")
    write_crate(rust, "beryl_card_build")
    return sweep.inputs(rust)


@pytest.mark.parametrize(
    ("changed", "is_due"),
    [
        # Beryl 3D's own code, data and tools.
        (["rust/beryl3d/crates/beryl_scene/src/lib.rs"], True),
        (["data/beryl3d/gates.json", "tools/src/beryl_tools/beryl3d/lane.py"], True),
        # A crate the session depends on two path dependencies down.
        (["README.md", "rust/crates/beryl_kernel/src/state.rs"], True),
        # The data files Beryl 3D reads, and the manifest whose lints it copies.
        (["data/card-store.tsv"], True),
        (["ui/fixtures.json"], True),
        (["data/ui-conventions.json"], True),
        (["rust/Cargo.toml"], True),
        # Neither Beryl 3D nor anything it builds on.
        (
            [
                "ui/src/stage/phone.ts",
                "rust/crates/beryl_card_build/src/lib.rs",
                "rust/Cargo.lock",
                "tools/src/beryl_tools/sweep.py",
                "rust/crates/beryl_sessionx/a.rs",
            ],
            False,
        ),
        ([], False),
    ],
)
def test_a_sweep_is_due_only_for_beryl_3d_and_what_it_builds_on(
    pattern: re.Pattern[str], changed: list[str], is_due: bool
) -> None:
    assert bool(sweep.due(changed, pattern)) == is_due


def test_the_real_due_rule_follows_the_sessions_path_dependencies() -> None:
    found = sweep.inputs()
    assert found.search("rust/crates/beryl_session/src/lib.rs")
    assert found.search("rust/crates/beryl_kernel/src/lib.rs")
    assert not found.search("rust/crates/beryl_app/src/main.rs")


def test_the_sweep_runs_every_step_of_the_lane_each_as_a_lane_of_its_own() -> None:
    names = [s.name for s in sweep.chosen(None, sweep.MAC)]
    assert names[: len(lane.steps())] == [s.name for s in lane.steps()]
    made = sweep.lanes(sweep.chosen("clippy", sweep.WINDOWS))
    assert [(m.name, [s.name for s in m.steps]) for m in made] == [
        ("b3d-clippy", ["clippy"])
    ]
    assert sweep.chosen("clippy", "linux") == []


class Runner:
    """A stand-in for the run in a tree: records the commands it was given, exits `code`."""

    def __init__(self, code: int = 0) -> None:
        self.code = code
        self.runs: list[tuple[str, list[str]]] = []

    def __call__(self, commit: str, command: list[str], output: Path) -> int:
        self.runs.append((commit, command))
        return self.code


def fire(
    runner: Runner,
    tip: str = "bbb",
    is_due: bool = True,
    load: float = 1.0,
    request: sweep.Request | None = None,
) -> dict[str, object]:
    def changed(since: str, commit: str) -> list[str] | None:
        return ["rust/beryl3d/a.rs"] if is_due else ["README.md"]

    MONKEY[0].setattr(sweep, "changed_since", changed)
    return sweep.run_sweep(
        request or sweep.Request(),
        now=lambda: NOW,
        load=lambda: load,
        cores=lambda: 4,
        tip=lambda: tip,
        pattern=lambda: re.compile(r"^rust/beryl3d/"),
        run=runner,
    )


# The test's monkeypatch, for the helpers that stand in for git's list of changed paths.
MONKEY: list[pytest.MonkeyPatch] = []


@pytest.fixture(autouse=True)
def monkey(monkeypatch: pytest.MonkeyPatch) -> None:
    MONKEY[:] = [monkeypatch]


def test_a_firing_sweeps_only_what_is_due_and_bisects_from_the_last_pass() -> None:
    assert fire(Runner(), tip="aaa")["outcome"] == "ok"
    skipped = fire(Runner(), tip="aaa")
    assert "swept already" in str(skipped["reason"])
    quiet = fire(Runner(), tip="bbb", is_due=False)
    assert "nothing since aaa" in str(quiet["reason"])
    failing = Runner(code=1)
    assert fire(failing, tip="ccc")["outcome"] == "failed"
    [(commit, command)] = failing.runs
    assert commit == "ccc" and command[-2:] == ["--since", "aaa"]
    assert "load average" in str(fire(Runner(), tip="ddd", load=100.0)["reason"])


def test_a_step_or_dry_run_is_not_the_last_sweep() -> None:
    fire(Runner(), tip="aaa")
    runner = Runner()
    request = sweep.Request(step="clippy", dry_run=True)
    # Not due, but a run of one step runs anyway.
    fire(runner, tip="aaa", is_due=False, request=request)
    [(_, command)] = runner.runs
    assert command[-3:] == ["--step", "clippy", "--dry-run"]
    assert sweep.last_swept(sweep.read_log()) == "aaa"
    assert len(sweep.read_log()) == 1


class FakeBd(verdicts.Beads):
    """bd as the sweep's filing uses it, in memory."""

    def __init__(self) -> None:
        self.issues: list[dict[str, object]] = []

    def bd(self, *args: str) -> tuple[int, str]:
        if args[:3] == ("list", "--label", sweep.LABEL):
            return 0, json.dumps(self.issues)
        if args[0] == "create":
            flag = lambda name: args[args.index(name) + 1]  # noqa: E731
            bead = f"by-{len(self.issues) + 1}"
            self.issues.append(
                {
                    "id": bead,
                    "title": flag("--title"),
                    "description": flag("--description"),
                    "parent": flag("--parent"),
                    "acceptance": flag("--acceptance"),
                    "priority": flag("--priority"),
                    "labels": flag("--labels").split(","),
                    "metadata": json.loads(flag("--metadata")),
                    "status": "open",
                    "comments": [],
                }
            )
            return 0, bead
        if args[:2] == ("comments", "add"):
            [issue] = [i for i in self.issues if i["id"] == args[2]]
            comments = issue["comments"]
            assert isinstance(comments, list)
            comments.append(args[3])
            return 0, ""
        raise AssertionError(args)


def test_a_failed_step_files_one_bead_under_the_epic_and_a_repeat_comments(
    capsys: pytest.CaptureFixture[str],
) -> None:
    store = FakeBd()
    failed = {"b3d-cargo-test": ["b3d-cargo-test:cargo-test"]}
    found = ["bisect: b3d-cargo-test:cargo-test  culprit 1234567890 feat: session"]
    # A dry run reads the open beads and prints the one it would file.
    dry = sweep.DryRun(store)
    sweep.file("bad1", failed, "good", found, "error[E0425]", dry)
    printed = capsys.readouterr().out
    assert printed.count("would file") == 1
    assert f"would file under {paths.EPIC}" in printed
    assert "culprit 1234567890" in printed
    assert store.issues == []

    lines = sweep.file("bad1", failed, "good", found, "error[E0425]", store)
    assert lines == ["b3d sweep: by-1: filed b3d-cargo-test:cargo-test"]
    [issue] = store.issues
    assert issue["parent"] == paths.EPIC and issue["priority"] == "1"
    assert sweep.LABEL in issue["labels"]  # pyre-ignore[58]
    text = str(issue["description"])
    assert "just b3d sweep --step cargo-test --commit bad1" in text
    assert "--commit origin/staging` passes" in str(issue["acceptance"])
    assert "last sweep that passed on this machine was at good" in text

    # The same check failing again comments on that bead, in a dry run too.
    sweep.file("bad2", failed, "good", [], "again", dry)
    assert "would add to by-1 the comment" in capsys.readouterr().out
    sweep.file("bad2", failed, "good", [], "again", store)
    [only] = store.issues
    [comment] = only["comments"]  # pyre-ignore[23]
    assert "bad2" in comment


def test_slow_cis_budget_leaves_out_the_sweeps_records() -> None:
    def record(i: int, script: str, check: str, spent: float) -> dict[str, object]:
        return {
            "time": f"2026-11-13T10:{i:02}:00+00:00",
            "host": "mac",
            "commit": f"c{i}",
            "script": script,
            "lane": check.split(":")[0],
            "check": check,
            "outcome": "pass",
            "seconds": spent,
            "cpu_seconds": spent,
        }

    records = [
        r
        for i in range(check_value.RUNS)
        for r in (
            record(i, "ci", "rust:tests", 10.0),
            record(i, sweep.SCRIPT, "b3d-cargo-test:cargo-test", 5000.0),
        )
    ]
    assert all(r["script"] == "ci" for r in check_value.slow_ci(records))
    lines = budget.slow_report(records, {}, verdicts.Beads(), 100.0, host="mac")
    assert lines == ["ci budget: a full run uses 10 of 100 CPU seconds"]


def test_a_sweep_that_fails_to_start_does_not_fail_slow_ci() -> None:
    def broken() -> str:
        raise RuntimeError("bd is gone")

    lines = slow_ci.start_sweeps((broken, lambda: "b3d sweep: not due"))
    assert lines == ["a sweep was not started: bd is gone", "b3d sweep: not due"]


def test_slow_ci_starts_a_sweep_only_when_one_is_due() -> None:
    started: list[list[str]] = []

    def start(by: list[str], log: Path) -> bool:
        started.append(by)
        return True

    def background(head: str, changed: list[str]) -> str:
        MONKEY[0].setattr(sweep, "changed_since", lambda since, commit: changed)
        return sweep.start_background(
            head=lambda: head,
            pattern=lambda: re.compile(r"^rust/beryl3d/"),
            start=start,
        )

    assert "started" in background("aaa", []), "no sweep on record"
    fire(Runner(), tip="aaa")
    assert background("aaa", []) == "b3d sweep: not due: aaa was swept already"
    assert "not due: nothing since aaa" in background("bbb", ["README.md"])
    with sweep.system.file_lock(sweep.folder() / ".lock", timeout=0):
        assert background("bbb", ["rust/beryl3d/a.rs"]) == "b3d sweep: one is running"
    assert "started" in background("bbb", ["rust/beryl3d/a.rs"])
    assert started == [["b3d", "sweep"], ["b3d", "sweep"]]


def test_a_cut_off_lane_step_is_left_to_the_sweep() -> None:
    assert "sweep" in lane.lane().deferred_to
    assert "slow CI" == slow_ci.checks.Lane("rust").deferred_to


def test_saving_the_build_directory_drops_incremental_caches(tmp_path: Path) -> None:
    target = tmp_path / "target"
    (target / "debug" / "incremental" / "x").mkdir(parents=True)
    (target / "debug" / "deps").mkdir()
    cached = paths.cache() / "target"
    (cached / "stale").mkdir(parents=True)
    assert sweep.save_target(target, cached)
    assert (cached / "debug" / "deps").is_dir()
    assert not (cached / "debug" / "incremental").exists()
    assert not (cached / "stale").exists()
    assert not sweep.save_target(tmp_path / "missing", cached)
