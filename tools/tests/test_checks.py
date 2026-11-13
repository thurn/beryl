import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from beryl_tools import (
    bisect,
    build_paths,
    checks,
    hygiene,
    oracles,
    paths,
    reruns,
    rust_tests,
)


def test_only_lanes_that_build_warm_the_caches(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    warmed: list[bool] = []
    monkeypatch.setattr(checks.warm, "ensure", lambda: warmed.append(True))
    docs = checks.Lane("docs").step("true", ["true"])
    assert checks.run_lanes([docs], "check", 60) == 0
    assert warmed == []
    build = checks.Lane("build", warm=True).step("true", ["true"])
    assert checks.run_lanes([docs, build], "check", 60) == 0
    assert warmed == [True]


def test_a_hung_step_is_killed_at_the_deadline_with_its_descendants(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setenv("CHECK_DEADLINE_S", "3")
    # A grandchild in the step's process group, and one that left it for its own session.
    hang = (
        "sleep 600 & echo $! > in-group; "
        "python3 -c 'import os, time; os.setsid(); time.sleep(600)' & echo $! > own-session; "
        "wait"
    )
    lane = checks.Lane("hang").step("sleep", ["sh", "-c", hang], tmp_path)
    started = time.monotonic()
    assert checks.run_lanes([lane], "check", 60) == 1
    assert time.monotonic() - started < 300
    out = capsys.readouterr().out
    assert "check hang sleep: TIMED OUT after" in out
    assert "(deadline 3 s)" in out
    for name in ("in-group", "own-session"):
        pid_file = tmp_path / name
        if (
            pid_file.exists()
        ):  # Absent only if the deadline came before the shell got there.
            assert gone(int(pid_file.read_text()))


def test_a_lane_the_deadline_killed_reruns_once_with_a_fresh_deadline(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    # Slow the first time, as a build after a rebase is, and quick once it is warm.
    monkeypatch.setenv("CHECK_DEADLINE_S", "2")
    first = "test -e built || { touch built; sleep 600; }"
    lane = checks.Lane("build").step("tests", ["sh", "-c", first], tmp_path)
    assert checks.run_lanes([lane], "check", 60) == 0
    out = capsys.readouterr().out
    assert "check build tests: TIMED OUT after" in out
    assert "the deadline killed build; rerunning once with a fresh deadline" in out
    assert "check: passed" in out


def gone(pid: int) -> bool:
    """Whether `pid` has exited (a zombie has), waiting up to a minute for a killed process."""
    ends = time.monotonic() + 60
    while time.monotonic() < ends:
        stat = subprocess.run(
            ["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True
        ).stdout.strip()
        if not stat or stat.startswith("Z"):
            return True
        time.sleep(0.05)
    os.kill(pid, 9)
    return False


def test_baked_in_build_paths_are_refused_with_the_run_time_way(
    tmp_path: Path,
) -> None:
    (tmp_path / "a.rs").write_text(
        '// Never env!("CARGO_MANIFEST_DIR"): a comment may name it.\n'
        'let bin = env!("CARGO_BIN_EXE_beryl_cli");\n'
        'let dir = std::env!( "CARGO_MANIFEST_DIR");\n'
        'let ok = std::env::var_os("CARGO_MANIFEST_DIR");\n'
    )
    found = build_paths.baked_build_paths(tmp_path)
    assert len(found) == 2
    assert found[0].startswith(f"{tmp_path / 'a.rs'}:2: ")
    assert "std::env::current_exe" in found[0]
    assert found[1].startswith(f"{tmp_path / 'a.rs'}:3: ")
    assert 'std::env::var_os("CARGO_MANIFEST_DIR") at run time' in found[1]


def test_the_crates_bake_in_no_build_paths() -> None:
    assert build_paths.baked_build_paths(build_paths.paths.RUST / "crates") == []


def test_an_over_budget_run_passes_and_reports_the_machine_load(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(checks.system, "loadavg", lambda: (53.0, 45.2, 46.1))
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    lane = checks.Lane("slow").step("true", [sys.executable, "-c", "pass"])
    assert checks.run_lanes([lane], "ci", 0) == 0
    assert "ci: load average 53 45 46 on 18 cores" in capsys.readouterr().out


def test_load_beyond_the_cores_stretches_the_clock() -> None:
    assert checks.stretch(10.0, 18) == 1.0
    assert checks.stretch(18.0, 18) == 1.0
    assert checks.stretch(36.0, 18) == 2.0
    assert checks.stretch(1000.0, 18) == checks.MAX_STRETCH


def test_a_loaded_machine_stretches_the_deadline_and_the_budget(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setenv("CHECK_DEADLINE_S", "1")
    lane = checks.Lane("busy").step(
        "sleep", [sys.executable, "-c", "import time; time.sleep(1.5)"]
    )
    # The most load slows the clock to an eighth: 1.5 s of sleep is under a second of the
    # deadline and the budget, with room for a busy machine to start Python.
    monkeypatch.setattr(checks.system, "loadavg", lambda: (1000.0, 1000.0, 1000.0))
    assert checks.run_lanes([lane], "ci", 1) == 0
    assert " s on the clock)" in capsys.readouterr().out
    # On a machine to itself the same step runs out of time.
    monkeypatch.setattr(checks.system, "loadavg", lambda: (4.0, 4.0, 4.0))
    assert checks.run_lanes([lane], "ci", 1) == 1
    assert "ci busy sleep: TIMED OUT after" in capsys.readouterr().out


def test_slow_ci_stops_its_steps_before_enacts_wall_clock_kills_it(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    # by-tkgu: the load stretched the deadline past Enact's kill, which left no log.
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    monkeypatch.setattr(checks.system, "loadavg", lambda: (1000.0, 1000.0, 1000.0))
    monkeypatch.setattr(bisect, "time_left", lambda *args: 1.0)
    monkeypatch.setattr(bisect, "after", lambda *args: None)
    lane = checks.Lane("busy").step(
        "sleep", [sys.executable, "-c", "import time; time.sleep(600)"]
    )
    started = time.monotonic()
    assert checks.slow_run([lane], 300, started) == 1
    assert time.monotonic() - started < 60
    out = capsys.readouterr().out
    assert "ci busy sleep: TIMED OUT after" in out
    assert "before Enact's kill" in out


# A step that fails the first time it runs and passes after, recording each run's arguments.
FLAKY = """
import pathlib, sys
runs = pathlib.Path("runs")
before = runs.read_text() if runs.exists() else ""
runs.write_text(before + " ".join(sys.argv[1:]) + "\\n")
if before and {passes}:
    sys.exit(0)
print("FAILED: plays a land")
print("FAILED: casts a spell")
sys.exit({code})
"""


def flaky_step(passes: bool = True, code: int = 1) -> list[str]:
    return [sys.executable, "-c", FLAKY.format(passes=passes, code=code)]


def runs(tmp_path: Path) -> list[str]:
    return (tmp_path / "runs").read_text().splitlines()


def test_a_step_that_fails_once_reruns_its_failed_checks_and_passes(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    lane = checks.Lane("ui-test").step(
        "ui-test", flaky_step(), tmp_path, rerun=checks.failed_titles
    )
    assert checks.run_lanes([lane], "ci", 60) == 0
    assert runs(tmp_path) == ["", "plays a land casts a spell"]
    out = capsys.readouterr().out
    assert "ci ui-test ui-test: FAILED (exit 1) after" in out
    assert "; rerunning plays a land, casts a spell once" in out
    assert "\nflaky: ui-test plays a land\nflaky: ui-test casts a spell\n" in out
    assert "ci: passed in" in out


def test_a_step_that_cannot_name_its_checks_reruns_whole(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    lane = checks.Lane("app").step("smoke", flaky_step(), tmp_path, rerun=checks.whole)
    assert checks.run_lanes([lane], "ci", 60) == 0
    assert runs(tmp_path) == ["", ""]
    assert "\nflaky: app smoke\n" in capsys.readouterr().out


def test_a_step_that_fails_twice_fails_its_lane_showing_both_runs(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    lane = checks.Lane("ui-test").step(
        "ui-test",
        flaky_step(passes=False),
        tmp_path,
        rerun=checks.failed_titles,
    )
    assert checks.run_lanes([lane], "ci", 60) == 1
    assert len(runs(tmp_path)) == 2
    out = capsys.readouterr().out
    assert out.count("FAILED: plays a land") == 2
    assert out.count("ci ui-test ui-test: FAILED (exit 1)") == 2
    assert "flaky:" not in out
    assert "ci: FAILED: ui-test in" in out


def test_a_build_failure_or_a_static_check_is_not_rerun(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    build = checks.Lane("rust").step(
        "tests",
        flaky_step(code=checks.CANNOT_RUN),
        tmp_path,
        rerun=checks.failed_tests,
    )
    assert checks.run_lanes([build], "ci", 60) == 1
    assert len(runs(tmp_path)) == 1
    (tmp_path / "runs").unlink()
    clippy = checks.Lane("rust").step("clippy", flaky_step(), tmp_path)
    assert checks.run_lanes([clippy], "ci", 60) == 1
    assert len(runs(tmp_path)) == 1
    assert "rerunning" not in capsys.readouterr().out


def test_failed_rust_tests_are_named_as_nextest_filters() -> None:
    output = """
        FAIL [   0.010s] (3/4) beryl_cli::exe also_bad
  stdout ───
    test also_bad ... FAILED
     SIGSEGV [   1.002s] (4/4) beryl_rules tests::crashes
     Summary [   0.011s] 4 tests run: 2 passed, 2 failed, 0 skipped
        FAIL [   0.010s] (3/4) beryl_cli::exe also_bad
"""
    failed = checks.failed_tests(output)
    assert failed == ["beryl_cli::exe also_bad", "beryl_rules tests::crashes"]
    assert checks.nextest_filter(failed) == (
        "(binary_id(beryl_cli::exe) & test(=also_bad))"
        " | (binary_id(beryl_rules) & test(=tests::crashes))"
    )
    # The allocations harness and the doctests run beside nextest, so a failure of either
    # reruns the lot.
    assert checks.failed_tests(output + reruns.ALLOCATIONS_FAILED) == []
    assert checks.failed_tests(output + reruns.DOCTESTS_FAILED) == []


def test_failed_shots_and_oracle_cards_are_named() -> None:
    shot = json.loads((checks.paths.UI / "shots.json").read_text())["shots"][0]["name"]
    shots = f"ui-shots: {shot}: screen: overlap\nui-shots: no-such-shot: x\nui-shots: FAILED\n"
    assert checks.failed_shots(shots) == [shot]
    summary = {
        "xmage": {"scenarios": [{"card": "Ponder", "status": "fail"}]},
        "forge": {
            "scenarios": [
                {"card": "Brainstorm", "status": "allowlisted"},
                {"card": "Daze", "status": "pass", "stale_allowlist": True},
            ]
        },
        "report": "oracle.json",
    }
    oracle = "oracle: noise\n" + json.dumps(summary, indent=2)
    assert checks.failed_cards(oracle) == ["Ponder", "Daze"]
    # Page errors are no check's, so the checks named alone would not show them again.
    assert checks.failed_titles("FAILED: a\npage errors:\n  b: boom\n") == []


def test_failed_script_checks_are_named_by_their_ids() -> None:
    output = (
        "FAILED: script:mana-drain/start/yours\n"
        "    panicked at crates/beryl_session/src/script/build.rs:131: a script breaks a rule\n"
        "FAILED: script:fixtures/stack-two/theirs\n"
        "script-check: 748 checks, 2 failed, in 2.1 s\n"
    )
    assert reruns.failed_scripts(output) == [
        "script:mana-drain/start/yours",
        "script:fixtures/stack-two/theirs",
    ]
    # A panic no check owns would not show again on a rerun of those named alone.
    assert reruns.failed_scripts(output + "FAILED: script:unwatched\n") == []


# A step that fails two checks, then on the rerun runs only the first of them and passes, as a
# title filter that misses a check's block would.
NARROWED = """
import json, os, pathlib
runs = pathlib.Path("runs")
first = not runs.exists()
runs.write_text("ran")
with open(os.environ["BERYL_CHECK_RESULTS"], "a") as f:
    for title in ["plays a land", "casts a spell"] if first else ["plays a land"]:
        outcome = "fail" if first else "pass"
        f.write(json.dumps({"check": "ui-test:" + title, "outcome": outcome, "seconds": 1}))
        f.write("\\n")
        if first:
            print("FAILED: " + title)
raise SystemExit(1 if first else 0)
"""


def test_a_rerun_that_misses_a_failed_check_is_no_flake(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    lane = checks.Lane("ui-test").step(
        "ui-test",
        [sys.executable, "-c", NARROWED],
        tmp_path,
        rerun=checks.failed_titles,
    )
    assert checks.run_lanes([lane], "ci", 60) == 1
    out = capsys.readouterr().out
    assert "the rerun did not pass ui-test:casts a spell: the failure stands" in out
    assert "flaky:" not in out


def test_changed_card_files_pick_cards_only_where_the_oracles_are_built(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    changed = ["rust/cards/cards_1993/lea/counterspell.rs", "tools/x.py"]
    assert checks.changed_cards(changed) == []
    monkeypatch.setattr(oracles, "built", lambda oracle: True)
    assert checks.changed_cards(changed) == ["Counterspell"]
    assert checks.changed_cards(None) == []
    lane = checks.card_oracle_lane(["Counterspell"])
    assert lane.steps[0].cmd[-4:] == ["--no-sim", "--lock-wait", "10", "Counterspell"]
    # Slow CI's lane chooses its cards itself (oracle_selection.py).
    assert checks.oracle_lane().steps[0].cmd[-2:] == ["oracle", "--select"]


def lanes_for(
    monkeypatch: pytest.MonkeyPatch, changed: list[str], windows: bool = False
) -> dict[str, list[list[str]]]:
    """The fast check's lanes for `changed`, as each lane's step commands, on a Mac unless
    `windows`."""
    monkeypatch.setattr(oracles, "built", lambda oracle: False)
    monkeypatch.setattr(checks.system, "WINDOWS", windows)
    return {
        lane.name: [s.cmd for s in lane.steps] for lane in checks.fast_lanes(changed)
    }


def scale_tests_run(lanes: dict[str, list[list[str]]]) -> int:
    return sum("tests/test_scale.py" in cmd for cmds in lanes.values() for cmd in cmds)


def test_a_card_or_vocabulary_change_runs_the_tests_that_read_card_files(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for path in [
        "rust/cards/cards_1993/lea/counterspell.rs",
        "rust/crates/beryl_card/src/effect/mod.rs",
        "rust/crates/beryl_cards/Cargo.toml",
    ]:
        lanes = lanes_for(monkeypatch, [path])
        assert scale_tests_run(lanes) == 1, path
        assert "python" not in lanes  # No black or pyre for a Rust-only change.
    # The engine's files are not read by any Python test.
    assert (
        scale_tests_run(lanes_for(monkeypatch, ["rust/crates/beryl_rules/src/turn.rs"]))
        == 0
    )


def test_a_change_under_tools_runs_the_python_lane_and_the_scale_tests_once(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    lanes = lanes_for(
        monkeypatch,
        ["tools/src/beryl_tools/scale.py", "rust/cards/cards_1993/lea/a.rs"],
    )
    assert "scale" not in lanes
    # The python lane's pytest names no files, so it runs every test, test_scale's once.
    assert lanes["python"][-1][-2:] == ["beryl_tools.checks", "pytest"]
    assert scale_tests_run(lanes) == 0


def test_the_script_checks_run_for_a_change_to_a_crate_a_script_comes_from(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for path in [
        "rust/crates/beryl_session/src/script/build.rs",
        "rust/crates/beryl_kernel/src/lib.rs",
        "rust/cards/cards_1993/lea/counterspell.rs",
        "ui/fixtures.json",
        "data/card-store.tsv",
    ]:
        assert "script" in lanes_for(monkeypatch, [path]), path
    # The app shell and the benchmarks build no script beryl_cli plays.
    for path in [
        "rust/crates/beryl_app/src/lib.rs",
        "rust/crates/beryl_bench/src/a.rs",
    ]:
        assert "script" not in lanes_for(monkeypatch, [path]), path


def test_a_refreshed_card_store_runs_the_lanes_that_read_it(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    lanes = lanes_for(monkeypatch, ["data/card-store.tsv"])
    assert {"python", "rust"} <= set(lanes)


def test_a_symlink_checked_out_or_committed_as_a_file_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def git(*args: str) -> None:
        subprocess.run(
            ["git", "-c", "core.symlinks=true", *args], cwd=tmp_path, check=True
        )

    git("init", "-q")
    (tmp_path / "target").mkdir()
    try:
        (tmp_path / "link").symlink_to("target", target_is_directory=True)
    except OSError:
        pytest.skip("this machine cannot make symlinks")
    git("add", "link")
    git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "feat: link")
    monkeypatch.setattr(checks.paths, "REPO", tmp_path)
    base = checks.git("rev-parse", "HEAD")
    assert hygiene.symlink_problems(base) == []
    (tmp_path / "link").unlink()
    (tmp_path / "link").write_text("target")
    assert [p.split(":")[0] for p in hygiene.symlink_problems(base)] == ["link"]
    git("add", "link")
    git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "fix: oops")
    assert "committed a symlink as a file" in " ".join(hygiene.symlink_problems(base))


def test_the_checkout_keeps_its_symlinks() -> None:
    assert hygiene.symlink_problems("") == []


def test_the_motion_capture_runs_when_the_stage_or_its_data_change(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for path in [
        "ui/src/stage/desktop.ts",
        "data/ui-conventions.json",
        "data/arena-motion.json",
        "tools/src/beryl_tools/video_measure.py",
    ]:
        assert "motion" in lanes_for(monkeypatch, [path]), path
    for path in ["ui/src/game/Board.tsx", "data/card-store.tsv", "tools/x.py"]:
        assert "motion" not in lanes_for(monkeypatch, [path]), path


def test_windows_leaves_the_browser_checks_to_the_mac(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    changed = ["ui/src/stage/verbs.ts", "rust/crates/beryl_rules/src/turn.rs"]
    mac = lanes_for(monkeypatch, changed)
    assert {"ui-test", "shots"} <= set(mac)
    windows = lanes_for(monkeypatch, changed, windows=True)
    assert not {"ui-test", "shots", "motion"} & set(windows)
    assert {"ui", "rust"} <= set(windows)


def test_the_fast_check_tests_the_packages_a_change_can_break() -> None:
    def package(name: str, *deps: str) -> dict[str, object]:
        return {
            "name": name,
            "manifest_path": str(paths.RUST / "crates" / name / "Cargo.toml"),
            "dependencies": [{"name": d, "path": f"../{d}"} for d in deps]
            + [{"name": "serde"}],
        }

    packages: list[dict[str, object]] = [
        package("kernel"),
        package("rules", "kernel"),
        package("search", "rules"),
        package("cli", "search"),
        package("bench", "search", "kernel"),
    ]

    def affected(*changed: str) -> list[str] | None:
        return rust_tests.affected_packages(list(changed), packages)

    assert affected("rust/crates/cli/src/main.rs") == ["cli"]
    assert affected("rust/crates/search/src/lib.rs", "tools/x.py") == [
        "bench",
        "cli",
        "search",
    ]
    # Everything depends on the kernel; the workspace's own files build every crate.
    assert affected("rust/crates/kernel/src/lib.rs") is None
    assert affected("rust/Cargo.lock") is None
    assert affected("data/card-store.tsv") is None
    assert rust_tests.affected_packages(None, packages) is None


def test_a_session_runs_rust_tests_as_the_check_does() -> None:
    command = rust_tests.session_command
    # The whole workspace is the check's own step.
    assert command([], nextest=True) is None
    # A package or a name filter goes to nextest, kept off the harness-less targets.
    assert command(["-p", "beryl_session", "corpus"], nextest=True) == [
        "cargo",
        "nextest",
        "run",
        "--cargo-quiet",
        "--no-fail-fast",
        "--status-level=fail",
        "--final-status-level=fail",
        "-E",
        "not binary(allocations)",
        "-p",
        "beryl_session",
        "corpus",
    ]
    # A filterset of the caller's own would be joined to ours by "or": theirs stands alone.
    own = command(["-E", "test(corpus)"], nextest=True)
    assert own is not None and own.count("-E") == 1
    # What nextest cannot run is cargo test's, and everything is where nextest is missing.
    for args in (
        ["--doc"],
        ["--test", "allocations"],
        ["-p", "beryl_engine", "--test=allocations"],
        ["corpus", "--", "--nocapture"],
    ):
        assert command(args, nextest=True) == ["cargo", "test", *args]
    assert command([], nextest=False) == ["cargo", "test"]
    assert command(["-p", "beryl_app"], nextest=False) == [
        "cargo",
        "test",
        "-p",
        "beryl_app",
    ]
