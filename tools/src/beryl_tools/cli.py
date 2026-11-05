"""`by`: Beryl's command line. `by --help` lists the commands; the justfile wraps them."""

from __future__ import annotations

import argparse
import sys

from . import app, checks, warm


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="by", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for module in (app, checks, warm):
        module.register(sub)
    args = parser.parse_args(argv)
    try:
        return int(args.func(args))
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
