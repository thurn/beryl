"""Opt-in Cargo timings, saved outside the checkout for efficiency ingestion.

Each JSON line is one crate in one invocation: `run`, UTC `time`, `host`, `mode`
(check, clippy, test-build), `crate`, summed unit `seconds`, and `fresh` (all
reported artifacts reused). Durations come from stable Cargo's HTML UNIT_DATA,
never from command wall time, which includes CPU-token and build-lock waits.
"""

from __future__ import annotations

import json
import re
import socket
import subprocess
import sys
import uuid
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

from . import paths


def crates(html: str, messages: str) -> dict[str, tuple[float, bool]]:
    """Sum build units by package; fresh artifacts absent from UNIT_DATA cost zero."""
    match = re.search(r"const UNIT_DATA\s*=\s*", html)
    if match is None:
        raise ValueError("Cargo timing report has no UNIT_DATA")
    units, _ = json.JSONDecoder().raw_decode(html[match.end() :])
    seconds: dict[str, float] = defaultdict(float)
    fresh: dict[str, bool] = {}
    for line in messages.splitlines():
        message = json.loads(line)
        if message.get("reason") != "compiler-artifact":
            continue
        package = str(message["package_id"])
        # Cargo's path IDs end in /package#version or #package@version;
        # registry IDs use #package@version (older versions used name version).
        tail = package.rsplit("#", 1)[-1]
        name = (
            tail.split("@", 1)[0]
            if "@" in tail
            else package.split("#", 1)[0].rsplit("/", 1)[-1]
        )
        fresh[name] = fresh.get(name, True) and bool(message["fresh"])
    for unit in units:
        name = str(unit["name"])
        seconds[name] += float(unit["duration"])
        # Recent Cargo versions include placeholder units for cache hits with
        # duration zero. The artifact's freshness is authoritative for those.
        if float(unit["duration"]) > 0:
            fresh[name] = False
        else:
            fresh.setdefault(name, False)
    return {name: (seconds[name], reused) for name, reused in fresh.items()}


def run(
    command: list[str], mode: str, env: dict[str, str] | None = None
) -> subprocess.CompletedProcess[str]:
    """Run Cargo with stable HTML timings and JSON artifacts; return its build messages.

    Read the timestamped report named by Cargo, not cargo-timing.html, which a
    concurrent invocation could replace. Failed builds are not baseline samples.
    Timing-format failures warn without changing the build's result.
    """
    began = datetime.now(timezone.utc).isoformat(timespec="milliseconds")
    # Quiet Cargo suppresses the report path even when it writes the HTML.
    command = [a for a in command if a not in ("-q", "--quiet")]
    at = command.index("--") if "--" in command else len(command)
    result = subprocess.run(
        [
            *command[:at],
            "--timings",
            "--message-format=json-render-diagnostics",
            *command[at:],
        ],
        cwd=paths.RUST,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        errors="replace",
        check=False,
    )
    print(result.stderr, end="", file=sys.stderr, flush=True)
    if result.returncode == 0:
        try:
            match = re.search(r"Timing report saved to (.+?\.html)", result.stderr)
            if match is None:
                raise ValueError("Cargo did not name a timing report")
            report = Path(match[1].strip('`"'))
            costs = crates(report.read_text(), result.stdout)
            folder = paths.cache_root() / "ci" / "compile-timings"
            folder.mkdir(parents=True, exist_ok=True)
            run_id = uuid.uuid4().hex
            common = {
                "run": run_id,
                "time": began,
                "host": socket.gethostname().split(".")[0].lower(),
                "mode": mode,
            }
            # One file per invocation avoids interleaving writes from concurrent lanes.
            with (folder / f"{run_id}.jsonl").open("w") as handle:
                for name, (seconds, fresh) in sorted(costs.items()):
                    handle.write(
                        json.dumps(
                            {
                                **common,
                                "crate": name,
                                "seconds": seconds,
                                "fresh": fresh,
                            }
                        )
                        + "\n"
                    )
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"compile-timings: {error}", file=sys.stderr, flush=True)
    return result
