"""The checks the fast check leaves to slow CI alone (verdicts.py re-exports them): the
pruning rule's moves (prune.py) and those a bead placed there, each with its fast-check record
when it moved. Also the reverse, [`FAST_ONLY`]: the checks slow CI leaves to the fast check
alone."""

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
    # The session fuzz's handful of games (beryl_session's fuzz.rs): slow CI
    # and the nightly's `just fuzz` play them, never the fast check.
    SlowOnly(
        "rust:beryl_session::fuzz::session_games_keep_the_script_checks",
        "2026-11-11",
        2.5,
        0,
    ),
)


# The checks slow CI leaves to the fast check alone: `rust:<binary>::<test path>` for a Rust
# test (as the run log names it), or `rust:doctests` and `rust:allocations` for the steps
# cargo test runs beside nextest (rust_tests.py). Each costs slow CI's CPU budget
# (budget.SLOW_CPU_BUDGET) and caught no bug there in the 30 days before it moved; the fast
# check still runs it before every push. A check here must not also be in SLOW_ONLY, or no
# check would run it.
FAST_ONLY: tuple[str, ...] = (
    "rust:doctests",
    "rust:allocations",
    "rust:beryl_search::rollout::policy::waste_tests::waste_declines_opposing_permanents_in_softmax_mirrors",
    "rust:beryl_cards::tests::every_card_has_scenarios_and_they_pass",
    "rust:beryl_session::chance::a_save_replays_the_same_under_another_seed",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::tests::first_example_offers_the_card_in_hand",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::tests::every_prepared_example_reaches_its_interaction",
    "rust:beryl_cards::exhaust::every_scenario_keeps_the_invariants_to_a_depth",
    "rust:beryl_rules::tests::replay::inline_and_replayed_answers_reach_the_same_state_at_every_decision",
    "rust:beryl_search::long::every_family_plays_a_game_to_its_end",
    "rust:beryl_cli::bin/beryl_cli::ladder::tests::positions_start_four_pairs_in_five",
    "rust:beryl_cards::tests::derive::generating_twice_gives_the_same_scenarios",
    "rust:beryl_cards::tests::derive::generated_scenarios_pass_in_every_slot_order",
    "rust:beryl_cli::bin/beryl_cli::screen::tests::a_log_that_no_longer_fits_is_refused_before_any_search",
    "rust:beryl_session::chance::games_that_found_slot_order_ties_replay_the_same_under_another_seed",
    "rust:beryl_search::mcts::an_agent_asked_again_searches_a_decision_as_it_did",
    "rust:beryl_cards::tests::the_dump_is_valid_json_with_every_scenario",
    "rust:beryl_cards::tests::mutation::every_mutant_of_a_card_is_killed_by_its_scenarios",
    "rust:beryl_session::sessions::no_view_shows_an_identity_its_viewer_does_not_know",
    "rust:beryl_session::undo::passing_on_goes_past_the_stops_in_its_step",
    "rust:beryl_bench::rollout::tests::rollouts_never_change",
    "rust:beryl_cli::bin/beryl_cli::why::tests::tree_report_counts_the_ordinary_search_visits",
    "rust:beryl_cards::tests::derive::every_ability_of_a_generated_card_is_exercised_by_its_generated_scenarios",
)
