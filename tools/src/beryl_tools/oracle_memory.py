"""What the oracles made of each scenario, remembered by content, so that a run plays only the
scenarios that are new to XMage and Forge.

A scenario's outcome in the oracles depends on two things: its dump entry (the position, the
script and the final snapshot Beryl played to) and the oracles' side (their pinned commits, the
exporters and harnesses, and the card store the exporters read). `Memory.key` hashes both, so
an entry whose key passed before passes again. Every `by oracle` run records the outcome of each
scenario it played in `$BERYL_CACHE/oracles/outcomes.txt`.

This matters most for the random scenarios filed under the card "Generated" (generate.rs).
Their scripts play through the rules engine, so a rules change rewrites some of them without
touching any card file. The fast check's oracle lane used to play only the scenarios of changed
card files, and slow CI played the generated ones only when the exporters changed or the commit's
sample drew them. A rules change that broke one therefore landed, and the failure surfaced in a
later slow-CI run, where it was blamed on an unrelated batch and needed a Steward bead. Now:

- The fast check (`by oracle --new`) plays the named cards' scenarios that have not passed
  before, and the generated scenarios that no run has played. It leaves out a generated scenario
  that failed elsewhere: that failure is already on staging, and slow CI owns it, so it does not
  fail every session's push.
- Slow CI (`by oracle --select`) plays the commit's sample whole, the changed cards' scenarios
  that have not passed before, and every other scenario that has not passed under its key.
- A scenario listed in `data/oracle-exporter-gaps.json` or `data/oracle-divergences.json` plays
  only as part of its card: its entry goes stale when it passes, and a run's stale check sees
  only what it played. The nightly and `by oracle` with no flags play everything.
"""

from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import cast

from . import paths, system

# The card the dump files the random scenarios under (`beryl_cards::generate::CARD`).
GENERATED = "Generated"

# Why a scenario outside the chosen cards played, as the run log's `chosen` says (runlog.py).
NEW = "new to the oracles"

# The files on the oracles' side whose contents decide an outcome besides the dump entry: the
# exporters, the Java harnesses and the card store. The dump's own code is left out, because
# whatever it changes shows in the entries. The allowlists only classify a failure, and the
# scenarios they list are never skipped.
SIDE_GLOBS: tuple[str, ...] = (
    "tools/src/beryl_tools/xmage*.py",
    "tools/src/beryl_tools/forge.py",
    "tools/src/beryl_tools/card_facts.py",
    "tools/oracles/**/*",
    "data/card-store.tsv",
)

PASS = "pass"
FAIL = "fail"

# The package whose scenarios the oracles play: a change it depends on can rewrite them.
SCENARIO_PACKAGE = "beryl_cards"


def reached(
    changed: Sequence[str], packages: list[dict[str, object]] | None = None
) -> bool:
    """Whether `changed` can change a dumped scenario: a card file, the card store, or a crate
    that `SCENARIO_PACKAGE` depends on (rust_tests.affected_packages, given the workspace's
    `packages` or reading them)."""
    from . import card_files, rust_tests  # They import much of the tools.

    if card_files.cards_in(changed):
        return True
    if not any(p.startswith("rust/") or p == "data/card-store.tsv" for p in changed):
        return False
    affected = rust_tests.affected_packages(changed, packages)
    return affected is None or SCENARIO_PACKAGE in affected


def side(pins: Sequence[str], repo: Path = paths.REPO) -> str:
    """A hash of the oracles' side: their `pins` and the contents of the `SIDE_GLOBS` files."""
    digest = hashlib.sha256("\0".join(pins).encode())
    files = sorted({p for g in SIDE_GLOBS for p in repo.glob(g) if p.is_file()})
    for path in files:
        digest.update(path.relative_to(repo).as_posix().encode() + b"\0")
        digest.update(path.read_bytes())
    return digest.hexdigest()


def label(entry: Mapping[str, object]) -> str:
    """How the allowlists and the oracles' results name a scenario: "card / scenario"."""
    return f"{entry['card']} / {entry['name']}"


def listed(repo: Path = paths.REPO) -> set[str]:
    """The scenarios the exporter-gap list or the divergence allowlist names."""
    found: set[str] = set()
    for name, field in (
        ("oracle-exporter-gaps.json", "scenario"),
        ("oracle-divergences.json", "scenario"),
    ):
        entries = cast(
            list[dict[str, str]], json.loads((repo / "data" / name).read_text())
        )
        found |= {f"{e['card']} / {e[field]}" for e in entries}
    return found


def store() -> Path:
    path = paths.cache_root() / "oracles"
    path.mkdir(parents=True, exist_ok=True)
    return path / "outcomes.txt"


class Memory:
    """The outcomes recorded under one oracles' side, with the scenarios always played whole."""

    def __init__(
        self, side_hash: str, outcomes: dict[str, str], always: set[str]
    ) -> None:
        self.side = side_hash
        self.outcomes = outcomes
        self.always = always

    @staticmethod
    def load(pins: Sequence[str], path: Path | None = None) -> Memory:
        """The outcomes in `path` (the cache's by default); the last line for a key wins."""
        path = store() if path is None else path
        outcomes: dict[str, str] = {}
        if path.is_file():
            for line in path.read_text().splitlines():
                key, _, outcome = line.partition(" ")
                if outcome:
                    outcomes[key] = outcome
        return Memory(side(pins), outcomes, listed())

    def key(self, entry: Mapping[str, object]) -> str:
        text = json.dumps(entry, sort_keys=True)
        return hashlib.sha256(f"{self.side}\0{text}".encode()).hexdigest()

    def seen(self, entry: Mapping[str, object]) -> str | None:
        return self.outcomes.get(self.key(entry))

    def select(
        self,
        entries: list[dict[str, object]],
        chosen: dict[str, str] | None,
        named: Sequence[str],
        new: bool,
        sampled: str,
    ) -> list[dict[str, object]]:
        """The entries to play. With neither `chosen` (slow CI's cards and why, which gains
        `NEW` for each card added) nor `new` (the fast check), those of the `named` cards, or
        every entry. Otherwise: the cards chosen as `sampled` whole, the other chosen or named
        cards' scenarios that have not passed, and then, outside them, in slow CI every
        scenario that has not passed, and in the fast check each generated scenario no run has
        played. A listed scenario plays only with its card."""
        if chosen is None and not new:
            return [e for e in entries if not named or e["card"] in named]
        # A copy: a card `chosen` gains as NEW still counts as outside the chosen ones.
        cards = dict(chosen) if chosen is not None else dict.fromkeys(named, "")
        kept: list[dict[str, object]] = []
        for e in entries:
            card = cast(str, e["card"])
            why = cards.get(card)
            seen = self.seen(e)
            if why == sampled:
                kept.append(e)
            elif why is not None:
                if seen != PASS or label(e) in self.always:
                    kept.append(e)
            elif label(e) in self.always:
                continue
            elif chosen is not None and seen != PASS:
                chosen.setdefault(card, NEW)
                kept.append(e)
            elif chosen is None and card == GENERATED and seen is None:
                kept.append(e)
        return kept

    def record(
        self,
        entries: Sequence[Mapping[str, object]],
        results: Mapping[str, Mapping[str, object]],
        path: Path | None = None,
    ) -> None:
        """Appends the outcome of each of `entries` in `results` (each oracle's report, before
        the exporter gaps are applied): a pass when every oracle passed it or could not export
        it, else a failure."""
        statuses: dict[str, list[dict[str, object]]] = {}
        for result in results.values():
            for s in cast(list[dict[str, object]], result["scenarios"]):
                statuses.setdefault(label(s), []).append(s)
        lines = []
        for e in entries:
            found = statuses.get(label(e))
            if not found:
                continue
            ok = all(
                s["status"] in (PASS, "unsupported") and not s.get("stale_allowlist")
                for s in found
            )
            key = self.key(e)
            outcome = PASS if ok else FAIL
            self.outcomes[key] = outcome
            lines.append(f"{key} {outcome}\n")
        if not lines:
            return
        path = store() if path is None else path
        with system.file_lock(path.with_suffix(".lock")):
            with path.open("a") as out:
                out.write("".join(lines))
