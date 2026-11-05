"""`by`: Beryl's command line. `by --help` lists the commands; the justfile wraps them."""

from __future__ import annotations

import argparse
import sys

from . import app, bench, cards, checks, edit, oracles, warm


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="by", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for module in (app, bench, cards, checks, edit, oracles, warm):
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
