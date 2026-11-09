"""`by`: Beryl's command line. `by --help` lists the commands; the justfile wraps them."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys

from . import (
    android,
    app,
    audio,
    bench,
    bugs,
    cards,
    checks,
    edit,
    mobile,
    oracles,
    shots,
    specs,
    system,
    warm,
)


def utf8_rerun(argv: list[str] | None) -> int | None:
    """On Windows, outside Python's UTF-8 mode, runs `by` again in it and returns its exit
    code: there text files are read and written in the ANSI code page by default, which garbles
    card names. Its children inherit the mode. None when already in it."""
    if not system.WINDOWS or sys.flags.utf8_mode:
        return None
    os.environ["PYTHONUTF8"] = "1"
    args = sys.argv[1:] if argv is None else argv
    proc = subprocess.Popen(
        [sys.executable, "-X", "utf8", "-m", "beryl_tools.cli", *args]
    )
    while True:
        try:
            return proc.wait()
        except KeyboardInterrupt:
            continue  # The console sent Ctrl-C to the child too: it answers it.


def main(argv: list[str] | None = None) -> int:
    rerun = utf8_rerun(argv)
    if rerun is not None:
        return rerun
    parser = argparse.ArgumentParser(prog="by", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for module in (
        android,
        app,
        audio,
        bench,
        bugs,
        cards,
        checks,
        edit,
        mobile,
        oracles,
        shots,
        specs,
        system,
        warm,
    ):
        module.register(sub)
    # Commands marked `passthrough` hand arguments they don't know to another tool.
    args, extra = parser.parse_known_args(argv)
    if extra and not getattr(args, "passthrough", False):
        parser.error(f"unrecognized arguments: {' '.join(extra)}")
    args.extra = extra
    try:
        return int(args.func(args))
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
