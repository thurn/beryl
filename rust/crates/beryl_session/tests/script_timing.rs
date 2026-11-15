//! Every script the session sends carries one timing for each of its steps
//! (`beryl_session::script::Script::timing`): scripts built as a scenario plays, and scripts
//! of a game resumed from a save of the corpus.

use std::collections::BTreeSet;
use std::path::Path;

use beryl_cards::{CARDS, names};
use beryl_session::corpus;
use beryl_session::protocol::PlayerView;
use beryl_session::script::Script;
use beryl_session::{Game, SessionId};

use crate::common::views;

/// Whether `script` has a timing for each step, saying where it does not.
fn assert_timed(script: &Script, where_: &str) {
    assert_eq!(
        script.timing.len(),
        script.steps(),
        "{where_}: one timing per step\n{script}"
    );
}

/// Trinket Mage is cast and resolves: each seat's scripts carry one timing per step, the
/// caster's flight to the stack timed as their own spell's, the other seat's as the
/// opponent's, and its flight onto the battlefield as a permanent's that resolves.
#[test]
fn a_cast_and_its_resolution_are_timed_step_for_step() {
    let views = views(
        names::TRINKET_MAGE,
        "Trinket Mage finds Portable Hole",
        |_, _, _| {},
    );
    let mut cases = [BTreeSet::new(), BTreeSet::new()];
    for (n, seats) in views.iter().enumerate() {
        for (seat, view) in seats.iter().enumerate() {
            let script = view.script.as_ref().expect("a view's script");
            assert_timed(script, &format!("view {n} seat {seat}"));
            cases[seat].extend(script.timing.iter().filter_map(|t| t.case.clone()));
        }
    }
    let both: BTreeSet<_> = cases[0].union(&cases[1]).cloned().collect();
    for case in [
        "own spell cast",
        "opponent spell cast",
        "permanent resolves",
    ] {
        assert!(both.contains(case), "no step timed as {case:?} in {both:?}");
    }
}

/// A game resumed from a save of the corpus, and played on by a stand-in for the human, sends
/// a timed script with every view. Most saves are of games that ended: the first that goes on
/// is played.
#[test]
fn a_game_resumed_from_a_save_sends_timed_scripts() {
    let crate_dir = std::env::var_os("CARGO_MANIFEST_DIR").expect("run under cargo");
    let dir = Path::new(&crate_dir).join("tests/saves");
    let mut saves: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".json.gz"))
        .collect();
    saves.sort();
    for save in &saves {
        let (file, _) = corpus::load(&CARDS, save).unwrap();
        let Ok((mut game, first)) = Game::from_replay(&CARDS, &file, SessionId(0)) else {
            continue;
        };
        game.set_stand_in("random").unwrap();
        let mut shown: Vec<PlayerView> = vec![first];
        while shown.len() < 40 {
            if game.answer_by_stand_in() {
                continue;
            }
            match game.next_view() {
                Some(view) => shown.push(view),
                None => break,
            }
        }
        let mut steps = 0;
        for (n, view) in shown.iter().enumerate() {
            let script = view.script.as_ref().expect("a view's script");
            assert_timed(script, &format!("{} view {n}", save.display()));
            steps += script.steps();
        }
        if steps > 0 {
            return;
        }
    }
    panic!("no save of the corpus resumes into a game that plays on");
}
