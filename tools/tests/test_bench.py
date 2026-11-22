import argparse
import json
import subprocess
import sys
import tarfile
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

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
    text = bench.GATE_FILE.read_text()
    gate = bench.load_gate(text)
    windows = bench.load_gate(text, windows=True)
    assert gate.benchmark.split("::")[0].endswith("_counts")
    assert gate.instructions > 0 and 0 < gate.tolerance_percent < 5
    assert windows.benchmark == gate.benchmark
    assert windows.instructions > 0 and windows.instructions != gate.instructions
    assert windows.tolerance_percent == gate.tolerance_percent


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


def test_a_run_someone_waits_for_goes_before_a_background_run() -> None:
    """The shell's order is checked by hand in the VM (macOS has no `flock`); this pins the
    protocol: a waited-for run holds the turn lock shared before it waits for the VM, and a
    background run takes the VM only while it can hold the turn lock alone, then lets it go.
    """
    waited = bench.locked("work", background=False)
    assert (
        waited.index("flock -s 8") < waited.index("flock -n 9") < waited.index("work")
    )
    assert bench.VM_TURN in waited and bench.VM_LOCK in waited
    swept = bench.locked("work", background=True)
    loop = swept[swept.index("while :") : swept.index("done")]
    assert (
        loop.index("flock -x 8") < loop.index("flock -n 9") < loop.index("flock -u 8")
    )
    assert "flock -s" not in swept and swept.endswith("work")


def test_the_gate_drops_last_runs_callgrind_outputs_but_a_worktree_keeps_them(
    tmp_path: Path,
) -> None:
    # An unclean VM stop once left the gate's last outputs starting with zero bytes, and
    # Gungraun failed to parse them on every later run.
    gate = tmp_path / "gungraun" / bench.GATE_NAME / "old.out.old"
    mine = tmp_path / "gungraun" / "a" / "old.out.old"
    for out in (gate, mine):
        out.parent.mkdir(parents=True)
        out.write_bytes(b"\0" * 8)
    run_count(tmp_path, bench.GATE_NAME, "/repo", ["a"])
    run_count(tmp_path, "a", "/repo/a", ["a"])
    assert not gate.exists()
    assert mine.exists()


LIMA_YAML = """images:
- location: "https://cloud-images.ubuntu.com/releases/ubuntu-arm64.img"
  arch: "aarch64"
mounts:
- location: /Users/me/beryl
  mountPoint: /Users/me/beryl
  writable: false
# - location: "{{.GlobalTempDir}}/lima"
"""


def test_the_vm_sees_only_checkouts_under_its_mounts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (tmp_path / ".lima" / "vm").mkdir(parents=True)
    (tmp_path / ".lima" / "vm" / "lima.yaml").write_text(LIMA_YAML)
    monkeypatch.delenv("LIMA_HOME", raising=False)
    monkeypatch.setattr(bench.Path, "home", lambda: tmp_path)
    monkeypatch.setattr(bench.system, "WINDOWS", False)
    # Image URLs and commented-out mounts are not mounts.
    assert bench.vm_host.vm_mounts("vm") == [Path("/Users/me/beryl")]
    assert bench.vm_host.vm_sees(Path("/Users/me/beryl/.worktrees/a/rust"), "vm")
    # A second clone, such as the one the tracker cutover pushed from, is not visible.
    assert not bench.vm_host.vm_sees(Path("/Users/me/.config/rollout/beryl/rust"), "vm")
    # When the mount cannot be confirmed, the count sends the checkout.
    assert not bench.vm_host.vm_sees(Path("/anywhere"), "missing")


def test_the_gate_counts_the_current_files_of_an_unmounted_worktree(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    if sys.platform == "win32":
        pytest.skip("Windows counts use WSL")
    main, lease = tmp_path / "main", tmp_path / "trees" / "lease"
    (main / "rust").mkdir(parents=True)
    (main / ".gitignore").write_text("/rust/target/\n")
    (main / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "stable"\n')
    (main / "rust" / "Cargo.lock").write_text(
        '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n'
    )
    (main / "rust" / "deleted.rs").touch()
    git = [
        "git",
        "-C",
        str(main),
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@example.com",
    ]
    subprocess.run([*git, "init", "-q"], check=True)
    subprocess.run([*git, "add", "."], check=True)
    subprocess.run(
        [*git, "-c", "commit.gpgsign=false", "commit", "-qm", "source"], check=True
    )
    subprocess.run([*git, "worktree", "add", "-q", "--detach", str(lease)], check=True)
    (lease / "rust" / "deleted.rs").unlink()
    (lease / "rust" / "added file.rs").write_text("lease sources")
    (lease / "rust" / "target").mkdir()
    (lease / "rust" / "target" / "artifact").touch()
    (lease / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "beta"\n')

    lima_home = tmp_path / "lima"
    (lima_home / bench.VM).mkdir(parents=True)
    (lima_home / bench.VM / "lima.yaml").write_text(
        LIMA_YAML.replace("/Users/me/beryl", str(main))
    )
    gate = tmp_path / "gate.toml"
    gate.write_text(
        f'benchmark = "{GATE.benchmark}"\n'
        f"instructions = {GATE.instructions}\ntolerance_percent = {GATE.tolerance_percent}\n"
    )
    monkeypatch.setenv("LIMA_HOME", str(lima_home))
    monkeypatch.setattr(bench.paths, "REPO", lease)
    monkeypatch.setattr(bench.paths, "RUST", lease / "rust")
    monkeypatch.setattr(bench, "GATE_FILE", gate)
    monkeypatch.setattr(bench.system, "WINDOWS", False)
    monkeypatch.setattr(bench.platform, "machine", lambda: "arm64")
    monkeypatch.setattr(bench.shutil, "which", lambda tool: tool)
    monkeypatch.setattr(bench.vm_host, "status", lambda _vm: "Running")
    monkeypatch.setattr(bench, "vm_ensure", lambda: 0)
    original_run: Callable[..., subprocess.CompletedProcess[Any]] = subprocess.run

    def count(cmd: list[str], **kwargs: Any) -> subprocess.CompletedProcess[Any]:
        if cmd[0] != "limactl":
            return original_run(cmd, **kwargs)
        assert cmd[:5] == ["limactl", "shell", "--workdir", "/", bench.VM]
        script = cmd[-1]
        assert (
            script.index("flock -s 8")
            < script.index("tar -xf -")
            < script.index("export CARGO_BUILD_JOBS=1")
            < script.index("cargo bench")
        )
        with tarfile.open(fileobj=kwargs["stdin"], mode="r:") as archive:
            assert set(archive.getnames()) == {
                "rust-toolchain.toml",
                "rust/Cargo.lock",
                "rust/added file.rs",
            }
            edited = archive.extractfile("rust-toolchain.toml")
            assert edited is not None and b"beta" in edited.read()
        return subprocess.CompletedProcess(
            cmd, 0, summary(GATE.benchmark.rsplit("::", 1)[0], GATE.instructions)
        )

    monkeypatch.setattr(subprocess, "run", count)
    measure = tmp_path / "measure.json"
    assert bench.count_gate(argparse.Namespace(family=None, measure=str(measure))) == 0
    assert json.loads(measure.read_text())["measured"] == GATE.instructions


def test_a_vm_that_stopped_mid_count_cannot_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    status = {"now": "Stopped"}
    monkeypatch.setattr(bench.vm_host, "status", lambda vm: status["now"])
    assert bench.vm_host.vm_failed(1, "vm") == bench.CANNOT_RUN
    status["now"] = "Running"
    # ssh's own failure says nothing about the count.
    assert bench.vm_host.vm_failed(255, "vm") == bench.CANNOT_RUN
    # A running VM whose script failed reports the script's code: a real failure.
    assert bench.vm_host.vm_failed(1, "vm") == 1
