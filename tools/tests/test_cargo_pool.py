from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

import pytest

from beryl_tools import cargo_pool, tokens
from beryl_tools.tokens import Pool

pytestmark: pytest.MarkDecorator = pytest.mark.skipif(
    os.name == "nt", reason="the shim is a POSIX script"
)

# A stand-in for cargo that records what it was started with: its arguments, the lease's
# variables, and, once `release` exists, the job slots its jobserver FIFO holds.
FAKE_CARGO = """#!{python}
import json, os, sys, time
out = {out!r}
seen = {{
    "argv": sys.argv[1:],
    "tokens": os.environ.get("BERYL_CPU_TOKENS"),
    "jobs": os.environ.get("CARGO_BUILD_JOBS"),
    "makeflags": os.environ.get("CARGO_MAKEFLAGS"),
}}
release = {release!r}
if release:
    while not os.path.exists(release):
        time.sleep(0.01)
    time.sleep(0.3)
flags = seen["makeflags"]
if flags:
    fifo = flags.split("fifo:")[1]
    fd = os.open(fifo, os.O_RDONLY | os.O_NONBLOCK)
    try:
        seen["slots"] = len(os.read(fd, 1024))
    except BlockingIOError:
        seen["slots"] = 0
    os.close(fd)
with open(out, "w") as handle:
    json.dump(seen, handle)
"""


@pytest.fixture(autouse=True)
def outside_lease(monkeypatch: pytest.MonkeyPatch) -> None:
    """The tests may themselves run under a check step's lease."""
    for name in (
        tokens.LEASE_ENV,
        tokens.POOL_ENV,
        tokens.CLASS_ENV,
        "CARGO_BUILD_JOBS",
    ):
        monkeypatch.delenv(name, raising=False)


def fake_cargo(directory: Path, release: Path | None = None) -> tuple[Path, Path]:
    """A fake `cargo` in `directory` and the file it writes what it saw to."""
    directory.mkdir(parents=True, exist_ok=True)
    out = directory / "seen.json"
    cargo = directory / "cargo"
    cargo.write_text(
        FAKE_CARGO.format(
            python=sys.executable, out=str(out), release=str(release) if release else ""
        )
    )
    cargo.chmod(0o755)
    return cargo, out


def test_subcommand_skips_flags_and_toolchains() -> None:
    assert (
        cargo_pool.subcommand(["+nightly", "--locked", "build", "-p", "x"]) == "build"
    )
    assert cargo_pool.subcommand(["--version"]) == "--version"
    assert not cargo_pool.needs_lease(["fmt", "--all"])
    assert not cargo_pool.needs_lease(["metadata", "--no-deps"])
    assert cargo_pool.needs_lease(["nextest", "list"])
    assert cargo_pool.needs_lease(["check", "-q"])


def test_install_puts_the_shim_first_once(monkeypatch: pytest.MonkeyPatch) -> None:
    shims = str(cargo_pool.SHIM_DIR)
    monkeypatch.setenv("PATH", os.pathsep.join(["/usr/bin", shims, "/bin"]))
    cargo_pool.install()
    cargo_pool.install()
    assert os.environ["PATH"].split(os.pathsep) == [shims, "/usr/bin", "/bin"]
    assert os.environ[cargo_pool.PYTHON_ENV] == sys.executable


def test_a_build_runs_on_its_own_jobserver_and_returns_its_tokens(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    cargo, out = fake_cargo(tmp_path / "bin")
    monkeypatch.setenv(cargo_pool.REAL_ENV, str(cargo))
    monkeypatch.setenv("CARGO_BUILD_JOBS", "18")
    pool = Pool(tmp_path / "pool", 9)
    assert cargo_pool.run(["build", "-p", "x"], pool) == 0
    seen = json.loads(out.read_text())
    assert seen["argv"] == ["build", "-p", "x"]
    # Two thirds of nine tokens: one held by cargo itself and five as bytes in the FIFO.
    assert seen["tokens"] == "6"
    assert seen["slots"] == 5
    # Cargo ignores the jobserver when it is told how many jobs to run.
    assert seen["jobs"] is None
    assert not pool.holders()
    log = [
        json.loads(line)
        for line in (pool.root / tokens.LEASE_LOG).read_text().splitlines()
    ]
    assert [(line["command"], line["cores"]) for line in log] == [("cargo build", 6)]


def test_a_build_grows_when_tokens_come_free(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    release: Path = tmp_path / "release"
    cargo, out = fake_cargo(tmp_path / "bin", release)
    monkeypatch.setenv(cargo_pool.REAL_ENV, str(cargo))
    monkeypatch.setattr(cargo_pool, "GROW_SECONDS", 0.05)
    pool = Pool(tmp_path / "pool", 9)
    # Another agent holds four of the seven tokens agents may take, which leaves three.
    other: tokens.Lease | None = pool.acquire("agent", 4)
    assert other is not None and other.size == 4

    def finish_other() -> None:
        time.sleep(0.2)
        assert other is not None
        other.release()
        release.touch()

    threading.Thread(target=finish_other).start()
    assert cargo_pool.run(["check"], pool) == 0
    seen = json.loads(out.read_text())
    assert seen["tokens"] == "3"
    # Started with two bytes, then given one for each token that brought it up to six.
    assert seen["slots"] == 5
    assert not pool.holders()


def test_commands_that_compile_nothing_take_no_tokens(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    cargo, out = fake_cargo(tmp_path / "bin")
    monkeypatch.setenv(cargo_pool.REAL_ENV, str(cargo))
    pool = Pool(tmp_path / "pool", 4)
    assert cargo_pool.run(["fmt", "--all"], pool) == 0
    assert json.loads(out.read_text())["tokens"] is None
    assert not (pool.root / tokens.LEASE_LOG).exists()


def test_the_shim_runs_cargo_as_it_is_inside_a_lease(tmp_path: Path) -> None:
    cargo, out = fake_cargo(tmp_path / "bin")
    env = {
        **os.environ,
        "PATH": os.pathsep.join(
            [str(cargo_pool.SHIM_DIR), str(cargo.parent), "/usr/bin", "/bin"]
        ),
        tokens.LEASE_ENV: "3",
        "CARGO_BUILD_JOBS": "3",
    }
    shim = cargo_pool.SHIM_DIR / "cargo"
    subprocess.run([str(shim), "build"], env=env, check=True)
    seen = json.loads(out.read_text())
    assert seen == {"argv": ["build"], "tokens": "3", "jobs": "3", "makeflags": None}


def test_a_check_step_names_the_class_its_cargo_takes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    cargo, _ = fake_cargo(tmp_path / "bin")
    monkeypatch.setenv(cargo_pool.REAL_ENV, str(cargo))
    monkeypatch.setenv(tokens.CLASS_ENV, "push")
    pool = Pool(tmp_path / "pool", 8)
    assert cargo_pool.run(["test"], pool) == 0
    log = [
        json.loads(line)
        for line in (pool.root / tokens.LEASE_LOG).read_text().splitlines()
    ]
    assert [line["class"] for line in log] == ["push"]
