"""The `Command` that `just client` runs: one entry of a module's `COMMANDS` tuple."""

from __future__ import annotations

import argparse
from collections.abc import Callable
from dataclasses import dataclass


@dataclass(frozen=True)
class Command:
    name: str
    # The one line `just client` lists it with.
    summary: str
    # Two command lines, each with what it does, for `--help`; none for a passthrough command,
    # whose program prints its own.
    examples: tuple[tuple[str, str], ...]
    run: Callable[[argparse.Namespace], int]
    # Adds the command's own arguments to its parser.
    arguments: Callable[[argparse.ArgumentParser], None] = lambda _parser: None
    # Whether the command's arguments, `--help` among them, go unread to a program that parses
    # them and prints its own first line (the `client` program's text commands).
    passthrough: bool = False
