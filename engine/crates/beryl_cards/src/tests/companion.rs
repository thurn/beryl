//! A scenario may record a revealed companion only where the pregame question would have
//! offered it ([CR 103.2b](https://yawgatog.com/resources/magic-rules/#R1032b)): "A player may
//! reveal no more than one card this way, and they may do so only if their deck fulfills the
//! condition of that card’s companion ability."
//!
//! A card scenario can't show a refusal, since the setup itself fails, so this test runs the
//! scenario and reads the failure.

use beryl_scenario::{Action, P0, Scenario};

use crate::{CARDS, names, props};

/// Lurrus of the Dream-Den revealed, with a three-mana creature card in the library.
const THREE_MANA_PERMANENT: Scenario =
    Scenario::new("Lurrus can't be revealed from a deck with a three-mana permanent")
        .sideboard(P0, &[names::LURRUS_OF_THE_DREAM_DEN])
        .companion(P0, names::LURRUS_OF_THE_DREAM_DEN)
        .battlefield(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
        .library(P0, &[props::FLIER.card])
        .actions(&[Action::Companion { player: P0 }]);

#[test]
fn a_deck_that_fails_the_condition_may_not_reveal_its_companion() {
    let failure = beryl_scenario::run(&CARDS, &THREE_MANA_PERMANENT).unwrap_err();
    assert_eq!(failure.action, None, "{failure}");
    assert!(
        failure
            .message
            .contains("not a companion this deck may reveal"),
        "{failure}"
    );
    // The same deck with the creature card in the sideboard may reveal it: the condition
    // reads the starting deck alone.
    let sideboarded = THREE_MANA_PERMANENT
        .library(P0, &[names::PLAINS])
        .sideboard(P0, &[names::LURRUS_OF_THE_DREAM_DEN, props::FLIER.card]);
    beryl_scenario::run(&CARDS, &sideboarded).unwrap_or_else(|e| panic!("{e}"));
}
