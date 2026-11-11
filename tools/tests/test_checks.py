import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from beryl_tools import checks


def test_only_lanes_that_build_warm_the_caches(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    warmed: list[bool] = []
    monkeypatch.setattr(checks.warm, "ensure", lambda: warmed.append(True))
    docs = checks.Lane("docs").step("true", ["true"])
    assert checks.run_lanes([docs], "check", 60, strict=False) == 0
    assert warmed == []
    build = checks.Lane("build", warm=True).step("true", ["true"])
    assert checks.run_lanes([docs, build], "check", 60, strict=False) == 0
    assert warmed == [True]


def test_card_names_are_found_as_whole_words(tmp_path: Path) -> None:
    (tmp_path / "a.ts").write_text(
        'const PONDER = "Ponder";\n// Pondering, and Jace, the Mind Sculptor\'s +2\n'
    )
    names = ["Ponder", "Jace, the Mind Sculptor", "Jace"]
    assert checks.card_names_in(tmp_path, names) == [
        f"{tmp_path / 'a.ts'}:1: Ponder",
        f"{tmp_path / 'a.ts'}:2: Jace, the Mind Sculptor",
    ]


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
    assert checks.run_lanes([lane], "check", 60, strict=False) == 1
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
    found = checks.baked_build_paths(tmp_path)
    assert len(found) == 2
    assert found[0].startswith(f"{tmp_path / 'a.rs'}:2: ")
    assert "std::env::current_exe" in found[0]
    assert found[1].startswith(f"{tmp_path / 'a.rs'}:3: ")
    assert 'std::env::var_os("CARGO_MANIFEST_DIR") at run time' in found[1]


def test_the_crates_bake_in_no_build_paths() -> None:
    assert checks.baked_build_paths(checks.paths.RUST / "crates") == []


def test_an_over_budget_run_reports_the_machine_load(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(checks.system, "loadavg", lambda: (53.0, 45.2, 46.1))
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    lane = checks.Lane("slow").step("true", [sys.executable, "-c", "pass"])
    assert checks.run_lanes([lane], "ci", 0, strict=True) == 1
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
    # Twice the cores' worth of load halves the clock's pace: 1.5 s of wall time is under a
    # second of the deadline, and of the budget.
    monkeypatch.setattr(checks.system, "loadavg", lambda: (36.0, 36.0, 36.0))
    assert checks.run_lanes([lane], "ci", 1, strict=True) == 0
    assert "on the clock, the machine loaded" in capsys.readouterr().out
    # On a machine to itself the same step runs out of time.
    monkeypatch.setattr(checks.system, "loadavg", lambda: (4.0, 4.0, 4.0))
    assert checks.run_lanes([lane], "ci", 1, strict=True) == 1
    assert "ci busy sleep: TIMED OUT after" in capsys.readouterr().out


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
    assert checks.run_lanes([lane], "ci", 60, strict=True) == 0
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
    assert checks.run_lanes([lane], "ci", 60, strict=True) == 0
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
    assert checks.run_lanes([lane], "ci", 60, strict=True) == 1
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
    assert checks.run_lanes([build], "ci", 60, strict=True) == 1
    assert len(runs(tmp_path)) == 1
    (tmp_path / "runs").unlink()
    clippy = checks.Lane("rust").step("clippy", flaky_step(), tmp_path)
    assert checks.run_lanes([clippy], "ci", 60, strict=True) == 1
    assert len(runs(tmp_path)) == 1
    assert "rerunning" not in capsys.readouterr().out


def test_failed_rust_tests_are_named_as_nextest_filters() -> None:
    output = """
        FAIL [   0.010s] (3/4) beryl_cli::exe also_bad
  stdout ───
    test also_bad ... FAILED
     SIGSEGV [   1.002s] (4/4) beryl_engine tests::crashes
     Summary [   0.011s] 4 tests run: 2 passed, 2 failed, 0 skipped
        FAIL [   0.010s] (3/4) beryl_cli::exe also_bad
"""
    failed = checks.failed_tests(output)
    assert failed == ["beryl_cli::exe also_bad", "beryl_engine tests::crashes"]
    assert checks.nextest_filter(failed) == (
        "(binary_id(beryl_cli::exe) & test(=also_bad))"
        " | (binary_id(beryl_engine) & test(=tests::crashes))"
    )
    # The allocations harness runs beside nextest, so its failure reruns the lot.
    assert checks.failed_tests(output + checks.ALLOCATIONS_FAILED) == []


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
