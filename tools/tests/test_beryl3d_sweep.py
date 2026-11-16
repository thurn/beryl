"""Beryl 3D's sweep (beryl3d/sweep.py): when it is due, what it files, and that slow CI neither
pays for it nor fails with it."""

from __future__ import annotations

import json
import os
import re
import time
from datetime import datetime, timezone
from collections.abc import Callable
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
    made = sweep.lanes(sweep.chosen("clippy", sweep.MAC))
    assert [(m.name, [s.name for s in m.steps]) for m in made] == [
        ("b3d-clippy", ["clippy"])
    ]
    assert sweep.chosen("clippy", "linux") == []
    assert sweep.chosen(None, sweep.WINDOWS) == []


def test_windows_never_starts_a_sweep() -> None:
    def start(by: list[str], log: Path) -> bool:
        raise AssertionError("a sweep started on Windows")

    line = sweep.start_background(start=start, platform=sweep.WINDOWS)
    assert line == "b3d sweep: not run on win32"


def test_the_quick_part_holds_the_build_and_the_lane_and_the_slow_part_the_vm() -> None:
    quick = [s.name for s in sweep.chosen(None, sweep.MAC, sweep.QUICK)]
    assert quick == [s.name for s in lane.steps()] + [
        "smoke",
        "release-socket",
        "every-tick",
        "timeline-parity-all",
    ]
    assert set(sweep.BUILDS) <= set(quick)
    assert [s.name for s in sweep.chosen(None, sweep.MAC, sweep.SLOW)] == [
        "gpu",
        "frame-counts",
    ]
    assert sweep.chosen(None, sweep.WINDOWS, sweep.SLOW) == []


class Runner:
    """A stand-in for the run in a tree: records the commands it was given, exits `code` (or
    `codes[part]` for a part named there), and runs `during` inside the part `during_part`.
    """

    def __init__(
        self,
        code: int = 0,
        codes: dict[str, int] | None = None,
        during: Callable[[], None] | None = None,
        during_part: str = sweep.SLOW,
    ) -> None:
        self.code = code
        self.codes: dict[str, int] = codes or {}
        self.during = during
        self.during_part = during_part
        self.runs: list[tuple[str, list[str]]] = []

    def __call__(self, commit: str, command: list[str], output: Path, part: str) -> int:
        self.runs.append((commit, command))
        # A replay's tree is kept apart from the parts', whatever part its step is in.
        assert part == sweep.REPLAY or command[command.index("--part") + 1] == part
        if self.during is not None and part == self.during_part:
            self.during()
        return self.codes.get(part, self.code)


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
    # The quick part failed, so the slow part did not run.
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
    assert command[command.index("--part") + 1] == sweep.QUICK
    assert sweep.last_swept(sweep.read_log()) == "aaa"
    assert len(sweep.read_log()) == 1


def test_the_quick_part_runs_first_and_each_part_bisects_from_its_own_last_pass() -> (
    None
):
    runner = Runner()
    record = fire(runner, tip="aaa")
    assert [c[c.index("--part") + 1] for _, c in runner.runs] == ["quick", "slow"]
    parts = record["parts"]
    assert isinstance(parts, dict)
    assert {p: sorted(v) for p, v in parts.items()} == {
        "quick": ["code", "seconds"],
        "slow": ["code", "seconds"],
    }
    # Only the slow part fails at bbb: the next sweep bisects it from aaa, the quick part
    # from bbb.
    assert fire(Runner(codes={"slow": 1}), tip="bbb")["outcome"] == "failed"
    runner = Runner(codes={"slow": 1})
    fire(runner, tip="ccc")
    [(_, quick), (_, slow)] = runner.runs
    assert quick[-2:] == ["--since", "bbb"] and slow[-2:] == ["--since", "aaa"]
    # A record from before the sweep had parts passed both.
    old: dict[str, object] = {"commit": "old", "outcome": "ok"}
    assert sweep.last_passed([old], sweep.SLOW) == "old"


def test_a_quick_part_runs_while_another_sweeps_slow_part_holds_its_lock() -> None:
    inner: list[dict[str, object]] = []

    def second_sweep() -> None:
        # Fired while the first sweep is in its slow part.
        inner.append(fire(Runner(), tip="bbb"))

    first = fire(Runner(during=second_sweep), tip="aaa")
    assert first["outcome"] == "ok"
    [second] = inner
    assert second["outcome"] == "ok"
    parts = second["parts"]
    assert isinstance(parts, dict)
    assert parts["quick"]["code"] == 0
    assert parts["slow"] == {"skipped": "another sweep's slow part is in progress"}
    # While a quick part runs, a firing is skipped whole.
    inner.clear()
    fire(Runner(during=second_sweep, during_part=sweep.QUICK), tip="ccc")
    assert inner[0]["reason"] == "another sweep is in progress"


def test_a_replay_of_one_step_runs_while_a_sweep_holds_its_lock() -> None:
    inner: list[dict[str, object]] = []
    replayed: Runner = Runner()

    def replay() -> None:
        # Asked for while the sweep's quick part runs.
        request = sweep.Request(step="clippy")
        inner.append(fire(replayed, tip="bbb", is_due=False, request=request))

    fire(Runner(), tip="aaa")
    fire(Runner(during=replay, during_part=sweep.QUICK), tip="bbb")
    [record] = inner
    assert record["outcome"] == "ok"
    [(commit, command)] = replayed.runs
    # No bisection and no filing: a dry run with no `--since`.
    assert commit == "bbb" and "--since" not in command
    assert command[-3:] == ["--step", "clippy", "--dry-run"]
    # Not logged as the machine's sweep.
    assert [r["commit"] for r in sweep.read_log()] == ["aaa", "bbb"]


def test_a_steps_own_command_is_what_a_worker_types() -> None:
    assert sweep.own_command("cargo-test") == "just b3d cargo-test"
    assert sweep.own_command("gpu") == "just b3d gate --gpu"
    assert sweep.own_command("scene-purity").startswith("just b3d sweep --step")


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
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    store = FakeBd()
    failed = {
        "b3d-cargo-test": ["b3d-cargo-test:cargo-test"],
        "b3d-release-socket": ["b3d-release-socket:release-socket"],
    }
    output = {
        "b3d-cargo-test": "error[E0425]: cargo-test failure",
        "b3d-release-socket": "error: release-socket failure",
    }
    found = ["bisect: b3d-cargo-test:cargo-test  culprit 1234567890 feat: session"]
    # Each new bead records the machine that ran the step, without pinning it there.
    monkeypatch.setenv("ENACT_HOST", "cygnus")
    # A dry run reads the open beads and prints the ones it would file.
    dry = sweep.DryRun(store)
    sweep.file("bad1", failed, "good", found, output, dry)
    printed = capsys.readouterr().out
    assert printed.count("would file") == 2
    assert f"would file under {paths.EPIC}" in printed
    assert "culprit 1234567890" in printed
    assert "from host cygnus" in printed
    assert store.issues == []

    lines = sweep.file("bad1", failed, "good", found, output, store)
    assert lines == [
        "b3d sweep: by-1: filed b3d-cargo-test:cargo-test",
        "b3d sweep: by-2: filed b3d-release-socket:release-socket",
    ]
    issue, socket_issue = store.issues
    assert issue["parent"] == paths.EPIC and issue["priority"] == "1"
    assert sweep.LABEL in issue["labels"]  # pyre-ignore[58]
    metadata = issue["metadata"]
    assert isinstance(metadata, dict)
    assert metadata["enact_ci_host"] == "cygnus"
    assert "enact_host" not in metadata
    text = str(issue["description"])
    assert "just b3d sweep --step cargo-test --commit bad1" in text
    assert output["b3d-cargo-test"] in text
    assert output["b3d-release-socket"] not in text
    assert output["b3d-release-socket"] in str(socket_issue["description"])
    accept = str(issue["acceptance"])
    assert accept.startswith("`just b3d cargo-test` passes in a worktree rebased on")
    assert "just b3d sweep --step cargo-test --commit origin/staging" in accept
    assert "last sweep that passed on this machine was at good" in text

    # The same check failing again comments on that bead, in a dry run too.
    sweep.file("bad2", failed, "good", [], output, dry)
    assert "would add to by-1 the comment" in capsys.readouterr().out
    sweep.file("bad2", failed, "good", [], output, store)
    assert len(store.issues) == 2
    [comment] = issue["comments"]  # pyre-ignore[23]
    assert "bad2" in comment
    assert output["b3d-cargo-test"] in comment
    assert output["b3d-release-socket"] not in comment


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
    assert all("sweep" in made.deferred_to for made in lane.lanes())
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


def test_saving_the_build_directory_drops_artifacts_no_build_uses(
    tmp_path: Path,
) -> None:
    target = tmp_path / "target"
    deps = target / "debug" / "deps"
    deps.mkdir(parents=True)
    stale = deps / "libberyl_scene-0123456789abcdef.rlib"
    current = deps / "libberyl_scene-fedcba9876543210.rlib"
    stale.write_bytes(b"old")
    current.write_bytes(b"new")
    month_ago = time.time() - 30 * 86400
    os.utime(stale, (month_ago, month_ago))
    cached = paths.cache() / "target"
    assert sweep.save_target(target, cached)
    assert [p.name for p in (cached / "debug" / "deps").iterdir()] == [current.name]


def test_a_step_that_passes_at_a_newer_tip_files_no_bead(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    # Every commit descends from the swept one, so only the tip's replays decide.
    monkeypatch.setattr(sweep, "run_git", lambda *args: "")
    failed = {
        "b3d-cargo-test": ["b3d-cargo-test:cargo-test"],
        "b3d-release-socket": ["b3d-release-socket:release-socket"],
    }
    replayed: list[tuple[str, str]] = []

    def replay(tip: str, step: str) -> int:
        replayed.append((tip, step))
        return 0 if step == "cargo-test" else 1

    kept = sweep.still_failing("bad1", failed, lambda: "newer", replay)
    assert replayed == [("newer", "cargo-test"), ("newer", "release-socket")]
    assert kept == {"b3d-release-socket": ["b3d-release-socket:release-socket"]}
    assert "cargo-test: fixed on the tip newer" in capsys.readouterr().out

    # Only what still fails reaches the filing.
    store = FakeBd()
    output = {name: "failure" for name in failed}
    sweep.file("bad1", kept, "good", [], output, store)
    [issue] = store.issues
    assert "release-socket" in str(issue["title"])

    # A tip that is the swept commit, or cannot be fetched, replays nothing.
    replayed.clear()
    assert sweep.still_failing("bad1", failed, lambda: "bad1", replay) == failed
    assert sweep.still_failing("bad1", failed, lambda: None, replay) == failed
    assert replayed == []
