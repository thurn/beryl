//! Who decks first in a shared library: the draws alternate from one pile, so the player who
//! must draw after its last card loses, whichever player that is.

use beryl_cards::{CARDS, names};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Outcome, Profile, Scratch, Seat, TurnStep};
use beryl_scenario::{P0, P1, Scenario, position};
use beryl_search::decking::{first_to_deck, next_drawer};

use crate::common::{agents, play};

/// With one card left in the shared library, the player who draws next takes it and the other
/// player decks; with none left, the player who draws next decks. Each forecast is then played
/// out, with nothing in either hand and no permanent, and the other player wins.
#[test]
fn the_player_to_draw_after_the_last_card_decks() {
    const ONE: &[&str] = &[names::ISLAND];
    let cases: [(Scenario, Seat, Seat); 4] = [
        // Our upkeep: we draw the last card, the opponent decks.
        (
            Scenario::new("we draw the last card")
                .at(3, P0, TurnStep::Upkeep)
                .shared_library(ONE),
            P0,
            P1,
        ),
        // Our main phase: the opponent draws the last card, we deck.
        (
            Scenario::new("they draw the last card")
                .at(3, P0, TurnStep::PrecombatMain)
                .shared_library(ONE),
            P1,
            P0,
        ),
        // The starting player's first upkeep has no draw: the opponent draws next.
        (
            Scenario::new("no draw on the first turn")
                .at(1, P0, TurnStep::Upkeep)
                .shared_library(ONE),
            P1,
            P0,
        ),
        // An empty library: the opponent, to draw next, decks.
        (
            Scenario::new("an empty library")
                .at(3, P0, TurnStep::PrecombatMain)
                .shared_library(&[]),
            P1,
            P1,
        ),
    ];
    for (scenario, next, decks) in cases {
        let name = scenario.name;
        let (t, mut s) = position(&CARDS, &scenario).unwrap();
        s.set_profile(Profile::SMART);
        assert!(s.shares_zones(), "{name}");
        assert_eq!(next_drawer(&s), next, "{name}");
        assert_eq!(first_to_deck(&s), decks, "{name}");
        let outcome = play(&t, &mut s, &mut Scratch::new(), &mut agents(1), |_, _| {});
        assert_eq!(outcome, Outcome::Win(decks.opponent()), "{name}");
    }
}
