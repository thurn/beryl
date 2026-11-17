"""Stable Cargo reports: rebuilt units and cached artifact placeholders."""

import json
import subprocess
from pathlib import Path

import pytest

from beryl_tools import checks, compile_timings


def artifact(name: str, fresh: bool) -> str:
    return json.dumps(
        {
            "reason": "compiler-artifact",
            "package_id": f"path+file:///w/{name}#0.1.0",
            "fresh": fresh,
        }
    )


def test_crate_totals_include_targets_but_do_not_count_cache_hits() -> None:
    html = (
        "const UNIT_DATA = "
        + json.dumps(
            [
                {"name": "built", "duration": 2},
                {"name": "built", "duration": 3},
                {"name": "cached", "duration": 0},
            ]
        )
        + ";"
    )
    messages = "\n".join(
        [artifact("built", False), artifact("cached", True), artifact("absent", True)]
    )
    assert compile_timings.crates(html, messages) == {
        "built": (5, False),
        "cached": (0, True),
        "absent": (0, True),
    }
    with pytest.raises(ValueError):
        compile_timings.crates("new unsupported format", messages)


def test_capture_uses_named_report_and_preserves_clippy_flags(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    report: Path = tmp_path / "cargo-timing-unique.html"
    report.write_text('const UNIT_DATA = [{"name":"built","duration":2}];')
    monkeypatch.setattr(compile_timings.paths, "cache_root", lambda: tmp_path)
    commands: list[list[str]] = []

    def execute(
        command: list[str], **kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        commands.append(command)
        return subprocess.CompletedProcess(
            command, 0, artifact("built", False), f"Timing report saved to {report}\n"
        )

    monkeypatch.setattr(compile_timings.subprocess, "run", execute)
    assert (
        compile_timings.run(
            ["cargo", "clippy", "-q", "--", "-D", "warnings"], "clippy"
        ).returncode
        == 0
    )
    assert commands[0][-3:] == ["--", "-D", "warnings"]
    assert "-q" not in commands[0]
    assert "--timings" in commands[0][: commands[0].index("--")]
    captured = list((tmp_path / "ci" / "compile-timings").glob("*.jsonl"))
    assert len(captured) == 1
    record = json.loads(captured[0].read_text())
    assert (record["crate"], record["mode"], record["seconds"], record["fresh"]) == (
        "built",
        "clippy",
        2,
        False,
    )


def test_slow_lane_collects_timings_and_fast_lane_does_not() -> None:
    for fast in (True, False):
        lane = checks.rust_lane(fast=fast)
        for step in lane.steps:
            if step.name in ("clippy", "tests"):
                assert ("--timings" in step.cmd) is not fast
