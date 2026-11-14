import json
import subprocess
from pathlib import Path

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


def plugged_in() -> bench.Machine:
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


def test_load_alone_is_no_reason_to_refuse() -> None:
    assert bench.problems(plugged_in()) == []
    busy = bench.Machine(
        busy_percent=95.0,
        on_battery=False,
        low_power=False,
        builds=["rustc"],
        busiest=["84.8 ruby"],
    )
    assert bench.problems(busy) == []
    said = bench.load(busy)
    assert "95%" in said and "84.8 ruby" in said and "rustc" in said


def test_each_slowed_cpu_is_reported() -> None:
    slowed = bench.Machine(
        busy_percent=30.0,
        on_battery=True,
        low_power=True,
        throttled=["CPU_Speed_Limit = 80"],
    )
    assert len(bench.problems(slowed)) == 3


def test_cargo_flags_go_before_the_harness_arguments() -> None:
    assert bench.cargo_bench(["--bench", "x", "--", "--quick"], "--no-run") == [
        "cargo",
        "bench",
        "--locked",
        "-p",
        "beryl_bench",
        "--no-run",
        "--bench",
        "x",
        "--",
        "--quick",
    ]


def test_benchmarks_build_their_package_alone_unless_told_otherwise() -> None:
    # From the whole workspace, test-only features such as `coverage` would be built in.
    assert bench.cargo_bench(["--bench", "x"])[3:5] == ["-p", "beryl_bench"]
    assert "beryl_bench" not in bench.cargo_bench(["-p", "other", "--bench", "x"])
    assert "beryl_bench" not in bench.cargo_bench(["--package=other"])
    # A harness argument named -p isn't cargo's.
    assert bench.cargo_bench(["--", "-p", "x"])[3:5] == ["-p", "beryl_bench"]


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


def test_counting_holds_the_vm_lock_and_dirs_of_its_own() -> None:
    script = bench.count_script(
        "provision\n", ["cargo", "bench"], "by-1", "/repo/.worktrees/by-1", ["by-1"]
    )
    lock, count = script.index("flock 9"), script.index("cargo bench")
    assert lock < script.index("provision") < script.index("cargo clean") < count
    assert (
        "CARGO_TARGET_DIR=$HOME/targets/by-1 GUNGRAUN_HOME=$HOME/gungraun/by-1 "
        in script
    )


def run_count(home: Path, name: str, checkout: str, live: list[str]) -> list[str]:
    """Runs the VM script on this machine with stub `flock` and `cargo` that log their
    arguments, returning the cargo calls."""
    bin = home / "bin"
    bin.mkdir(exist_ok=True)
    for tool in ("flock", "cargo"):
        (bin / tool).write_text(f'#!/bin/sh\necho "$*" >>"$HOME/{tool}.log"\n')
        (bin / tool).chmod(0o755)
    (home / "cargo.log").write_text("")
    script = bench.count_script("", ["cargo", "bench"], name, checkout, live)
    env = {"HOME": str(home), "PATH": f"{bin}:/usr/bin:/bin"}
    subprocess.run(["bash", "-c", script], env=env, check=True)
    return (home / "cargo.log").read_text().splitlines()


def test_target_dirs_rebuild_for_a_new_checkout_and_go_with_their_worktree(
    tmp_path: Path,
) -> None:
    gate = bench.GATE_NAME
    (tmp_path / "targets" / gate / "release").mkdir(parents=True)
    (tmp_path / "targets" / "gone").mkdir()
    (tmp_path / "gungraun" / "gone").mkdir(parents=True)

    calls = run_count(tmp_path, gate, "/repo/a", ["a"])
    assert calls[0].startswith("clean ") and "--profile bench" in calls[0]
    assert run_count(tmp_path, gate, "/repo/a", ["a"]) == ["bench"]  # same owner: warm
    assert run_count(tmp_path, gate, "/repo/b", ["a", "b"])[0].startswith("clean ")
    assert not (tmp_path / "targets" / "gone").exists()
    assert not (tmp_path / "gungraun" / "gone").exists()

    # A worktree's first run copies the gate's warm dir, then rebuilds its own code.
    assert run_count(tmp_path, "a", "/repo/a", ["a"])[0].startswith("clean ")
    assert (tmp_path / "targets" / "a" / "release").is_dir()
    assert run_count(tmp_path, "a", "/repo/a", ["a"]) == ["bench"]
    assert sorted(p.name for p in (tmp_path / "targets").iterdir()) == ["a", gate]


def test_a_new_owner_cleans_the_profile_it_builds() -> None:
    def clean(cargo: list[str]) -> str:
        script = bench.count_script("", cargo, "by-1", "/repo", [])
        return next(line for line in script.splitlines() if "cargo clean" in line)

    assert "--profile bench " in clean(["cargo", "bench", "--", "--profile", "x"])
    assert "--profile search " in clean(["cargo", "bench", "--profile", "search"])
    assert "--profile dev " in clean(["cargo", "bench", "--profile=dev"])


GATE = bench.Gate("rollout_counts::cube_rollout::rollouts::corpus", 1_000_000, 1.0)


def test_the_gate_file_parses_and_names_a_counts_benchmark() -> None:
    gate = bench.load_gate(bench.GATE_FILE.read_text())
    assert gate.benchmark.split("::")[0].endswith("_counts")
    assert gate.instructions > 0 and 0 < gate.tolerance_percent < 5


def test_the_gate_counts_only_its_benchmark_as_json() -> None:
    cmd = bench.gate_cargo(GATE)
    assert cmd[cmd.index("--bench") + 1] == "rollout_counts"
    assert cmd[cmd.index("-p") + 1] == "beryl_bench"
    harness = cmd[cmd.index("--") + 1 :]
    assert harness == ["--output-format=json", GATE.benchmark]


def summary(module_path: str, ir: int) -> str:
    metrics = {"Ir": {"values": {"new": ir}}}
    profile = {"tool": "Callgrind", "data": {"total": {"metrics": metrics}}}
    return json.dumps(
        {"module_path": module_path, "id": "corpus", "profiles": [profile]}
    )


def test_the_count_comes_from_the_gated_benchmark_summary() -> None:
    out = "\n".join(
        [
            "provisioning noise",
            summary("rollout_counts::cube_rollout::next_apply", 5),
            "{not json",
            summary("rollout_counts::cube_rollout::rollouts", 1_004_000),
        ]
    )
    assert bench.counted(out, GATE.benchmark) == 1_004_000
    assert bench.counted("nothing here\n", GATE.benchmark) is None


def test_a_rise_past_the_tolerance_fails_and_a_drop_asks_for_a_lower_bar() -> None:
    assert bench.judge(GATE, 1_010_000)[0]  # exactly at the tolerance
    ok, message = bench.judge(GATE, 1_010_001)
    assert not ok and "count-gate.toml" in message
    ok, message = bench.judge(GATE, 980_000)
    assert ok and "lower" in message
    ok, message = bench.judge(GATE, 995_000)
    assert ok and "lower" not in message


def test_the_gate_file_gives_each_family_a_bar() -> None:
    bars = bench.load_families(bench.GATE_FILE.read_text())
    assert [b.name for b in bars][:2] == ["blank", "attachments"]
    assert all(1.0 <= b.ratio <= 2.0 for b in bars)


def test_a_family_counts_both_sides_then_prints_its_pairing() -> None:
    counts, steps = bench.family_cargo(["attachments"])
    assert counts[counts.index("--bench") + 1] == "family_counts"
    assert counts[-1] == "family_counts::present_cost::*::attachments"
    assert steps[steps.index("--bench") + 1] == "family_steps"
    assert steps[-1] == "attachments"


def test_a_family_reads_both_counts_and_its_pairing() -> None:
    def side(name: str, ir: int) -> str:
        metrics = {"Ir": {"values": {"new": ir}}}
        profile = {"tool": "Callgrind", "data": {"total": {"metrics": metrics}}}
        return json.dumps(
            {
                "module_path": f"family_counts::present_cost::{name}",
                "id": "attachments",
                "profiles": [profile],
            }
        )

    out = "\n".join(
        [
            side("present", 1_100),
            side("control", 1_000),
            '{"family": "attachments", "positions": 12, "paired": 90, "skipped": 10}',
        ]
    )
    present = bench.counted(out, bench.family_benchmark("present", "attachments"))
    control = bench.counted(out, bench.family_benchmark("control", "attachments"))
    assert (present, control) == (1_100, 1_000)
    assert bench.pairing(out, "attachments") == bench.Pairing(90, 10)
    assert bench.pairing(out, "blank") is None


def test_a_family_fails_over_its_ratio_or_with_a_tenth_skipped() -> None:
    bar = bench.FamilyBar("attachments", 1.25)
    assert bench.judge_family(bar, 1_250, 1_000, bench.Pairing(90, 10))[0]
    ok, message = bench.judge_family(bar, 1_251, 1_000, bench.Pairing(90, 10))
    assert not ok and "over the bar" in message
    ok, message = bench.judge_family(bar, 1_000, 1_000, bench.Pairing(89, 11))
    assert not ok and "skipped" in message


def test_the_counts_lane_gates_every_family_in_one_step() -> None:
    steps = {s.name: s.cmd for s in bench.lane().steps}
    assert steps["count-gate"][-1] == "count-gate"
    families = steps["count-gate-families"]
    assert families[families.index("--family") + 1] == "blank"
