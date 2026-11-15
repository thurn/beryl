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


def test_reaped_growth_pays_what_is_owed_before_counting_the_rest() -> None:
    sampler = Sampler()
    wt = "/Users/x/beryl/.worktrees/by-abc"
    cargo = proc(10, 1, 1.0, "/usr/bin/cargo", cwd=wt)
    cargo.children = 0.0
    test = proc(11, 10, 0.0, "/x/target/debug/deps/beryl-123", cwd=wt)
    test.children = 0.0
    sampler.step({10: cargo, 11: test}, {10: wt, 11: wt})  # Baseline.
    test.cpu = 3.0
    used, _ = sampler.step({10: cargo, 11: test}, {})
    assert used == {("session", "rust_test", "by-abc"): 3.0}
    # The test ran 2 s more and exited; cargo reaped it, and a 4 s child nobody sampled.
    cargo.children = 5.0 + 4.0
    used, _ = sampler.step({10: cargo}, {})
    # 3 s were counted already; the test's last 2 s and the unseen 4 s are charged to the
    # test's key, the most recently exited child's.
    assert used == {("session", "rust_test", "by-abc"): 6.0}
    assert sampler.owed == {10: 0.0} and 10 not in sampler.tail


def test_a_parent_and_child_exiting_together_owe_the_grandparent() -> None:
    sampler = Sampler()
    shell = proc(5, 1, 0.0, "/bin/zsh")
    shell.children = 0.0
    cargo = proc(10, 5, 1.0, "/usr/bin/cargo")
    cargo.children = 0.0
    rustc = proc(11, 10, 2.0, "/usr/bin/rustc")
    rustc.children = 0.0
    sampler.step({5: shell, 10: cargo, 11: rustc}, {})  # Baseline: 3 s before it.
    rustc.cpu = 4.0
    used, _ = sampler.step({5: shell, 10: cargo, 11: rustc}, {})
    assert sum(used.values()) == 2.0
    # Both exit; the shell reaps cargo, whose own 1 s and reaped 4 s (rustc) arrive at once.
    # The owed figures are whole lifetimes, cargo's 1 s and rustc's 4 s, although only rustc's
    # 2 s since the baseline were reported: the shell's reaped 5 s include the time before the
    # baseline too, so all of it is paid and nothing is new.
    shell.children = 1.0 + 4.0
    used, _ = sampler.step({5: shell}, {})
    assert used == {} and sampler.owed == {5: 0.0}


def test_a_child_reaped_after_a_delay_is_paid_when_the_parent_reaps_it() -> None:
    sampler = Sampler()
    cargo = proc(10, 1, 0.0, "/usr/bin/cargo")
    cargo.children = 0.0
    rustc = proc(11, 10, 0.0, "/usr/bin/rustc")
    rustc.children = 0.0
    sampler.step({10: cargo, 11: rustc}, {})  # Baseline.
    rustc.cpu = 2.0
    sampler.step({10: cargo, 11: rustc}, {})
    # rustc exits, but cargo has not reaped it yet: nothing new, and 2 s are owed.
    used, _ = sampler.step({10: cargo}, {})
    assert used == {} and sampler.owed == {10: 2.0}
    # cargo reaps it: rustc's last 0.5 s is new, and charged to rustc's key.
    cargo.children = 2.5
    used, _ = sampler.step({10: cargo}, {})
    assert used == {("other", "compile", "-"): 0.5}


def test_a_zombie_child_is_not_counted_twice_when_reaped() -> None:
    sampler = Sampler()
    app = proc(10, 1, 0.0, "/usr/bin/node")
    app.children = 0.0
    child = proc(11, 10, 0.0, "/usr/bin/node")
    child.children = 0.0
    sampler.step({10: app, 11: child}, {})  # Baseline.
    child.cpu = 8000.0
    used, _ = sampler.step({10: app, 11: child}, {})
    assert sum(used.values()) == 8000.0
    # The child exits and waits to be reaped. `proc_pid_rusage` cannot read the zombie, so
    # its CPU is `ps`'s, which is zero for a zombie: that adds nothing.
    zombie = proc(11, 10, 0.0, "<defunct>")
    used, _ = sampler.step({10: app, 11: zombie}, {})
    assert used == {}
    # The app reaps it: its whole 8,000 s were counted already, and its last second is new.
    app.children = 8001.0
    used, _ = sampler.step({10: app}, {})
    assert sum(used.values()) == 1.0


def test_a_pid_with_a_new_start_time_is_a_new_process() -> None:
    sampler = Sampler()
    shell = proc(5, 1, 0.0, "/bin/zsh")
    shell.children = 0.0
    old = proc(11, 5, 0.0, "/usr/bin/rustc")
    old.start = "Sun Nov 15 06:54:46 2026"
    sampler.step({5: shell, 11: old}, {})  # Baseline.
    old.cpu = 30.0
    sampler.step({5: shell, 11: old}, {})
    # Between two samples rustc exits, the shell reaps it (31 s), and a new process takes its
    # pid and runs for 2 s: rustc's last second and the new process's 2 s are new.
    shell.children = 31.0
    new = proc(11, 5, 2.0, "/usr/bin/git")
    new.start = "Sun Nov 15 07:10:02 2026"
    used, _ = sampler.step({5: shell, 11: new}, {})
    assert used == {("other", "compile", "-"): 1.0, ("other", "tool", "-"): 2.0}
