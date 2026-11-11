"""`just citations migrate <date>`: move the tree's rules citations to a new rules release.

A release renumbers some rules and rewords others. Most of a migration is mechanical, and this
does that part: for each citation it finds where the cited rule went in the new release, then
rewrites the number (and a link's anchor) in place, all in one pass so a renumbering chain
(712.11 -> 712.8 while 712.8 -> 712.6) never applies twice. Where a rule went is found:

1. by the quotation after the citation, when it has one: the rule the quotation is still in
   (the cited number when it is still there, else the one new rule that holds every piece, or
   among several that do, the one the old rule's text points to);
2. otherwise by the old rule's text: the cited number when its text is unchanged, else the one
   new rule with the same text, else the one rule of the same section whose text is clearly the
   nearest.

What it cannot place is left as it was and reported with the old and new text, for an agent to
judge: a quotation the new rules no longer hold, a rule split, merged or rewritten. Then it pins
the new release in `data/rules-version.json`, so `just citations` checks against it.
"""

from __future__ import annotations

import argparse
import difflib
from dataclasses import dataclass
from pathlib import Path

from . import citations, paths
from .citations import Citation, Version, anchor_of

# How alike a reworded rule must be to its old text to be taken for it, and by how much it
# must beat the next nearest. A rule that kept its number needs only to be the nearest and
# this alike.
NEAREST_RATIO = 0.8
NEAREST_MARGIN = 0.1
IN_PLACE_RATIO = 0.5


@dataclass(frozen=True)
class Placement:
    """Where a citation's rule went: its new number (None when it cannot be placed), what an
    agent should look at, and whether that is only that the rule was reworded (`reworded`,
    reported once per rule) or a citation to judge."""

    number: str | None
    note: str | None = None
    reworded: bool = False


def number_of(anchor: str) -> str:
    """The rule number of a rules key: `R7126a` is `712.6a`, `R7` is `7`, `R712` is `712`."""
    digits = anchor[1:]
    if len(digits) <= 3 or not digits[:3].isdigit():
        return digits
    return f"{digits[:3]}.{digits[3:]}"


def section(anchor: str) -> str:
    """The three-digit section a rule belongs to."""
    return anchor[1:4]


class Migration:
    """Places citations of the `old` rules in the `new` ones, remembering each answer."""

    def __init__(self, old: dict[str, str], new: dict[str, str]) -> None:
        self.old = old
        self.new = new
        self.by_text: dict[str, list[str]] = {}
        for anchor, text in new.items():
            self.by_text.setdefault(text, []).append(anchor)
        self.placed: dict[tuple[str, str | None], Placement] = {}

    def place(self, number: str, quotation: str | None) -> Placement:
        key = (number, quotation)
        if key not in self.placed:
            self.placed[key] = self.place_uncached(number, quotation)
        return self.placed[key]

    def place_uncached(self, number: str, quotation: str | None) -> Placement:
        anchor = anchor_of(number)
        if quotation is None:
            return self.by_rule(anchor)
        pieces = citations.segments(quotation)
        if not pieces:
            return self.by_rule(anchor)
        if all(piece in self.new.get(anchor, "") for piece in pieces):
            return Placement(number)
        holders = [a for a, text in self.new.items() if all(p in text for p in pieces)]
        if len(holders) == 1:
            return Placement(number_of(holders[0]))
        if len(holders) > 1:
            by_rule = self.by_rule(anchor).number
            if by_rule is not None and anchor_of(by_rule) in holders:
                return Placement(by_rule)
            named = ", ".join(number_of(a) for a in holders[:5])
            return Placement(None, f"the quotation is in several rules: {named}")
        by_rule = self.by_rule(anchor)
        target = by_rule.number or number
        held = all(piece in self.old.get(anchor, "") for piece in pieces)
        problem = (
            "the quotation is no longer in the rules"
            if held
            else "the quotation was not the old rule's text either"
        )
        return Placement(
            by_rule.number,
            f"{problem}; requote from CR {target}: "
            f'"{self.new.get(anchor_of(target), "(no such rule)")}"',
        )

    def by_rule(self, anchor: str) -> Placement:
        """Where the old rule at `anchor` went, by its text."""
        number = number_of(anchor)
        text = self.old.get(anchor)
        if text is None:
            if anchor in self.new:
                # Already cites the new release, as a citation written after it can.
                return Placement(number)
            return Placement(None, "no such rule in either release")
        if self.new.get(anchor) == text:
            return Placement(number)
        same = self.by_text.get(text, [])
        if len(same) == 1:
            return Placement(number_of(same[0]))
        candidates = [a for a in self.new if section(a) == section(anchor)]
        ranked = sorted(
            (
                (difflib.SequenceMatcher(None, text, self.new[a]).ratio(), a)
                for a in candidates
            ),
            reverse=True,
        )
        best_ratio, best = ranked[0] if ranked else (0.0, "")
        runner_up = ranked[1][0] if len(ranked) > 1 else 0.0
        in_place = best == anchor and best_ratio >= IN_PLACE_RATIO
        clear = best_ratio >= NEAREST_RATIO and best_ratio - runner_up >= NEAREST_MARGIN
        if in_place or clear:
            target = number_of(best)
            if best_ratio == 1:
                return Placement(target)
            return Placement(target, f'"{self.new[best]}"', reworded=True)
        return Placement(None, f'cannot place it; it said: "{text}"')


def rewritten(line: str, changes: list[tuple[Citation, str]]) -> str:
    """`line` with each citation's number (and anchor) replaced by its new number."""
    spans: list[tuple[tuple[int, int], str]] = []
    for citation, number in changes:
        spans.append((citation.number_span, number))
        if citation.anchor_span is not None:
            spans.append((citation.anchor_span, anchor_of(number)))
    for (start, end), text in sorted(spans, reverse=True):
        line = line[:start] + text + line[end:]
    return line


def migrate(
    root: Path, old: dict[str, str], new: dict[str, str], write: bool
) -> tuple[list[str], list[str]]:
    """Move the citations under `root` from the `old` rules to the `new`: each renumbering
    (`file:line: CR a -> CR b`) and each note for an agent (`file:line: CR n: note`)."""
    migration = Migration(old, new)
    moved: list[str] = []
    notes: list[str] = []
    reworded: dict[tuple[str, str], tuple[str, int]] = {}
    for rel, path in citations.source_files(root):
        text = path.read_text(encoding="utf-8")
        lines = text.split("\n")
        by_line: dict[int, list[tuple[Citation, str]]] = {}
        for citation in citations.citations_in(lines):
            placement = migration.place(citation.number, citation.quotation)
            where = f"{rel}:{citation.line + 1}: CR {citation.number}"
            target = placement.number
            if placement.reworded and placement.note and target is not None:
                key = (citation.number, target)
                count = reworded.get(key, (placement.note, 0))[1]
                reworded[key] = (placement.note, count + 1)
            elif placement.note:
                notes.append(f"{where}: {placement.note}")
            if target is not None and target != citation.number:
                moved.append(f"{where} -> CR {target}")
                by_line.setdefault(citation.line, []).append((citation, target))
        if by_line and write:
            for index, changes in by_line.items():
                lines[index] = rewritten(lines[index], changes)
            path.write_text("\n".join(lines), encoding="utf-8")
    for (number, target), (text, count) in sorted(reworded.items()):
        moved_to = "" if target == number else f" (now CR {target})"
        notes.append(
            f"CR {number}{moved_to} was reworded; check its {count} citations still hold: "
            f"{text}"
        )
    return moved, notes


def run(args: argparse.Namespace) -> int:
    root = paths.REPO
    current = citations.pinned(root)
    target = Version(args.date, args.url) if args.url else Version.for_date(args.date)
    old = citations.load_rules(current)
    new = citations.load_rules(target)
    moved, notes = migrate(root, old, new, write=not args.dry_run)
    for line in moved:
        print(line)
    if notes:
        print(f"\n{len(notes)} citations to judge by hand:")
        for line in notes:
            print(line)
    if not args.dry_run:
        citations.pin(root, target)
        problems = citations.check(root, new)
        print(
            f"\ncitations: moved {len(moved)} from the {current.effective} rules to the "
            f"{target.effective} rules; `just citations` now reports {len(problems)} problems"
        )
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "migrate", help="move the tree's citations to a new rules release"
    )
    p.add_argument("date", help="the release's effective date, YYYY-MM-DD")
    p.add_argument(
        "--url", help="the release's .txt, when it is not at Wizards' usual URL"
    )
    p.add_argument(
        "--dry-run", action="store_true", help="print the changes without making them"
    )
    p.set_defaults(func=run)
