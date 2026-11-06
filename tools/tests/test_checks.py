import os
import subprocess
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
    monkeypatch.setattr(os, "getloadavg", lambda: (53.0, 45.2, 46.1))
    monkeypatch.setattr(os, "cpu_count", lambda: 18)
    lane = checks.Lane("slow").step("true", ["true"])
    assert checks.run_lanes([lane], "ci", 0, strict=True) == 1
    assert "ci: load average 53 45 46 on 18 cores" in capsys.readouterr().out
