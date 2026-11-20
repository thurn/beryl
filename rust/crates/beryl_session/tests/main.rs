//! Every integration test of the session in one test binary. Each file under `tests/` is a
//! module here rather than a binary of its own: each binary links the whole engine and every
//! card (about 55 MB), so 39 of them made each rules change relink 39 binaries, and on macOS
//! each newly linked binary waits for the system's malware scan on its first launch.

mod common;

/// The folder of the frozen saves, opened through the helper so that the saves are inputs of
/// the tests that replay them.
pub(crate) fn saves_dir() -> std::path::PathBuf {
    let relative = "rust/crates/beryl_session/tests/saves";
    beryl_test_files::read_dir(relative).expect("the saves folder");
    beryl_test_files::root().expect("a checkout").join(relative)
}
#[path = "common/play.rs"]
mod play;

mod ability_events;
mod answers;
mod asks;
mod cancel;
mod candidates;
mod card_records;
mod chance;
mod checkpoints;
mod chosen_values;
mod clicks;
mod controlled;
mod copy_saves;
mod corpus;
mod cost_replay;
mod counters;
mod decider_saves;
mod declarations;
mod divisions;
mod face_down;
mod fair;
mod full_control;
mod fuzz;
mod game_over;
mod games_ledger;
mod hidden_picks;
mod modes;
mod object_states;
mod own_turn;
mod paid_costs;
mod payments;
mod picks;
mod preview;
mod promotion;
mod prompt;
mod refused;
mod replacement_saves;
mod saves;
mod script;
mod script_build;
mod script_manual;

mod script_order;
mod script_piles;
mod script_spans;
mod script_timing;
mod sessions;
mod smart_stops;
mod splits;
mod stop_places;
mod streaming;
mod table_actions;
mod typescript;
mod undo;
mod zone_picks;

/// A file under `tests/` that this file does not declare would never be compiled or run, since
/// the crate turns off cargo's automatic test targets (`autotests = false`).
#[test]
fn every_test_file_is_a_module_here() {
    let declared = include_str!("main.rs");
    let mut missing = Vec::new();
    for entry in
        beryl_test_files::read_dir("rust/crates/beryl_session/tests").expect("the tests directory")
    {
        let path = entry.expect("a directory entry").path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let is_test_file = path.extension().is_some_and(|e| e == "rs") && stem != "main";
        if is_test_file && !declared.contains(&format!("\nmod {stem};")) {
            missing.push(stem.to_owned());
        }
    }
    assert!(
        missing.is_empty(),
        "add `mod <name>;` to tests/main.rs for {missing:?}"
    );
}
