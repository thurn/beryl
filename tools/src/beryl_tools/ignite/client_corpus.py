"""The `client-corpus` exploration adapter, with one resumable check per card crate."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from ..client import corpus
from ..system import exe
from . import Listed, Result, listed, report
from .rust import asked

FAMILY = "client-corpus"


def main(arguments: list[str], debug: Path) -> int:
    match arguments:
        case ["list"]:
            listed(Listed(f"{FAMILY}:{crate}") for crate in corpus.crates())
            return 0
        case ["next"]:
            for part in corpus.parts(corpus.crates()):
                print(
                    json.dumps(
                        {"check": f"{FAMILY}:{part.crate}", "digest": part.digest}
                    ),
                    flush=True,
                )
            return 0
        case ["run", *checks]:
            failed = False
            try:
                parts = corpus.parts(asked(FAMILY, checks))
                for part, sweep in corpus.sweep(
                    parts, exe(debug / "client"), exe(debug / "record")
                ):
                    message = sweep.message()
                    report(
                        [
                            Result(
                                f"{FAMILY}:{part.crate}",
                                "fail" if message else "pass",
                                message,
                            )
                        ]
                    )
                    failed |= bool(message)
            except (OSError, ValueError, subprocess.CalledProcessError) as error:
                print(f"{FAMILY}: {error}", file=sys.stderr)
                return 1
            return int(failed)
        case _:
            print(f"usage: {FAMILY} list | next | run <check>...", file=sys.stderr)
            return 1
