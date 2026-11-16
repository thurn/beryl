"""The `beryl-status` command, which `just status` runs:

- `serve` (the default): the site on http://127.0.0.1:8766, kept current;
- `build`: export the beads and build the site once;
- `publish`: build, fill changelogs for a few minutes, and push the site to GitHub Pages;
- `install` / `uninstall`: the launchd agent that publishes when the beads change, at most
  every ten minutes."""

from __future__ import annotations

import argparse
import fcntl
import sys
from collections.abc import Sequence

from . import agent, paths, publish, serve
from .site import Site

# How long one `publish` spends filling changelogs before it pushes, so a run finishes well
# inside the agent's ten-minute interval.
PUBLISH_FILL_SECONDS = 240.0


def build_command(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(prog="just status build")
    parser.add_argument(
        "--fill",
        type=float,
        default=0.0,
        help="seconds to spend fetching changelogs that are not current",
    )
    args = parser.parse_args(argv)
    site = Site(paths.main_checkout(), paths.site())
    site.refresh(force=True)
    if args.fill:
        site.fill(budget=float(args.fill))
    print(f"status: {site.folder}")
    return 0


def publish_command(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(prog="just status publish")
    parser.add_argument("--fill", type=float, default=PUBLISH_FILL_SECONDS)
    args = parser.parse_args(argv)
    # One publish at a time: the agent's run and a manual one share the index file.
    with open(paths.folder() / "publish.lock", "w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("status: another publish is running")
            return 0
        main = paths.main_checkout()
        site = Site(main, paths.site())
        site.refresh(force=True)
        if args.fill:
            site.fill(budget=float(args.fill))
        print(publish.publish(main, site.folder), flush=True)
        paths.prune_sites(site.folder)
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    command = args.pop(0) if args and not args[0].startswith("-") else "serve"
    if command == "serve":
        return serve.serve(args)
    if command == "build":
        return build_command(args)
    if command == "publish":
        return publish_command(args)
    if command == "install":
        return agent.install()
    if command == "uninstall":
        return agent.uninstall()
    print(
        "usage: just status [serve|build|publish|install|uninstall] [options]",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
