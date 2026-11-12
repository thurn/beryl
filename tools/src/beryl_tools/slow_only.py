"""The checks the fast check leaves to slow CI alone (verdicts.py re-exports them): the
pruning rule's moves (prune.py) and those a bead placed there, each with its fast-check record
when it moved."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class SlowOnly:
    """A check the pruning rule moved to slow CI alone (prune.py: never failed, and in the
    costliest tenth of the fast check by seconds per run), or that its bead placed there, on
    `date`: checks.fast() leaves it out and slow CI runs it as before. `seconds` (a run) and
    `runs` are its fast-check record when it moved, which keeps it in the ranking of later
    prunes."""

    check: str
    date: str
    seconds: float
    runs: int


SLOW_ONLY: tuple[SlowOnly, ...] = (
    # The session fuzz's handful of games (beryl_session's fuzz.rs, by-w8gv.21.12): slow CI
    # and the nightly's `just fuzz` play them, never the fast check.
    SlowOnly(
        "rust:beryl_session::fuzz::session_games_keep_the_script_checks",
        "2026-11-11",
        2.5,
        0,
    ),
    SlowOnly(
        "rust:beryl_session::fuzz::a_planted_builder_fault_shrinks_to_a_few_choices",
        "2026-11-11",
        2.2,
        0,
    ),
    # Games played whole or at random for the AI's tools and invariants, the slowest tests left
    # after the long-think arena's and those that set the tests step's length: about 160 test-seconds a
    # fast check over 14 days and no fast-check failure. Their one slow-CI failure was a single
    # run (2b51781d) that failed 45 tests at once, which kept the pruning rule from moving them.
    # Slow CI, which plays them as before, judges them.
    SlowOnly(
        "rust:beryl_cli::bin/beryl_cli::positions::tests::a_seed_rebuilds_its_set_and_each_position_plays_on",
        "2026-11-11",
        27.1,
        147,
    ),
    SlowOnly(
        "rust:beryl_search::games::random_games_keep_the_invariants",
        "2026-11-11",
        26.8,
        147,
    ),
    SlowOnly(
        "rust:beryl_cli::bin/beryl_cli::screen::tests::the_champion_against_itself_differs_nowhere",
        "2026-11-11",
        23.2,
        147,
    ),
    SlowOnly(
        "rust:beryl_cli::bin/beryl_cli::hunt::tests::collected_positions_replay_and_detect_drift",
        "2026-11-11",
        21.8,
        147,
    ),
    SlowOnly(
        "rust:beryl_cli::bin/beryl_cli::screen::tests::one_crafted_difference_counts_and_duels_once",
        "2026-11-11",
        18.6,
        147,
    ),
    SlowOnly(
        "rust:beryl_search::mcts::a_rebuilt_agent_plays_on_as_the_uninterrupted_one",
        "2026-11-11",
        14.2,
        147,
    ),
    SlowOnly(
        "rust:beryl_cli::bin/beryl_cli::positions::tests::lingering_rejects_a_cut_and_known_cards_stay_known",
        "2026-11-11",
        13.6,
        147,
    ),
    SlowOnly(
        "rust:beryl_search::shrink::fuzz_games_keep_the_invariants",
        "2026-11-11",
        12.2,
        145,
    ),
)
