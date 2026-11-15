"""What every query takes (docs/efficiency-dashboard.md, "The command"): a time range from
`--since`, and cohort filters that choose beads by label, epic, host or provider.

The range bounds every part of a result the same way: a window belongs to it when the window
starts inside it, a request or tool call when it happened inside it, and a bead when it was
closed inside it. Stored times are UTC ISO strings, so a bound is one too and compares as a
string.
"""

from __future__ import annotations

import argparse
import re
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone

# `--since` as a count of hours or days before now.
RELATIVE: re.Pattern[str] = re.compile(r"^(\d+)([hd])$")


@dataclass(frozen=True)
class Range:
    """From `since` (inclusive) to `until` (exclusive), each a UTC ISO string or `None` for
    no bound. `name` is how the command was given it, such as `7d`."""

    since: str | None
    until: str | None
    name: str

    def holds(self, moment: str | None) -> bool:
        if moment is None:
            return False
        if self.since is not None and moment < self.since:
            return False
        return self.until is None or moment < self.until

    def before(self) -> Range | None:
        """The range of the same length that ends where this one starts, for trends; `None`
        when this one has no start."""
        if self.since is None:
            return None
        start = parse_time(self.since)
        end = parse_time(self.until) if self.until else now()
        return Range(stamp(start - (end - start)), self.since, f"before {self.name}")


def now() -> datetime:
    return datetime.now(timezone.utc)


def stamp(moment: datetime) -> str:
    return moment.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")


def parse_time(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).replace(
        tzinfo=timezone.utc
    )


def parse_since(value: str, at: datetime | None = None) -> Range:
    """`24h`, `7d`, `all`, or a date (`2026-11-01`, the start of that day in local time)."""
    if value == "all":
        return Range(None, None, value)
    found = RELATIVE.match(value)
    if found:
        count = int(found.group(1))
        step = (
            timedelta(hours=count) if found.group(2) == "h" else timedelta(days=count)
        )
        return Range(stamp((at or now()) - step), None, value)
    try:
        day = datetime.strptime(value, "%Y-%m-%d")
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"--since takes 24h, 7d, all or a date such as 2026-11-01, not {value!r}"
        ) from None
    return Range(stamp(day.astimezone()), None, value)


@dataclass(frozen=True)
class Scope:
    """A time range and cohort filters. `labels` holds labels a bead must all carry: from
    `--where key=value` (the label `key:value`), `--where label` (that label exactly),
    `--role` and `--tier`. `epic` keeps the epic and the beads under it. `host` and
    `provider` keep windows that ran there."""

    range: Range = field(default_factory=lambda: Range(None, None, "all"))
    labels: tuple[str, ...] = ()
    epic: str | None = None
    host: str | None = None
    provider: str | None = None

    def chooses_beads(self) -> bool:
        """Whether a filter chooses beads, so that work on no bead falls outside it."""
        return bool(self.labels) or self.epic is not None

    def describe(self) -> str:
        """The filters as the command takes them, empty when there are none."""
        parts = [f"--where {label}" for label in self.labels]
        for flag, value in (
            ("--epic", self.epic),
            ("--host", self.host),
            ("--provider", self.provider),
        ):
            if value is not None:
                parts.append(f"{flag} {value}")
        return " ".join(parts)

    def flags(self) -> str:
        """`--since` and the filters, for the queries a result names under `more`."""
        return " ".join(f for f in (f"--since {self.range.name}", self.describe()) if f)


def label_of(where: str) -> str:
    key, equals, value = where.partition("=")
    return f"{key}:{value}" if equals else where


def add_arguments(p: argparse.ArgumentParser, default_since: str) -> None:
    p.add_argument(
        "--since",
        default=default_since,
        help=f"24h, 7d, all or a date (default {default_since})",
    )
    p.add_argument(
        "--where",
        action="append",
        default=[],
        metavar="label=value",
        help="beads with the label label:value (or a whole label); repeatable",
    )
    p.add_argument("--epic", help="the epic and the beads under it")
    p.add_argument("--role", help="beads with the label role:<role>")
    p.add_argument("--tier", help="beads with the label tier:<tier>")
    p.add_argument("--host", help="work done on this host")
    p.add_argument("--provider", help="work done with this provider (claude, codex)")


def from_arguments(args: argparse.Namespace) -> Scope:
    labels = [label_of(str(w)) for w in args.where]
    for key in ("role", "tier"):
        value = getattr(args, key)
        if value is not None:
            labels.append(f"{key}:{value}")
    return Scope(
        parse_since(str(args.since)),
        tuple(labels),
        args.epic,
        args.host,
        args.provider,
    )
