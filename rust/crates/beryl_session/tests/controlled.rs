//! A turn one player controls for another (CR 724): what each seat's view shows of it.

use beryl_cards::names;
use beryl_session::protocol::{PlayerView, Side, ZoneName};

use crate::common;

/// The name the view gives the card in the opponent's hand, if it shows one.
fn opponents_hand(view: &PlayerView) -> Vec<Option<String>> {
    (view.objects.iter())
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::Opponent)
        .map(|o| o.name.clone())
        .collect()
}

/// As [CR 724.4] says, "If information about an object in the game would be visible to the player
/// being controlled, it's visible to both that player and the controller of the player."
/// P0, controlling P1's turn 2, sees P1's Oust and the Island P1 draws, and still knows both
/// in their own turn after. P1's view of P0's cards does not change.
///
/// [CR 724.4]: https://yawgatog.com/resources/magic-rules/#R7244
#[test]
fn the_controller_sees_the_controlled_players_hand_and_remembers_it() {
    let views = common::views(
        names::MINDSLAVER,
        "the control ends with the turn",
        |_, _, _| {},
    );
    let hand = |turn: u16| {
        let [mine, theirs] = views
            .iter()
            .find(|[mine, _]| mine.turn.number == turn)
            .expect("a view in the turn");
        (opponents_hand(mine), opponents_hand(theirs))
    };
    let [oust, island] = ["Oust", "Island"].map(|name| Some(name.to_owned()));
    assert_eq!(hand(1), (vec![None], vec![]));
    assert_eq!(hand(2).0, vec![oust.clone(), island.clone()]);
    assert_eq!(hand(3).0, vec![oust, island]);
    assert!(hand(2).1.is_empty());
}
