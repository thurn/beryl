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


SLOW_ONLY: tuple[SlowOnly, ...] = ()


# The checks slow CI leaves to the fast check alone, as the run log names them:
# `rust:<binary>::<test path>` for a Rust test, `rust:doctests` and `rust:allocations` for the
# steps cargo test runs beside nextest (rust_tests.py), `python:<node id>` for a Python test
# (python_tests.py), `script:<id>` for a script check (script_checks.py), and `<lane>:<step>`
# for a whole step of a lane the fast check also runs, such as `ui:test` for the UI's Node
# tests or a formatter (slow_ci.without_fast_only). Each costs slow CI's CPU budget
# (budget.SLOW_CPU_BUDGET) and caught no bug there in the 30 days before it moved; the fast
# check still runs it before every push. A check here must not also be in SLOW_ONLY, or no
# check would run it.
FAST_ONLY: tuple[str, ...] = (
    "rust:doctests",
    "rust:allocations",
    "rust:beryl_search::rollout::policy::waste_tests::waste_declines_opposing_permanents_in_softmax_mirrors",
    "rust:beryl_cards::tests::every_card_has_scenarios_and_they_pass",
    "rust:beryl_session::session::chance::a_save_replays_the_same_under_another_seed",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::tests::first_example_offers_the_card_in_hand",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::tests::every_prepared_example_reaches_its_interaction",
    "rust:beryl_cards::exhaust::every_scenario_keeps_the_invariants_to_a_depth",
    "rust:beryl_rules::tests::replay::inline_and_replayed_answers_reach_the_same_state_at_every_decision",
    "rust:beryl_search::long::every_family_plays_a_game_to_its_end",
    "rust:beryl_cli::bin/beryl_cli::ladder::tests::positions_start_four_pairs_in_five",
    "rust:beryl_cards::tests::derive::generating_twice_gives_the_same_scenarios",
    "rust:beryl_cards::tests::derive::generated_scenarios_pass_in_every_slot_order",
    "rust:beryl_cli::bin/beryl_cli::screen::tests::a_log_that_no_longer_fits_is_refused_before_any_search",
    "rust:beryl_session::session::chance::games_that_found_slot_order_ties_replay_the_same_under_another_seed",
    "rust:beryl_search::mcts::an_agent_asked_again_searches_a_decision_as_it_did",
    "rust:beryl_cards::tests::the_dump_is_valid_json_with_every_scenario",
    "rust:beryl_cards::tests::mutation::every_mutant_of_a_card_is_killed_by_its_scenarios",
    "rust:beryl_session::session::sessions::no_view_shows_an_identity_its_viewer_does_not_know",
    "rust:beryl_session::session::undo::passing_on_goes_past_the_stops_in_its_step",
    "rust:beryl_bench::rollout::tests::rollouts_never_change",
    "rust:beryl_cli::bin/beryl_cli::why::tests::tree_report_counts_the_ordinary_search_visits",
    "rust:beryl_cards::tests::derive::every_ability_of_a_generated_card_is_exercised_by_its_generated_scenarios",
    "rust:beryl_session::session::sessions::a_game_against_the_random_agent_resumes_at_any_decision",
    "rust:beryl_session::session::undo::each_undo_takes_back_one_answer_and_replays",
    "rust:beryl_session::session::corpus::each_perturbation_breaks_a_save_of_indices",
    "rust:beryl_cards::tests::mutation::a_mutant_of_an_emblem_is_a_mutant_of_its_card",
    "rust:beryl_search::rollout::tests::greedy_holds_a_portable_hole_with_nothing_to_exile",
    "rust:beryl_search::long::expert_anchored_search_replays_a_game_from_its_seed",
    "rust:beryl_cards::tests::mutation::damn_that_allows_regeneration_is_killed_by_its_generated_scenarios",
    "rust:beryl_search::rollout::policy::hold_tests::hold_mirror_casts_at_the_opponents_end_step",
    "rust:beryl_cards::exhaust::a_broken_invariant_is_found_and_replays",
    "rust:beryl_search::long::tests::an_anchored_search_plays_greedy_unless_clearly_beaten",
    "rust:beryl_cards::tests::vocabulary::every_entry_a_card_says_is_carried_out_by_a_hand_written_scenario",
    "rust:beryl_cards::tests::props::scenarios_name_only_their_card_its_tokens_basic_lands_and_props",
    "rust:beryl_session::session::sessions::a_game_against_the_expert_resumes_at_any_decision",
    "rust:beryl_search::shrink::a_broken_invariant_shrinks_to_a_scenario",
    "rust:beryl_search::long::tests::an_expert_anchored_search_checks_nonpriority_choices",
    "rust:beryl_cli::bin/beryl_cli::specs::tests::moments_are_named_uniquely",
    "rust:beryl_search::expert::expert_beats_random",
    "rust:beryl_session::session::streaming::each_step_and_each_agent_decision_has_a_view_and_each_event_one_view",
    "rust:beryl_session::session::undo::undo_goes_back_past_the_agent_and_cards_seen",
    "rust:beryl_rules::tests::combat::random_combat_games_keep_the_invariants",
    "rust:beryl_search::games::mirror_games_rarely_stall",
    "rust:beryl_cards::tests::every_ability_and_casting_option_is_exercised_by_a_scenario",
    "rust:beryl_session::session::asks::every_prompt_the_scenarios_reach_is_sent_with_its_shape_and_a_whole_title",
    "rust:beryl_search::mcts::subtree_reuse_stays_within_one_action",
    "rust:beryl_session::session::streaming::a_human_who_can_play_stops_at_their_main_phase",
    "rust:beryl_cli::bin/beryl_cli::blunders::tests::combat_planner_holds_back_at_the_two_blocker_positions",
    "rust:beryl_search::long::tests::a_guarded_search_plays_greedy_at_attackers_unless_clearly_beaten",
    "rust:beryl_session::session::smart_stops::smart_stops_order_the_humans_triggers_and_exact_stops_ask",
    "rust:beryl_cards::prompts::every_decision_says_a_prompt_its_shape_and_part_allow",
    "rust:beryl_search::agents_tests::every_agent_plays_cards_with_no_overlay_entry_to_the_end",
    "rust:beryl_search::long::tests::mid_lets_greedy_answer_below_the_root",
    "rust:beryl_session::session::sessions::a_scripted_human_plays_a_whole_game_and_its_replay_reproduces_it",
    "rust:beryl_session::session::promotion::tokens_past_the_small_slots_play_on_in_large",
    "rust:beryl_cards::decks::tests::one_card_more_or_less_changes_only_decks_it_joins",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::history_tests::costs_tokens_returned_spells_and_coin_flips_are_reachable",
    "rust:beryl_session::session::undo::undo_keeps_the_latest_points",
    "rust:beryl_search::greedy::greedy_beats_random",
    "rust:beryl_search::long::the_hidden_searches_decide_the_same_on_a_redeterminized_state",
    "rust:beryl_bench::family::tests::families_pair_their_steps",
    "rust:beryl_session::session::smart_stops::a_humans_own_spell_resolves_without_their_priority_under_smart_stops_only",
    "rust:beryl_session::session::checkpoints::every_board_rebuilds_as_it_was",
    "rust:beryl_session::session::cancel::a_cast_taken_back_at_its_target_returns_to_priority_as_it_was",
    "rust:beryl_session::session::cancel::a_cast_taken_back_flies_its_card_to_the_hand_again",
    "rust:beryl_session::session::full_control::full_control_gives_priority_over_every_opponent_spell_in_either_turn",
    "rust:beryl_session::session::checkpoints::each_turn_s_board_is_recorded_and_replays",
    "rust:beryl_session::session::chance::a_version_2_save_upgrades_to_version_3_from_its_seed",
    "rust:beryl_session::session::checkpoints::an_impossible_answer_resumes_at_its_turn_s_beginning",
    "rust:beryl_search::long::neither_search_thinks_about_the_order_of_cards_for_the_library_bottom",
    "rust:beryl_session::session::smart_stops::games_against_responses_keep_the_smart_stops_and_replay",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::history_tests::alternate_casts_cycling_and_tax_payment_remain_choices",
    "rust:beryl_search::games::agents_decide_the_same_on_a_redeterminized_state",
    "rust:beryl_session::session::chance::a_save_records_each_card_as_it_is_first_seen_and_the_flip_winner",
    "rust:beryl_search::long::turn::tests::turn_choice_is_independent_of_hidden_cards",
    "rust:beryl_session::session::checkpoints::a_board_the_replay_does_not_reach_is_rebuilt_from_the_record",
    "rust:beryl_search::games::a_seed_and_its_choices_reproduce_every_state_hash",
    "rust:beryl_cli::bin/beryl_cli::why::tests::compares_oust_and_pass",
    "rust:beryl_session::session::checkpoints::a_lingering_effect_refuses_a_rebuild",
    "rust:beryl_cli::bin/beryl_cli::why::tests::identical_choices_have_identical_rollouts",
    "rust:beryl_session::session::chance::a_recorded_reveal_that_never_happens_stops_the_replay",
    "rust:beryl_search::long::a_seed_and_settings_reproduce_a_move_on_any_thread_count",
    "rust:beryl_session::session::chance::a_version_4_save_replays_without_owners_or_cards_shown",
    "rust:beryl_search::games::a_widened_game_plays_on_as_the_small_one",
    "rust:beryl_session::session::chance::a_recorded_card_no_unseen_card_can_be_stops_the_replay",
    "rust:beryl_session::session::corpus::every_frozen_save_plays_the_same_under_each_perturbation",
    "rust:beryl_cli::bin/beryl_cli::specs::playground::history_tests::subtlety_keeps_opponents_bottom_choice_and_viewers_prompt",
    "rust:beryl_search::games::a_sixty_card_game_round_trips_between_the_layouts",
    "rust:beryl_search::mcts::the_exact_stop_chooses_as_the_full_budget",
    "python:tests/test_checks.py::test_a_hung_step_is_killed_at_its_hang_limit_with_its_descendants",
    "python:tests/test_bisect.py::test_a_probe_that_hangs_dies_at_the_search_limit_and_keeps_its_range",
    "python:tests/test_checks.py::test_a_loaded_machine_stretches_the_hang_limit_and_the_budget",
    "python:tests/test_video_measure.py::test_the_command_measures_a_recording_and_a_folder_of_frames",
    "script:fixtures/combat-attackers/yours",
    "script:fixtures/choice-trigger-order/yours",
    "script:fixtures/late-game/yours",
    "ui:test",
    # Formatters and file checks the fast check runs on every push that touches their files:
    # slow CI ran them again on a tree they had already passed.
    "ui:prettier",
    "rust:fmt",
    "rust:build-paths",
    "rust:card-text",
    "repo:just",
    "repo:markdown",
    "repo:conventions",
)


def fast_only(lane: str) -> list[str]:
    """The checks of `lane` that slow CI leaves to the fast check, without their `<lane>:`."""
    return [c.removeprefix(f"{lane}:") for c in FAST_ONLY if c.startswith(f"{lane}:")]
