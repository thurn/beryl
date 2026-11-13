import json
import time

from beryl_tools import cpu
from beryl_tools.cpu import Process, Sampler


def proc(
    pid: int, ppid: int, seconds: float, exe: str, args: str = "", cwd: str = ""
) -> Process:
    return Process(pid, ppid, seconds, exe, args or exe, cwd)


def test_parse_cputime() -> None:
    assert cpu.parse_cputime("0:07.70") == 7.7
    assert cpu.parse_cputime("166:27.11") == 166 * 60 + 27.11
    assert cpu.parse_cputime("1:02:03.00") == 3723.0


def test_context_kind_and_worktree() -> None:
    wt = "/Users/x/beryl/.worktrees/by-abc"
    assert cpu.classify(proc(5, 4, 0, "/usr/bin/rustc", cwd=wt), None) == (
        "session",
        "compile",
        "by-abc",
    )
    ci = proc(
        5,
        4,
        0,
        "/usr/bin/python3",
        "python -m beryl_tools.cli ci",
        "/Users/x/beryl/.worktrees/ci-1234",
    )
    assert cpu.classify(ci, None)[0] == "slow_ci"
    web = proc(9, 1, 0, "/System/WebKitWebContent")
    assert cpu.classify(web, None) == ("browser", "browser", "-")
    stranger = proc(9, 2, 0, "/opt/mystery")
    assert cpu.classify(stranger, None) == ("other", "other", "-")


def test_a_process_without_a_cwd_takes_its_parents_context() -> None:
    parent = ("fast_check", "tool", "by-abc")
    child = proc(7, 6, 0, "/usr/bin/rustc")
    assert cpu.classify(child, parent) == ("fast_check", "compile", "by-abc")


def test_sampler_counts_growth_and_new_processes_after_the_baseline() -> None:
    sampler = Sampler()
    wt = "/Users/x/beryl/.worktrees/by-abc"
    first = {
        1: proc(1, 0, 100.0, "/sbin/launchd"),
        2: proc(2, 1, 50.0, "/usr/bin/rustc", cwd=wt),
    }
    used, _ = sampler.step(first, {2: wt})
    assert used == {}  # Baseline: nothing counted.
    second = {
        1: proc(1, 0, 100.0, "/sbin/launchd"),
        2: proc(2, 1, 52.5, "/usr/bin/rustc", cwd=wt),
        3: proc(3, 0, 1.5, "/opt/mystery"),
    }
    used, unnamed = sampler.step(second, {3: ""})
    assert used[("session", "compile", "by-abc")] == 2.5
    assert used[("other", "other", "-")] == 1.5
    assert unnamed == {"mystery": 1.5}
    # Process 2 exits: nothing is added for it, and its entry is dropped.
    used, _ = sampler.step({1: second[1]}, {})
    assert used == {} and 2 not in sampler.last


def test_report_sums_rows(own_cache: object) -> None:
    now = time.time()
    rows = [
        {
            "time": now - 60 * i,
            "seconds": 60.0,
            "cores": 18,
            "cpu": {"slow_ci/compile/ci-1": 120.0, "other/other/-": 6.0},
            "unnamed": {"mystery": 6.0},
            "peak_cores": 4.0,
            "top_cores": 2.0,
            "load": 3.0,
        }
        for i in range(3)
    ]
    cpu.samples_file().write_text("".join(json.dumps(r) + "\n" for r in rows))
    text = cpu.summarize(cpu.read_rows(now - 3600))
    assert "3 minutes, 18 cores" in text
    assert "slow_ci" in text and "mystery" in text
    assert "named 95%" in text
    assert cpu.summarize([]).startswith("no samples")
