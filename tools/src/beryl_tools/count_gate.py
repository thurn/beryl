"""Judge rollout and paired present-cost instruction counts on the registered verifier."""

from __future__ import annotations

import argparse
import json
import math
import platform
import shlex
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

from . import bench, calibration, paths, system, vm_host
from .deps import CANNOT_RUN

# The instruction-count gate. Its bar, and when to move it, are in
# GATE_FILE's comments.

GATE_FILE = Path("rust/crates/beryl_bench/count-gate.toml")


@dataclass
class Gate:
    benchmark: str  # FILE::GROUP::FUNCTION::ID, as Gungraun filters and reports it
    instructions: int
    tolerance_percent: float


@dataclass(frozen=True)
class CalibratedGate:
    profile: calibration.Profile
    rollout: Gate
    families: dict[str, FamilyBar]


def load_gate(root: Path) -> CalibratedGate:
    """Load the configured limits after observing the local measurement environment."""
    try:
        text = (root / GATE_FILE).read_text(encoding="utf-8")
        data = tomllib.loads(text)
        profile = calibration.select(
            data.get("profile", {}), str(data["fixture"]), lambda: observe_runtime(root)
        )
        instructions = profile.limits.get("instructions")
        if type(instructions) is not int or instructions <= 0:
            raise ValueError("expected positive rollout instructions")
        ratios = profile.limits.get("family")
        ceilings = {bar.name: bar.ratio for bar in load_families(text)}
        if not isinstance(ratios, dict) or ratios.keys() != ceilings.keys():
            raise ValueError("expected a ratio for each present-cost family")
        families = {}
        for name, ceiling in ceilings.items():
            ratio = float(ratios[name])
            if not math.isfinite(ratio) or not 0 < ratio <= ceiling:
                raise ValueError(f"{name} ratio must be positive and at most {ceiling}")
            families[name] = FamilyBar(name, ratio)
        tolerance = float(data["tolerance_percent"])
        if not math.isfinite(tolerance) or tolerance < 0:
            raise ValueError("expected a nonnegative rollout tolerance")
        rollout = Gate(str(data["benchmark"]), instructions, tolerance)
        return CalibratedGate(profile, rollout, families)
    except calibration.SetupError:
        raise
    except (AttributeError, OSError, ValueError, KeyError, TypeError) as error:
        raise calibration.SetupError(f"invalid {GATE_FILE.name}: {error}") from error


def gate_cargo(gate: Gate) -> list[str]:
    """Builds and counts only the gated benchmark, printing Gungraun's JSON summary."""
    file = gate.benchmark.split("::")[0]
    return bench.cargo_bench(
        ["--bench", file, "--", "--output-format=json", gate.benchmark]
    )


def counted(stdout: str, benchmark: str) -> int | None:
    """The benchmark's instructions (Callgrind's `Ir`) from Gungraun's JSON lines."""
    for line in stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            summary = json.loads(line)
        except json.JSONDecodeError:
            continue
        if f"{summary.get('module_path')}::{summary.get('id')}" != benchmark:
            continue
        for profile in summary.get("profiles", []):
            if profile.get("tool") == "Callgrind":
                return int(profile["data"]["total"]["metrics"]["Ir"]["values"]["new"])
    return None


def judge(gate: Gate, measured: int) -> tuple[bool, str]:
    """Fails a rise past the tolerance; a drop past it passes, asking for a lower bar."""
    change = 100.0 * (measured - gate.instructions) / gate.instructions
    said = f"{measured:,} instructions, {change:+.2f}% against the bar of {gate.instructions:,}"
    if change > gate.tolerance_percent:
        return False, (
            f"{said}: over the {gate.tolerance_percent}% tolerance. Make the new cost free "
            f"for these decks, or raise the bar only as {GATE_FILE.name} allows"
        )
    if change < -gate.tolerance_percent:
        return (
            True,
            f"{said}: lower `instructions` in {GATE_FILE.name} to keep the gain",
        )
    return True, said


# The present-cost gate (docs/benchmarking.md, "The present-cost gate"): each family's paired
# rollouts, counted on both sides, and their pairing, run natively in the same build.
FAMILY_BENCH = "family_counts"
FAMILY_GROUP = "present_cost"
FAMILY_STEPS = "family_steps"
# Past this share of steps left out of both sums, the ratio describes too few of them.
SKIPPED_LIMIT = 0.1


@dataclass
class FamilyBar:
    name: str
    ratio: float  # the most the present side may count over the control's


@dataclass
class Pairing:
    paired: int
    skipped: int


def load_families(text: str) -> list[FamilyBar]:
    """The families with a bar, `[family.<name>] ratio = 1.25`, in the file's order."""
    data = tomllib.loads(text)
    families = data.get("family", {})
    return [FamilyBar(name, float(bar["ratio"])) for name, bar in families.items()]


def family_benchmark(side: str, name: str) -> str:
    """The family's benchmark on one side, `present` or `control`, as Gungraun names it.
    A benchmark id is a Rust identifier, so a family's hyphens become underscores."""
    return f"{FAMILY_BENCH}::{FAMILY_GROUP}::{side}::{name.replace('-', '_')}"


def family_cargo(names: list[str]) -> list[list[str]]:
    """Counts both sides of each family as Gungraun JSON, then prints their pairings."""
    counts = [
        bench.cargo_bench(
            ["--bench", FAMILY_BENCH, "--", "--output-format=json"]
            + [family_benchmark("*", name)]
        )
        for name in names
    ]
    return [*counts, bench.cargo_bench(["--bench", FAMILY_STEPS, "--", *names])]


def pairing(stdout: str, name: str) -> Pairing | None:
    """The family's steps paired and skipped, from `family_steps`' JSON line."""
    for line in stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            found = json.loads(line)
        except json.JSONDecodeError:
            continue
        if found.get("family") == name:
            return Pairing(int(found["paired"]), int(found["skipped"]))
    return None


def judge_family(
    bar: FamilyBar, present: int, control: int, steps: Pairing
) -> tuple[bool, str]:
    """Fails a ratio over the bar, or more than a tenth of the steps left out."""
    ratio = present / control
    total = steps.paired + steps.skipped
    share = steps.skipped / total if total else 1.0
    said = (
        f"family {bar.name}: {present:,} instructions with the family, {control:,} on the "
        f"controls, ratio {ratio:.3f} against the bar of {bar.ratio}; {steps.paired:,} steps "
        f"paired, {steps.skipped:,} skipped ({100 * share:.1f}%)"
    )
    if share > SKIPPED_LIMIT:
        return False, (
            f"{said}: over a tenth of the steps skipped, so the ratio says little. Make the "
            f"control offer the choices the family's positions do"
        )
    if ratio > bar.ratio:
        return (
            False,
            f"{said}: over the bar. Make the mechanism cheaper while it is in play",
        )
    return True, said


def vm_ready() -> None:
    """Start the existing guest, or report why this verifier cannot measure."""
    error = calibration.SetupError
    machine = platform.machine().lower()
    if system.WINDOWS and machine not in ("amd64", "x86_64"):
        raise error("the Windows count gate needs an x86-64 WSL distribution")
    if not system.WINDOWS and machine not in ("arm64", "aarch64"):
        raise error("the Lima count gate needs an ARM64 host")
    backend = "wsl" if system.WINDOWS else "limactl"
    if shutil.which(backend) is None:
        raise error(
            f"{backend} is missing; install it, then run `just bench-count` once"
        )
    if vm_host.status(bench.VM) is None:
        raise error(f"no {bench.VM} VM; run `just bench-count` once to create it")
    if bench.vm_ensure() != 0:
        raise error(f"could not start the {bench.VM} VM")


def observe_runtime(root: Path) -> dict[str, str]:
    """Read the tool versions inside the guest that actually runs the counts."""
    vm_ready()
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"][
        "channel"
    ]
    runner = bench.locked_version((root / "rust/Cargo.lock").read_text(), "gungraun")
    script = f"""set -e
. /etc/os-release
. "$HOME/.cargo/env"
printf 'guest=%s %s\\n' "$ID" "$VERSION_ID"
printf 'arch=%s\\n' "$(uname -m)"
printf 'valgrind=%s\\n' "$(valgrind --version)"
printf 'libc=%s\\n' "$(getconf GNU_LIBC_VERSION)"
printf 'cc=%s\\n' "$(cc -dumpfullversion)"
printf 'rustc=%s\\n' "$(rustc +{shlex.quote(channel)} --version)"
printf 'gungraun=%s\\n' "$(gungraun-runner --version)"
"""
    probe = subprocess.run(
        bench.vm_command(script, workdir="/"),
        cwd=vm_host.shell_host_cwd(),
        capture_output=True,
        text=True,
        check=False,
    )
    keys = ("guest", "arch", "valgrind", "libc", "cc", "rustc", "gungraun")
    observed: dict[str, str] = {}
    for line in probe.stdout.splitlines():
        key, separator, value = line.partition("=")
        if separator:
            observed[key] = value
    if probe.returncode or not all(observed.get(key) for key in keys):
        raise calibration.SetupError(
            f"could not identify tools in {bench.VM}; run `just bench-count` once"
        )
    if runner is None or runner not in observed["gungraun"].split():
        raise calibration.SetupError(
            f"{bench.VM} needs Gungraun {runner}; run `just bench-count` once"
        )
    return {
        "backend": "wsl" if system.WINDOWS else "lima",
        **{key: observed[key] for key in keys},
    }


def family_gate(names: list[str], bars: dict[str, FamilyBar]) -> int:
    """Counts each family named and judges it against its bar: 0 if all pass."""
    unknown = [name for name in names if name not in bars]
    if unknown:
        known = ", ".join(bars) or "none"
        bench.log(
            f"no bar in {GATE_FILE.name} for {', '.join(unknown)}; families: {known}"
        )
        return 2
    first, *rest = family_cargo(names)
    code, out = bench.run_count(first, bench.GATE_NAME, then=rest)
    if code != 0:
        return vm_host.vm_failed(code, bench.VM)
    failed = False
    for name in names:
        present = counted(out, family_benchmark("present", name))
        control = counted(out, family_benchmark("control", name))
        found = pairing(out, name)
        if present is None or control is None or found is None:
            bench.log(
                f"Gungraun or {FAMILY_STEPS} reported nothing for the family {name}"
            )
            return 2
        ok, message = judge_family(bars[name], present, control, found)
        bench.log(message)
        # One line per family that the Ignite adapter reads (`VERDICT` in ignite/rest.py).
        bench.log(f"verdict {name}: {'pass' if ok else 'fail'}")
        failed |= not ok
    return 1 if failed else 0


def count_gate(args: argparse.Namespace) -> int:
    try:
        if args.environment:
            print(
                json.dumps(
                    {
                        "verifier": calibration.verifier(),
                        "runtime": observe_runtime(paths.REPO),
                    }
                )
            )
            return 0
        setup = load_gate(paths.REPO)
    except (calibration.SetupError, OSError) as error:
        bench.log(str(error))
        return CANNOT_RUN
    bench.log(
        f"verifier {setup.profile.verifier}, calibration at {setup.profile.revision[:12]}"
    )
    families: list[str] | None = args.family
    if families:
        return family_gate(families, setup.families)
    gate = setup.rollout
    code, out = bench.run_count(gate_cargo(gate), bench.GATE_NAME)
    if code != 0:
        return vm_host.vm_failed(code, bench.VM)
    measured = counted(out, gate.benchmark)
    if measured is None:
        bench.log(f"Gungraun reported no count for {gate.benchmark}")
        return 2
    ok, message = judge(gate, measured)
    bench.log(f"{gate.benchmark}: {message}")
    if args.measure:
        # Ignite's `counts` family judges the trend itself and files its tickets.
        Path(args.measure).write_text(
            json.dumps(
                {
                    "measured": measured,
                    "bar": gate.instructions,
                    "ok": ok,
                    "message": message,
                    "verifier": setup.profile.verifier,
                    "calibration": setup.profile.digest,
                }
            )
            + "\n"
        )
    return 0 if ok else 1


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "count-gate",
        help="fail when the gated benchmark's instruction count rises past its bar",
    )
    p.add_argument(
        "--family",
        action="append",
        help="gate this family's present cost instead (`[family.<name>]` in count-gate.toml);"
        " repeat for several",
    )
    p.add_argument(
        "--measure",
        metavar="FILE",
        help="write the count, the bar and the verdict to FILE as JSON and leave the trend's"
        " tickets to the caller",
    )
    p.add_argument(
        "--environment",
        action="store_true",
        help="print the verifier ID and guest runtime for calibration",
    )
    p.set_defaults(func=count_gate)


def main(argv: list[str]) -> int:
    """`python -m beryl_tools.count_gate count-gate ...`: the count gate without `cli.py`, so that
    Ignite's `counts` family loads only this module and the ones it declares as inputs.
    """
    parser = argparse.ArgumentParser(prog="beryl_tools.count_gate")
    register(parser.add_subparsers(dest="command", required=True))
    args = parser.parse_args(argv)
    if args.command != "count-gate":
        parser.error("only count-gate runs this way; run the others as `by <command>`")
    return int(count_gate(args))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
