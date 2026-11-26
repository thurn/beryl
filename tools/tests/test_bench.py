import argparse
import base64
import json
import subprocess
import tarfile
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import bench, calibration, count_gate

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


def run_count(
    home: Path, bash: str, name: str, checkout: str, live: list[str]
) -> list[str]:
    """Runs the VM script on this machine with stub `flock` and `cargo` that log their
    arguments, returning the cargo calls."""
    (home / "cargo.log").write_text("")
    script = bench.count_script("", ["cargo", "bench"], name, checkout, live)
    stub = """\
export HOME="$PWD"
flock() { return 0; }
cargo() { echo "$*" >>"$HOME/cargo.log"; }
"""
    subprocess.run([bash, "-c", stub + script], cwd=home, check=True)
    return (home / "cargo.log").read_text().splitlines()


def test_target_dirs_rebuild_for_a_new_checkout_and_go_with_their_worktree(
    tmp_path: Path, bash_executable: str
) -> None:
    gate = bench.GATE_NAME
    (tmp_path / "targets" / gate / "release").mkdir(parents=True)
    (tmp_path / "targets" / "gone").mkdir()
    (tmp_path / "gungraun" / "gone").mkdir(parents=True)

    calls = run_count(tmp_path, bash_executable, gate, "/repo/a", ["a"])
    assert calls[0].startswith("clean ") and "--profile bench" in calls[0]
    assert run_count(tmp_path, bash_executable, gate, "/repo/a", ["a"]) == ["bench"]
    assert run_count(tmp_path, bash_executable, gate, "/repo/b", ["a", "b"])[
        0
    ].startswith("clean ")
    assert not (tmp_path / "targets" / "gone").exists()
    assert not (tmp_path / "gungraun" / "gone").exists()

    # A worktree's first run copies the gate's warm dir, then rebuilds its own code.
    assert run_count(tmp_path, bash_executable, "a", "/repo/a", ["a"])[0].startswith(
        "clean "
    )
    assert (tmp_path / "targets" / "a" / "release").is_dir()
    assert run_count(tmp_path, bash_executable, "a", "/repo/a", ["a"]) == ["bench"]
    assert sorted(p.name for p in (tmp_path / "targets").iterdir()) == ["a", gate]


def test_a_new_owner_cleans_the_profile_it_builds() -> None:
    def clean(cargo: list[str]) -> str:
        script = bench.count_script("", cargo, "by-1", "/repo", [])
        return next(line for line in script.splitlines() if "cargo clean" in line)

    assert "--profile bench " in clean(["cargo", "bench", "--", "--profile", "x"])
    assert "--profile search " in clean(["cargo", "bench", "--profile", "search"])
    assert "--profile dev " in clean(["cargo", "bench", "--profile=dev"])


GATE = count_gate.Gate("rollout_counts::cube_rollout::rollouts::corpus", 1_000_000, 1.0)


def test_the_local_gate_uses_only_its_calibration(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    gate = tmp_path / count_gate.GATE_FILE
    gate.parent.mkdir(parents=True)
    profile = """
fixture = "wu-control/v1"
benchmark = "rollouts_counts::group::rollouts::corpus"
tolerance_percent = 1.0
[family.blank]
ratio = 1.25
[profile.windows]
revision = "ffffffffffffffffffffffffffffffffffffffff"
fixture = "wu-control/v1"
runtime = { backend = "wsl", rustc = "1" }
limits = { instructions = 100, family = { blank = 1.20 } }
[profile.mac]
revision = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
fixture = "wu-control/v1"
runtime = { backend = "lima", rustc = "1" }
limits = { instructions = 110, family = { blank = 1.25 } }
"""
    gate.write_text(profile)
    digests: set[str] = set()
    for verifier, backend, bar, ratio in [
        ("windows", "wsl", 100, 1.20),
        ("mac", "lima", 110, 1.25),
    ]:
        monkeypatch.setenv("IGNITE_MACHINE", verifier)
        monkeypatch.setattr(
            count_gate,
            "observe_runtime",
            lambda root: {"backend": backend, "rustc": "1"},
        )
        setup = count_gate.load_gate(tmp_path)
        assert setup.rollout.instructions == bar
        assert setup.families["blank"].ratio == ratio
        assert count_gate.judge(setup.rollout, 110)[0] == (verifier == "mac")
        assert count_gate.judge_family(
            setup.families["blank"], 122, 100, count_gate.Pairing(10, 0)
        )[0] == (verifier == "mac")
        digests.add(setup.profile.digest)
    assert len(digests) == 2

    monkeypatch.setenv("IGNITE_MACHINE", "another-mac")
    with pytest.raises(
        calibration.SetupError, match="no calibration for verifier another-mac"
    ):
        count_gate.load_gate(tmp_path)
    monkeypatch.setenv("IGNITE_MACHINE", "windows")
    with pytest.raises(
        calibration.SetupError, match="incompatible calibration.*backend"
    ):
        count_gate.load_gate(tmp_path)
    monkeypatch.setenv("IGNITE_MACHINE", "mac")
    monkeypatch.setattr(
        count_gate, "observe_runtime", lambda root: {"backend": "lima", "rustc": "2"}
    )
    with pytest.raises(calibration.SetupError, match="incompatible calibration.*rustc"):
        count_gate.load_gate(tmp_path)
    monkeypatch.setenv("IGNITE_MACHINE", "windows")
    monkeypatch.setattr(
        count_gate, "observe_runtime", lambda root: {"backend": "wsl", "rustc": "1"}
    )
    gate.write_text(profile.replace("blank = 1.20", "blank = 1.30"))
    with pytest.raises(
        calibration.SetupError, match="blank ratio must be .* at most 1.25"
    ):
        count_gate.load_gate(tmp_path)


def test_the_gate_counts_only_its_benchmark_as_json() -> None:
    cmd = count_gate.gate_cargo(GATE)
    assert cmd[cmd.index("--bench") + 1] == "rollout_counts"
    assert cmd[cmd.index("-p") + 1] == "beryl_bench"
    harness = cmd[cmd.index("--") + 1 :]
    assert harness == ["--output-format=json", GATE.benchmark]


def test_the_dandan_benchmark_is_counted_after_the_gated_one_with_no_bar() -> None:
    (cmd,) = count_gate.tracked_cargo()
    assert cmd[cmd.index("--bench") + 1] == "dandan_counts"
    assert cmd[cmd.index("--") + 1 :] == [
        "--output-format=json",
        "dandan_counts::dandan_rollout::rollouts::corpus",
    ]
    gate_file = (bench.paths.REPO / count_gate.GATE_FILE).read_text()
    assert "dandan" not in gate_file.lower()


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
    assert count_gate.counted(out, GATE.benchmark) == 1_004_000
    assert count_gate.counted("nothing here\n", GATE.benchmark) is None


def test_a_rise_past_the_tolerance_fails_and_a_drop_asks_for_a_lower_bar() -> None:
    assert count_gate.judge(GATE, 1_010_000)[0]  # exactly at the tolerance
    ok, message = count_gate.judge(GATE, 1_010_001)
    assert not ok and "count-gate.toml" in message
    ok, message = count_gate.judge(GATE, 980_000)
    assert ok and "lower" in message
    ok, message = count_gate.judge(GATE, 995_000)
    assert ok and "lower" not in message


def test_the_gate_file_gives_each_family_a_bar() -> None:
    bars = count_gate.load_families(
        (bench.paths.REPO / count_gate.GATE_FILE).read_text()
    )
    assert [b.name for b in bars][:2] == ["blank", "attachments"]
    assert all(1.0 <= b.ratio <= 2.0 for b in bars)


def test_count_jobs_uses_grant_on_windows_and_caps_lima(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(bench.system, "WINDOWS", True)
    monkeypatch.setenv("IGNITE_CORES", "6")
    assert bench.count_jobs() == 6
    monkeypatch.delenv("IGNITE_CORES", raising=False)
    assert bench.count_jobs() == 4
    monkeypatch.setattr(bench.system, "WINDOWS", False)
    monkeypatch.setenv("IGNITE_CORES", "6")
    assert bench.count_jobs() == 2
    monkeypatch.setenv("IGNITE_CORES", "1")
    assert bench.count_jobs() == 1


def test_parallel_gate_body_builds_once_and_runs_both_sides_in_parallel() -> None:
    body = count_gate.parallel_gate_body(GATE, ["blank", "two-faced"], jobs=6)
    assert (
        "--bench rollout_counts --bench dandan_counts --bench family_counts "
        "--bench family_steps --no-run" in body
    )
    assert 'launch dandan "$dandan_bin" --bench --output-format=json' in body
    assert "dandan_counts::dandan_rollout::rollouts::corpus" in body
    assert "count_jobs=6" in body
    assert "wait -n || failed=1" in body
    assert "rollout_counts::cube_rollout::rollouts::corpus" in body
    assert "family_counts::present_cost::present::blank" in body
    assert "family_counts::present_cost::control::blank" in body
    assert "family_counts::present_cost::present::two_faced" in body
    assert "family_counts::present_cost::control::two_faced" in body
    assert '"$steps_bin" --bench blank two-faced' in body


def test_streamed_runs_share_owner_across_worktrees(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    owners: list[str] = []
    for name in ("landing-1", "landing-2"):
        tree = tmp_path / name
        (tree / "engine").mkdir(parents=True)
        (tree / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "stable"\n')
        (tree / "engine" / "Cargo.lock").write_text(
            '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n'
        )
        monkeypatch.setattr(bench.paths, "REPO", tree)
        monkeypatch.setattr(bench.paths, "ENGINE", tree / "engine")
        monkeypatch.setattr(
            bench.paths, "worktree_names", lambda: ["landing-1", "landing-2"]
        )
        monkeypatch.setattr(bench.system, "WINDOWS", True)

        def capture(
            cmd: list[str], *_a: object, **_kw: object
        ) -> subprocess.CompletedProcess[str]:
            script = base64.b64decode(cmd[-1].split("echo ")[1].split()[0]).decode()
            line = next(
                t for t in script.splitlines() if bench.VM_OWNER in t and "printf" in t
            )
            owners.append(line)
            return subprocess.CompletedProcess[str](cmd, 0, "", "")

        monkeypatch.setattr(bench.vm_host, "run_with_sources", capture)
        assert bench.run_count(["cargo", "bench"], bench.GATE_NAME)[0] == 0
    assert len(owners) == 2 and owners[0] == owners[1]
    assert "streamed" in owners[0] and "landing-1" not in owners[0]


def test_a_family_counts_both_sides_then_prints_its_pairing() -> None:
    counts, steps = count_gate.family_cargo(["attachments"])
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
    present = count_gate.counted(
        out, count_gate.family_benchmark("present", "attachments")
    )
    control = count_gate.counted(
        out, count_gate.family_benchmark("control", "attachments")
    )
    assert (present, control) == (1_100, 1_000)
    assert count_gate.pairing(out, "attachments") == count_gate.Pairing(90, 10)
    assert count_gate.pairing(out, "blank") is None


def test_a_family_fails_over_its_ratio_or_with_a_tenth_skipped() -> None:
    bar = count_gate.FamilyBar("attachments", 1.25)
    assert count_gate.judge_family(bar, 1_250, 1_000, count_gate.Pairing(90, 10))[0]
    ok, message = count_gate.judge_family(bar, 1_251, 1_000, count_gate.Pairing(90, 10))
    assert not ok and "over the bar" in message
    ok, message = count_gate.judge_family(bar, 1_000, 1_000, count_gate.Pairing(89, 11))
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
    tmp_path: Path, bash_executable: str
) -> None:
    # An unclean VM stop once left the gate's last outputs starting with zero bytes, and
    # Gungraun failed to parse them on every later run.
    gate = tmp_path / "gungraun" / bench.GATE_NAME / "old.out.old"
    mine = tmp_path / "gungraun" / "a" / "old.out.old"
    for out in (gate, mine):
        out.parent.mkdir(parents=True)
        out.write_bytes(b"\0" * 8)
    run_count(tmp_path, bash_executable, bench.GATE_NAME, "/repo", ["a"])
    run_count(tmp_path, bash_executable, "a", "/repo/a", ["a"])
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


@pytest.mark.parametrize("windows", [False, True])
def test_the_gate_counts_the_current_files_from_the_guest_filesystem(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, windows: bool
) -> None:
    main, lease = tmp_path / "main", tmp_path / "trees" / "lease"
    (main / "engine").mkdir(parents=True)
    (main / ".gitignore").write_text("/engine/target/\n")
    (main / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "stable"\n')
    (main / "engine" / "Cargo.lock").write_text(
        '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n'
    )
    (main / "engine" / "deleted.rs").touch()
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
    (lease / "engine" / "deleted.rs").unlink()
    (lease / "engine" / "added file.rs").write_text("lease sources")
    (lease / "engine" / "target").mkdir()
    (lease / "engine" / "target" / "artifact").touch()
    (lease / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "beta"\n')

    lima_home = tmp_path / "lima"
    (lima_home / bench.VM).mkdir(parents=True)
    (lima_home / bench.VM / "lima.yaml").write_text(
        LIMA_YAML.replace("/Users/me/beryl", str(main))
    )
    monkeypatch.setenv("LIMA_HOME", str(lima_home))
    monkeypatch.setattr(bench.paths, "REPO", lease)
    monkeypatch.setattr(bench.paths, "ENGINE", lease / "engine")
    monkeypatch.setattr(bench.system, "WINDOWS", windows)
    monkeypatch.setenv("IGNITE_CORES", "4")
    profile = calibration.Profile("test-machine", "a" * 40, "fixture", {}, {})
    monkeypatch.setattr(
        count_gate, "load_gate", lambda _: count_gate.CalibratedGate(profile, GATE, {})
    )
    original_run: Callable[..., subprocess.CompletedProcess[Any]] = subprocess.run

    def count(cmd: list[str], **kwargs: Any) -> subprocess.CompletedProcess[Any]:
        if cmd[0] not in {"limactl", "wsl"}:
            return original_run(cmd, **kwargs)
        if windows:
            assert kwargs["cwd"] is None
            assert cmd[:7] == ["wsl", "-d", bench.VM, "--cd", "/", "--exec", "bash"]
            script = base64.b64decode(cmd[-1].split("echo ")[1].split()[0]).decode()
        else:
            assert kwargs["cwd"] == "/"
            assert cmd[:5] == ["limactl", "shell", "--workdir", "/", bench.VM]
            script = cmd[-1]
        jobs = 4 if windows else 1
        assert (
            script.index("flock -s 8")
            < script.index("tar -xf -")
            < script.index('cd "$source/engine"')
            < script.index(f"export CARGO_BUILD_JOBS={jobs}")
            < script.index("cargo bench")
        )
        with tarfile.open(fileobj=kwargs["stdin"], mode="r:") as archive:
            assert set(archive.getnames()) == {
                "rust-toolchain.toml",
                "engine/Cargo.lock",
                "engine/added file.rs",
            }
            edited = archive.extractfile("rust-toolchain.toml")
            assert edited is not None and b"beta" in edited.read()
        return subprocess.CompletedProcess(
            cmd, 0, summary(GATE.benchmark.rsplit("::", 1)[0], GATE.instructions)
        )

    monkeypatch.setattr(subprocess, "run", count)
    measure = tmp_path / "measure.json"
    assert (
        count_gate.count_gate(
            argparse.Namespace(
                family=None, subgame=False, measure=str(measure), environment=False
            )
        )
        == 0
    )
    assert json.loads(measure.read_text())["measured"] == GATE.instructions


@pytest.mark.parametrize("status", [None, "Stopped"])
def test_lima_starts_outside_the_worktree(
    status: str | None, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(bench.system, "WINDOWS", False)
    monkeypatch.setattr(bench, "output", lambda *_cmd: "/usr/bin/limactl")
    monkeypatch.setattr(bench.paths, "main_checkout", lambda: bench.paths.REPO)
    monkeypatch.setattr(bench.vm_host, "join", lambda _vm: None)
    monkeypatch.setattr(bench.vm_host, "status", lambda _vm: status)
    monkeypatch.setattr(bench.vm_host, "prioritize", lambda _vm: None)
    monkeypatch.setattr(bench.vm_host, "size", lambda _vm: (4, 4))
    starts: list[list[str]] = []

    def start(
        cmd: list[str], *, cwd: str, check: bool
    ) -> subprocess.CompletedProcess[str]:
        assert cwd == "/"
        starts.append(cmd)
        return subprocess.CompletedProcess(cmd, 0)

    monkeypatch.setattr(subprocess, "run", start)
    assert bench.vm_ensure() == 0
    assert [cmd[:2] for cmd in starts] == [["limactl", "start"]]


def test_a_vm_that_stopped_mid_count_cannot_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    status = {"now": "Stopped"}
    monkeypatch.setattr(bench.vm_host, "status", lambda vm: status["now"])
    assert bench.vm_host.vm_failed(1, "vm") == count_gate.CANNOT_RUN
    status["now"] = "Running"
    # ssh's own failure says nothing about the count.
    assert bench.vm_host.vm_failed(255, "vm") == count_gate.CANNOT_RUN
    # A running VM whose script failed reports the script's code: a real failure.
    assert bench.vm_host.vm_failed(1, "vm") == 1


def bench_args(*args: str) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    bench.register(parser.add_subparsers())
    parsed, extra = parser.parse_known_args(["bench", *args])
    parsed.extra = extra
    return parsed


def test_bench_counts_alias_passes_cargo_flags_to_the_count_runner() -> None:
    args = bench_args("--counts", "--bench", "example_counts")
    assert args.func is bench.bench_count
    assert args.extra == ["--bench", "example_counts"]


def test_wall_clock_baselines_follow_the_machine_across_checkouts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    monkeypatch.delenv("CRITERION_HOME", raising=False)
    monkeypatch.setattr(bench, "probe", plugged_in)
    monkeypatch.setattr(bench.platform, "system", lambda: "Windows")
    monkeypatch.setattr(bench.platform, "machine", lambda: "AMD64")
    monkeypatch.setattr(bench.platform, "node", lambda: "Cygnus")
    root = tmp_path / "cache" / "bench" / "windows-amd64-cygnus"
    criterion_homes: list[tuple[Path, str]] = []

    def cargo(
        cmd: list[str], *, cwd: Path, env: dict[str, str], check: bool
    ) -> subprocess.CompletedProcess[str]:
        criterion_homes.append((cwd, env["CRITERION_HOME"]))
        if "BERYL_ABBA_REPORT" in env:
            duration = 50 if cwd.name == "before" else 40
            Path(env["BERYL_ABBA_REPORT"]).write_text(
                f"60\t{duration}\t0.8\t0.79\t0.81\ttest subject\n",
                encoding="utf-8",
            )
        return subprocess.CompletedProcess(cmd, 0)

    monkeypatch.setattr(subprocess, "run", cargo)
    monkeypatch.setattr(bench.paths, "ENGINE", tmp_path / "before")
    assert bench.bench(bench_args("--save-abba-baseline", "before")) == 0
    assert (root / "abba" / "baseline-before.tsv").is_file()
    monkeypatch.setattr(bench.paths, "ENGINE", tmp_path / "after")
    assert bench.bench(bench_args("--abba-baseline", "before")) == 0
    report = capsys.readouterr().err
    assert "in baseline 'before': A 60 ns, B 50 ns" in report
    assert "now: A 60 ns, B 40 ns" in report
    assert {cwd.name for cwd, _ in criterion_homes} == {"before", "after"}
    assert {home for _, home in criterion_homes} == {str(root / "criterion")}
    monkeypatch.setattr(bench.platform, "node", lambda: "Other")
    assert bench.bench(bench_args("--abba-baseline", "before")) == 2
