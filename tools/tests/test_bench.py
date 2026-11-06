from beryl_tools import bench

TOP = """Processes: 812 total
CPU usage: 15.0% user, 16.28% sys, 68.71% idle
Processes: 812 total
CPU usage: 2.1% user, 3.4% sys, 94.5% idle
"""

PMSET = """System-wide power settings:
Currently in use:
 standby              1
 powernap             1
 powermode            2
"""


def quiet() -> bench.Machine:
    return bench.Machine(busy_percent=4.0, on_battery=False, low_power=False)


def test_busy_comes_from_the_last_sample() -> None:
    assert abs(bench.parse_busy(TOP) - 5.5) < 1e-9
    assert bench.parse_busy("") == 100.0


def test_power_settings() -> None:
    assert not bench.parse_low_power(PMSET)  # 2 is High Power
    assert bench.parse_low_power(PMSET.replace("powermode            2", "powermode 1"))
    assert bench.parse_low_power(" lowpowermode         1\n")
    assert bench.parse_battery("Now drawing from 'Battery Power'\n")
    assert not bench.parse_battery("Now drawing from 'AC Power'\n")


def test_throttling() -> None:
    assert not bench.parse_throttled(
        "Note: No thermal warning level has been recorded\n"
    )
    assert bench.parse_throttled("Thermal Warning Level = 2\nCPU_Speed_Limit = 80\n")
    assert not bench.parse_throttled("CPU_Speed_Limit = 100\n")


def test_a_quiet_machine_has_no_problems() -> None:
    assert bench.problems(quiet()) == []


def test_each_kind_of_noise_is_reported() -> None:
    noisy = bench.Machine(
        busy_percent=30.0,
        on_battery=True,
        low_power=True,
        throttled=["CPU_Speed_Limit = 80"],
        builds=["rustc"],
        busiest=["84.8 ruby"],
    )
    found = bench.problems(noisy)
    assert len(found) == 5
    assert "84.8 ruby" in found[0]


def test_cargo_flags_go_before_the_harness_arguments() -> None:
    assert bench.cargo_bench(["--bench", "x", "--", "--quick"], "--no-run") == [
        "cargo",
        "bench",
        "--locked",
        "--no-run",
        "--bench",
        "x",
        "--",
        "--quick",
    ]


def test_timing_builds_the_search_profile_unless_told_otherwise() -> None:
    assert bench.timing_build(["--bench", "x", "--", "--quick"]) == (
        ["--profile", "search"],
        {"RUSTFLAGS": bench.SEARCH_CPU},
    )
    assert bench.timing_build(["--profile", "bench"]) == ([], {})
    assert bench.timing_build(["--profile=release", "--bench", "x"]) == ([], {})
    # A harness argument named --profile isn't cargo's.
    assert bench.timing_build(["--", "--profile", "x"])[0] == ["--profile", "search"]


def test_runner_version_comes_from_the_lockfile() -> None:
    lock = '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n'
    assert bench.locked_version(lock, "gungraun") == "0.20.0"
    assert bench.locked_version(lock, "gungraun-runner") is None


def test_counting_holds_the_vm_lock_and_a_home_of_its_own() -> None:
    script = bench.count_script("provision\n", ["cargo", "bench"], "by-1")
    lock, count = script.index("flock 9"), script.index("cargo bench")
    assert lock < script.index("provision") < count
    assert "GUNGRAUN_HOME=$HOME/gungraun/by-1 " in script
