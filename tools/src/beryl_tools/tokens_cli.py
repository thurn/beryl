"""The `by tokens` and `by tokens-run` commands over the machine's CPU tokens (tokens.py): who
holds and waits for them, and a command run inside a lease of them."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

from . import system, tokens


def status(_args: argparse.Namespace) -> int:
    pool = tokens.Pool()
    with system.file_lock(pool.root / "lock"):
        held = pool.holders()
        waiting = pool.waiters()
    print(f"{len(held)} of {pool.size} tokens held")
    for klass in tokens.CLASSES:
        pids: dict[int, int] = {}
        for h in held.values():
            if h.klass == klass:
                pids[h.pid] = pids.get(h.pid, 0) + 1
        queued = sum(1 for _, w in waiting if w.klass == klass)
        if pids or queued:
            leases = ", ".join(f"pid {p}: {n}" for p, n in sorted(pids.items()))
            print(f"  {klass}: {leases or 'none held'}; {queued} waiting")
    return 0


def run(args: argparse.Namespace) -> int:
    """Runs a command inside a lease of `--class` tokens, at most `--cores`."""
    command = args.extra[1:] if args.extra[:1] == ["--"] else args.extra
    if not command:
        print("tokens run: no command", file=sys.stderr)
        return 2
    lease = tokens.acquire(
        args.klass,
        args.cores or tokens.largest(tokens.pool_size()),
        command=Path(command[0]).name,
    )
    assert lease is not None
    with lease:
        env = {**os.environ, **lease.env()}
        return subprocess.run(
            [system.which(command[0]), *command[1:]], env=env, check=False
        ).returncode


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser("tokens", help="the machine's CPU tokens: who holds and waits")
    p.set_defaults(func=status)
    r = sub.add_parser(
        "tokens-run",
        help="run a command inside a lease of CPU tokens (--class C, --cores N, -- COMMAND...)",
    )
    r.add_argument("--class", dest="klass", choices=tokens.CLASSES, default="agent")
    r.add_argument("--cores", type=int, default=0, help="the most tokens to take")
    r.set_defaults(func=run, passthrough=True)
