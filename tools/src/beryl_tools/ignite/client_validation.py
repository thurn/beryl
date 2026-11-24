"""Validate The client's tuning and corpus with the programs Ignite already built."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

from ..client import pack, paths
from ..system import exe
from . import Listed, Result
from .rust import asked

FAMILY = "client-validation"


def checks() -> list[Listed]:
    return [
        Listed(f"{FAMILY}:tuning"),
        Listed(f"{FAMILY}:corpus", pack.CORPUS_INPUTS),
    ]


def tuning_problems(debug: Path) -> list[str]:
    done = subprocess.run(
        [str(exe(debug / "client")), "data"],
        cwd=paths.WORKSPACE,
        env={**os.environ, "BERYL_CLIENT_DATA": str(paths.DATA)},
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if not done.returncode:
        return []
    message = done.stderr.strip() or done.stdout.strip()
    return [message or f"client data: exit {done.returncode}"]


def run(debug: Path, requested: list[str]) -> list[Result]:
    results: list[Result] = []
    for name in asked(FAMILY, requested):
        try:
            if name == "tuning":
                found = tuning_problems(debug)
            elif name == "corpus":
                found = pack.check_corpus(
                    paths.DATA, pack.record(exe(debug / "record"))
                )
            else:
                continue
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            found = [str(error)]
        results.append(
            Result(f"{FAMILY}:{name}", "fail" if found else "pass", "\n".join(found))
        )
    return results
