from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

from beryl_tools import tokens, vm_host

pytestmark: pytest.MarkDecorator = pytest.mark.skipif(
    os.name == "nt", reason="Windows' machines are WSL distributions, left alone"
)


@pytest.fixture(autouse=True)
def cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    for name in (tokens.LEASE_ENV, tokens.POOL_ENV, tokens.CLASS_ENV):
        monkeypatch.delenv(name, raising=False)
    return tmp_path / "cache"


class Limactl:
    """Stands in for `limactl`, `lsof` and `taskpolicy`: which machines run, and what was
    called."""

    def __init__(self, running: set[str]) -> None:
        self.running = running
        self.calls: list[list[str]] = []

    def __call__(
        self, cmd: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        self.calls.append(cmd)
        out = ""
        if cmd[:2] == ["limactl", "list"]:
            out = "\n".join(f"{vm} Running" for vm in sorted(self.running))
        if cmd[:2] == ["limactl", "stop"]:
            self.running.discard(cmd[-1])
        if cmd[0] == "lsof":
            out = "4242\n"
        return subprocess.CompletedProcess(cmd, 0, out, "")


def dead_pid() -> int:
    proc = subprocess.Popen(["true"])
    proc.wait()
    return proc.pid


def test_an_idle_machine_stops_after_the_wait(monkeypatch: pytest.MonkeyPatch) -> None:
    limactl = Limactl({"beryl-bench"})
    monkeypatch.setattr(subprocess, "run", limactl)
    vm_host.folder("beryl-bench")
    # A running machine with no record of when it went idle gets one, and keeps running.
    assert vm_host.stop_idle(now=1_000_000) == []
    idle = vm_host.folder("beryl-bench") / vm_host.IDLE_FILE
    os.utime(idle, (1_000_000, 1_000_000))
    assert vm_host.stop_idle(now=1_000_000 + 60) == []
    assert vm_host.stop_idle(now=1_000_000 + vm_host.IDLE_MINUTES * 60 + 1) == [
        "beryl-bench"
    ]
    assert limactl.running == set()


def test_a_machine_with_a_live_user_keeps_running(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    limactl = Limactl({"beryl-bench"})
    monkeypatch.setattr(subprocess, "run", limactl)
    users = vm_host.folder("beryl-bench") / "users"
    (users / str(os.getpid())).write_text("")
    (vm_host.folder("beryl-bench") / vm_host.IDLE_FILE).touch()
    os.utime(vm_host.folder("beryl-bench") / vm_host.IDLE_FILE, (0, 0))
    assert vm_host.stop_idle(now=10**9) == []
    # A user that has exited no longer counts, and the machine's idle wait starts then.
    (users / str(os.getpid())).unlink()
    (users / str(dead_pid())).write_text("")
    assert vm_host.stop_idle(now=10**9) == []
    assert not list(users.iterdir())
    assert limactl.running == {"beryl-bench"}


def test_joining_takes_tokens_for_the_machines_cpus(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    limactl = Limactl(set())
    monkeypatch.setattr(subprocess, "run", limactl)
    pool = tokens.Pool(tmp_path / "pool", 18)
    monkeypatch.setattr(tokens, "Pool", lambda: pool)
    vm_host.join("beryl3d-bench", 8)
    try:
        assert (vm_host.folder("beryl3d-bench") / "users" / str(os.getpid())).exists()
        assert len(pool.holders()) == 8
    finally:
        for lease in vm_host._LEASES:
            lease.release()
        vm_host._LEASES.clear()


def test_low_priority_work_runs_its_machine_in_the_background(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    limactl = Limactl(set())
    monkeypatch.setattr(subprocess, "run", limactl)
    monkeypatch.setenv("BERYL_LOW_PRIORITY", "1")
    vm_host.prioritize("beryl3d-bench")
    monkeypatch.delenv("BERYL_LOW_PRIORITY")
    vm_host.prioritize("beryl3d-bench")
    policies = [c for c in limactl.calls if c[0] == "taskpolicy"]
    assert policies == [
        ["taskpolicy", "-b", "-p", "4242"],
        ["taskpolicy", "-B", "-p", "4242"],
    ]
