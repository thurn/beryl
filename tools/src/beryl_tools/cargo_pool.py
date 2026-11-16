"""Every cargo command on the machine, run inside a lease of its CPU tokens (tokens.py).

Only the check steps and `just test-rust` took tokens. Every other cargo command started its
own build as wide as the machine: the ones `by` runs, such as `just b3d`, `just card check`
and `just specs`, and the ones sessions type, about 1,600 `cargo check`, `test`, `build` and
`clippy` commands in the last 300 Claude transcripts on 2026-11-15. On that day the pool's log
held 8 leases taken by agents against 224 taken by pushes and slow CI, so the pool saw almost
none of the work agents start.

`tools/shims/cargo` stands in for cargo on the PATH of every `by` command (`install`) and of
every Claude session (`.claude/settings.json` adds it at session start). It runs the real cargo
directly when its parent already holds a lease (`tokens.LEASE_ENV`) or the pool is off, and
otherwise starts this module, which runs cargo inside a lease (`run`): an `agent` lease, or
one of the check's class when a check step that holds no tokens starts it (`tokens.named_class`). Commands that
compile nothing, such as `cargo fmt` and `cargo metadata`, take no tokens (`NO_LEASE`).

A lease's size is fixed when it starts, and cargo reads `-j` once, so a build that started
while the machine was busy kept its small share after the machine went quiet. So cargo here
takes its job slots from a jobserver that belongs to this lease alone (`Jobserver`): a FIFO
holding one byte per token after the first, which cargo reads through `CARGO_MAKEFLAGS`. Every
`GROW_SECONDS` the lease takes the tokens that have come free (`tokens.Pool.top_up`) and writes
a byte for each, and cargo starts that many more jobs at once. tokens.py avoids a jobserver
shared across the machine, because a byte read by a process that is then killed is lost for
good; this FIFO belongs to one cargo command and is removed with it, and the tokens behind its
bytes are files that the pool takes back whenever this process exits. Cargo ignores the
jobserver when `CARGO_BUILD_JOBS` is set, so this lease's environment leaves that out.

The jobserver also lets a build shrink. A command that only compiles (`SHRINKS`) marks its
tokens after the first as elastic, and while a lease of a higher class waits for tokens
(`tokens.Pool.higher_waiting`) it reads back the bytes no compiler job holds and returns a token
for each (`give_back`). Cargo then runs fewer jobs, and a push that starts during an agent's long
build gets the cores within seconds instead of after the build. `cargo test`, `nextest` and `run`
are left out, because the tests or the program they run take as many threads as the lease had
when it started, which the jobserver does not control.
"""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

from . import paths, tokens

# The directory holding the `cargo` shim, put first on PATH by `install`.
SHIM_DIR: Path = paths.REPO / "tools" / "shims"

# Set by the shim to the real cargo, the first one on PATH after the shim's own directory.
REAL_ENV = "BERYL_REAL_CARGO"

# Set by `install` to the Python that runs `by`, so that the shim starts this module without
# going through uv.
PYTHON_ENV = "BERYL_SHIM_PYTHON"

# Cargo commands that compile nothing, and their usual flags, which run at once with no lease.
NO_LEASE: frozenset[str] = frozenset(
    {
        "fmt",
        "metadata",
        "locate-project",
        "pkgid",
        "tree",
        "clean",
        "version",
        "help",
        "read-manifest",
        "search",
        "--version",
        "-V",
        "--help",
        "-h",
        "--list",
    }
)

# How often a running command takes the tokens that have come free, or, while it gives tokens back
# to a higher class, looks for job slots that have come free.
GROW_SECONDS = 2.0
SHRINK_SECONDS = 0.25

# Cargo commands that only compile, whose parallelism the jobserver controls throughout, so they
# can give tokens back while they run.
SHRINKS: frozenset[str] = frozenset({"build", "check", "clippy", "rustc", "doc"})


def install() -> None:
    """Puts the shim first on this process's PATH, so that every cargo command `by` starts,
    directly or through another tool, runs inside a lease. Windows has neither the shell
    script nor FIFOs, so cargo there runs as before."""
    if os.name == "nt":
        return
    shims = str(SHIM_DIR)
    parts = os.environ.get("PATH", "").split(os.pathsep)
    if parts[:1] != [shims]:
        os.environ["PATH"] = os.pathsep.join([shims, *(p for p in parts if p != shims)])
    os.environ[PYTHON_ENV] = sys.executable


def subcommand(argv: list[str]) -> str:
    """Cargo's subcommand: the first argument that is not a flag or a `+toolchain`, or the
    first flag when there is none, as in `cargo --version`."""
    for arg in argv:
        if not arg.startswith(("-", "+")):
            return arg
    return argv[0] if argv else ""


def needs_lease(argv: list[str]) -> bool:
    return subcommand(argv) not in NO_LEASE


class Jobserver:
    """A FIFO in the GNU make jobserver protocol that one cargo command takes its job slots
    from: one byte per token after the first, which every process holds without a byte.
    """

    def __init__(self, size: int) -> None:
        self.dir: str = tempfile.mkdtemp(prefix="beryl-jobserver-")
        self.path: str = os.path.join(self.dir, "fifo")
        os.mkfifo(self.path)
        # Opened for reading and writing, so that writes never block for want of a reader.
        self.fd: int = os.open(self.path, os.O_RDWR | os.O_NONBLOCK)
        self.grow(size - 1)

    def grow(self, n: int) -> None:
        if n > 0:
            os.write(self.fd, b"+" * n)

    def take(self, n: int) -> int:
        """Reads back up to `n` job slots that no job holds now, and how many it got."""
        if n < 1:
            return 0
        try:
            return len(os.read(self.fd, n))
        except BlockingIOError:
            return 0

    def env(self) -> dict[str, str]:
        return {"CARGO_MAKEFLAGS": f"-j --jobserver-auth=fifo:{self.path}"}

    def close(self) -> None:
        os.close(self.fd)
        shutil.rmtree(self.dir, ignore_errors=True)


def lease_env(lease: tokens.Lease, jobserver: Jobserver) -> dict[str, str]:
    """Cargo's environment inside the lease: the lease's variables, which also tell a cargo
    started under this one to run inside it, without `CARGO_BUILD_JOBS`, which would make cargo
    ignore the jobserver."""
    env = {**os.environ, **lease.env(), **jobserver.env()}
    env.pop("CARGO_BUILD_JOBS", None)
    return env


def is_shim(path: str) -> bool:
    """Whether `path` is a Beryl checkout's cargo shim rather than cargo itself."""
    return Path(path).parent.parts[-2:] == ("tools", "shims")


def run(argv: list[str], pool: tokens.Pool | None = None) -> int:
    """Runs the real cargo with `argv` inside a lease (of `tokens.named_class`) that grows while it runs."""
    real = os.environ.get(REAL_ENV) or "cargo"
    if is_shim(real):
        # Running another checkout's shim as cargo would start this one again, without end.
        print(f"cargo shim: {real} is a shim, not cargo", file=sys.stderr)
        return 127
    command = [real, *argv]
    if (
        not needs_lease(argv)
        or os.environ.get(tokens.LEASE_ENV, "").isdigit()
        or os.environ.get(tokens.POOL_ENV) == "0"
    ):
        return subprocess.run(command, check=False).returncode
    pool = pool or tokens.Pool()
    want = tokens.largest(pool.size)
    shrinks = subcommand(argv) in SHRINKS
    lease = pool.acquire(
        tokens.named_class(),
        want,
        command=f"cargo {subcommand(argv)}",
        elastic_from=1 if shrinks else None,
    )
    assert lease is not None
    extra: list[tokens.Lease] = []
    jobserver = Jobserver(lease.size)
    try:
        child: subprocess.Popen[bytes] = subprocess.Popen(
            command, env=lease_env(lease, jobserver)
        )

        def forward(sig: int, _frame: object) -> None:
            child.send_signal(sig)

        for sig in (signal.SIGTERM, signal.SIGHUP):
            signal.signal(sig, forward)
        # Ctrl-C reaches cargo through the terminal; this process waits for it to stop.
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        held = lease.size
        code: int | None = None
        yielding = False
        while code is None:
            try:
                code = child.wait(timeout=SHRINK_SECONDS if yielding else GROW_SECONDS)
            except subprocess.TimeoutExpired:
                pass
            if code is not None:
                break
            yielding = shrinks and held > 1 and pool.higher_waiting(lease.klass)
            if yielding:
                held -= give_back(pool, [lease, *extra], jobserver, held - 1)
            elif held < want:
                more = pool.top_up(lease.klass, held, want, elastic=shrinks)
                if more.size:
                    extra.append(more)
                    jobserver.grow(more.size)
                    held += more.size
                    # The pool's log records the most tokens the command held.
                    lease.size = max(lease.size, held)
        return code
    finally:
        jobserver.close()
        for more in extra:
            more.release()
        lease.release()


def give_back(
    pool: tokens.Pool, leases: list[tokens.Lease], jobserver: Jobserver, n: int
) -> int:
    """Returns to the pool up to `n` of the leases' elastic tokens whose job slots no compiler
    job holds now, newest lease first, and how many. A slot read back from the FIFO and not
    matched by a token (none left to return) is written back."""
    slots = jobserver.take(n)
    returned = 0
    for lease in reversed(leases):
        if returned < slots:
            returned += pool.give_back(lease, slots - returned)
    jobserver.grow(slots - returned)
    return returned


def main() -> int:
    return run(sys.argv[1:])


if __name__ == "__main__":
    sys.exit(main())
