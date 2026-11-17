"""The lanes of the checks (checks.py), each a sequence of timed steps, and the constructors of
the lanes the fast check and slow CI choose from."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

from . import card_files, paths, system
from .changes import markdown_files
from .reruns import (
    Failed,
    failed_cards,
    failed_pytests,
    failed_shots,
    failed_titles,
    whole,
)

# The `ui-test` checks slow CI's `ui-smoke` lane plays, by title: the start screen starts a game,
# and on a recorded game the stage passes priority (Space) and plays a land at a click. Each
# takes about a second, runs on the page's own clock (no wall-clock waits) and has not failed in
# over 100 slow-CI runs. Every other browser check runs in the fast check's selection and in the
# browser sweep every few hours (sweep.py).
SMOKE_TITLES: tuple[str, ...] = (
    "Enter on the start screen starts a game",
    "a land pressed while a script plays on to it is played",
)

BY: list[str] = [sys.executable, "-m", "beryl_tools.cli"]


@dataclass
class Step:
    name: str
    cmd: list[str]
    cwd: Path
    # How the step reruns once after failing; None for a step whose failure is no flake.
    failed: Failed | None = None


@dataclass
class Lane:
    name: str
    # Whether it builds, so needs warm caches; Markdown and Python lanes skip the clone.
    warm: bool = False
    steps: list[Step] = field(default_factory=list)
    # What runs the steps the fast check cuts off at its hard limit (deadline.py), for its
    # message: slow CI for every lane it also runs.
    deferred_to: str = "slow CI"

    def step(
        self,
        name: str,
        cmd: list[str],
        cwd: Path = paths.REPO,
        rerun: Failed | None = None,
    ) -> Lane:
        self.steps.append(Step(name, cmd, cwd, rerun))
        return self


# Lanes.


def repo_lane() -> Lane:
    checks = [sys.executable, "-m", "beryl_tools.checks"]
    return (
        Lane("repo")
        .step("just", [*checks, "just"])
        .step(
            "markdown",
            [sys.executable, "-m", "mdformat", "--check", *markdown_files()],
        )
        .step("file-length", [*checks, "file-length"])
        .step("conventions", [*checks, "conventions"])
    )


# Beryl 3D's tools and the tests of them: a change confined to these (and Beryl 3D's own paths)
# runs only these tests in the python lane (`python_lane(only=...)`).
BERYL3D_TOOLS = r"^tools/src/beryl_tools/beryl3d/"
BERYL3D_TOOL_TESTS = r"^tools/tests/test_beryl3d(_[^/]*)?\.py$"


def python_lane(only: list[str] | None = None) -> Lane:
    """The python lane. With `only`, the repository-relative paths of a change confined to
    Beryl 3D's tools, black checks just those files and pytest runs only Beryl 3D's tool tests
    (tools/tests/test_beryl3d*.py); pyre still checks the whole project, as it cannot check
    a part."""
    black = ["tools"]
    tests: list[str] = []
    if only is not None:
        black = [p for p in only if p.endswith(".py") and (paths.REPO / p).exists()]
        tests = sorted(
            f"tests/{f.name}"
            for f in (paths.TOOLS / "tests").glob("test_beryl3d*.py")
            if re.match(BERYL3D_TOOL_TESTS, f"tools/tests/{f.name}")
        )
    lane = Lane("python")
    if black:
        lane.step(
            "black", [sys.executable, "-m", "black", "--check", "--quiet", *black]
        )
    # Pyre has no Windows build: macOS checks the types.
    if not system.WINDOWS:
        lane.step("pyre", ["pyre", "--noninteractive", "check"], paths.TOOLS)
    return lane.step(
        "pytest",
        [sys.executable, "-m", "beryl_tools.checks", "pytest", *tests],
        rerun=failed_pytests,
    )


# The Python tests that read card and vocabulary files, which `scale_lane` runs alone for a Rust
# change (tests/test_scale.py and tests/test_card_names.py read every card file;
# tests/test_card.py reads beryl_card's source as card brief does, and tests/test_specs.py the
# card files' names). Others read files outside tools/ too (test_forge:
# data/card-store.tsv and oracle-divergences.json; test_runlog: git), but never what a card or
# vocabulary change touches.
CARD_READERS = [
    "tests/test_scale.py",
    "tests/test_card_names.py",
    "tests/test_card.py",
    "tests/test_specs.py",
]
CARD_FILES = r"^rust/(cards|crates/beryl_cards?)/"
# The card store is source to both: beryl_card_store embeds it, so a refresh (`just cards`)
# rebuilds every crate that reads a record, and the tools' tests read it and hold it to their
# sample.
CARD_STORE = r"^data/card-store\.tsv$"
# The web interface's stylesheet of design tokens is generated from this file: a change to it
# runs the ui lane, whose `design-tokens` step holds the two to each other.
DESIGN_TOKENS = r"^data/design-tokens\.json$"


def scale_lane() -> Lane:
    """Only the Python tests that read card and vocabulary files: a change to those, which does
    not touch tools/, can break them and slow CI would be the first to say so."""
    return Lane("scale").step(
        "pytest",
        [
            *[sys.executable, "-m", "beryl_tools.checks", "pytest"],
            *["--lane", "scale", *CARD_READERS],
        ],
        rerun=failed_pytests,
    )


def ui_lane() -> Lane:
    return (
        Lane("ui", warm=True)
        # tokens.css is generated from data/design-tokens.json, which prettier leaves alone.
        .step(
            "design-tokens",
            [sys.executable, "-m", "beryl_tools.checks", "design-tokens"],
        )
        .step("prettier", [system.npm_bin("prettier"), "--check", "."], paths.UI)
        .step("tsc", [system.npm_bin("tsc"), "-b"], paths.UI)
        # The UI's logic that runs with no browser (ui/package.json `test`): Node expands the
        # pattern itself.
        .step("test", ["node", "--test", "scripts/*.test.mjs"], paths.UI)
    )


def names_lane(
    cited: list[str] | None = None, fetch: bool = False, crates: bool | None = None
) -> Lane:
    """No card name in generic code, held to the burn-down list (card_names.py), no other
    player of two, no board door and no failed push thrown away in rules code (two_player.py,
    board_door.py, discarded_push.py), no citation of a planning document in code
    (plan_references.py), and every rules citation held to the pinned release
    (citations.py) and card file header to the card store (card_header.py): in the files
    `cited`, or with None in the whole tree, fetching the release's text if `fetch` and it is
    not cached. With `crates` (by default when `cited` is None), the set crates are held to
    what `just card crates` writes (card_crates.py).
    """
    checks = [sys.executable, "-m", "beryl_tools.checks"]
    lane = Lane("names")
    for check in (
        "card-names",
        "two-player",
        "board-door",
        "discarded-push",
        "plan-references",
        "divergences",
    ):
        lane.step(check, [*checks, check])
    if cited != []:
        flags = ["--fetch"] if fetch else []
        lane.step("citations", [*checks, "citations", *flags, *(cited or [])])
        headers = [p for p in cited or [] if card_files.FILE.match(p)]
        if cited is None or headers:
            lane.step("card-headers", [*checks, "card-headers", *headers])
    lane.step("rule-ownership", [*checks, "rule-ownership"])
    if crates or (crates is None and cited is None):
        lane.step("card-crates", [*checks, "card-crates"])
    return lane


def app_lane() -> Lane:
    return Lane("app", warm=True).step(
        "smoke", [*BY, "smoke", "--without-iroh"], rerun=whole
    )


def shots_lane(select: bool = False, affected: str = "") -> Lane:
    """Renders every screen in ui/shots.json headless, or with `select` those the change can
    break as fit the fast check, or with `affected` all of those it can break since that
    commit (selection.py); a page error or a crash fails it."""
    cmd = [*BY, "ui-shots", *choice_flags(select, affected)]
    return Lane("shots", warm=True).step("ui-shots", cmd, rerun=failed_shots)


def ui_test_lane(select: bool = False, affected: str = "") -> Lane:
    """Plays the game screen through fixtures it records apart from `shots`' (shots.py), so
    the two run at once, every check or those the change can break (`shots_lane`); a page
    error or a failed interaction fails it."""
    cmd = [*BY, "ui-test", *choice_flags(select, affected)]
    return Lane("ui-test", warm=True).step("ui-test", cmd, rerun=failed_titles)


def ui_smoke_lane() -> Lane:
    """Plays the few [`SMOKE_TITLES`] checks, so slow CI still sees the browser UI start and
    play, while the full `shots`, `ui-test` and `mobile` lanes run in the sweep."""
    cmd = [*BY, "ui-test", *SMOKE_TITLES]
    return Lane("ui-smoke", warm=True).step("ui-test", cmd, rerun=failed_titles)


def choice_flags(select: bool, affected: str) -> list[str]:
    return ["--select"] if select else ["--affected", affected] if affected else []


def oracle_lane() -> Lane:
    """Plays in XMage and Forge what `by oracle --select` chooses (oracle_selection.py): the
    changed cards, a sample and every scenario not yet passed, or every card when the exporters
    changed. A failure the divergence allowlist does not explain fails it."""
    cmd = [*BY, "oracle", "--select"]
    return Lane("oracle", warm=True).step("oracle", cmd, rerun=failed_cards)


# Seconds the fast check's oracle lane waits for another oracle run (slow CI's: ~90 s), then skips.
ORACLE_LOCK_WAIT = 10


def card_oracle_lane(cards: list[str]) -> Lane:
    """Plays the scenarios of `cards` and the new generated ones (`--new`) in XMage and Forge
    without Forge's AI games, passing when another run holds the oracles past the wait.
    """
    cmd = [*BY, "oracle", "--new", "--no-sim", "--lock-wait", str(ORACLE_LOCK_WAIT)]
    return Lane("oracle", warm=True).step("oracle", cmd + cards, rerun=failed_cards)
