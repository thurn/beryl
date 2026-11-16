//! Where `lift` holds up cards taken from a hand, as the web interface's `handlingPlacements`
//! (`ui/src/game/layout.ts`) holds them: smaller and at the top left before the first turn, and
//! face down for a counted look at a pile.

use beryl_protocol::protocol::{CardHandling, ObjectId, PlayerView, Side, ZoneName};
use beryl_scene::testing::ViewBuilder;
use glam::Vec2;

use crate::common::show;

/// A view with three cards in the viewer's hand, which the viewer is handling as `handling`
/// says, on the game's turn `turn`; and the slots of the three cards.
fn handling(handling: CardHandling, turn: u16) -> (PlayerView, Vec<ObjectId>) {
    let mut builder = ViewBuilder::new();
    let slots = ["Island", "Plains", "Counterspell"]
        .map(|name| builder.card(ZoneName::Hand, Side::You, name).slot)
        .to_vec();
    let mut view = builder.build();
    view.turn.number = turn;
    view.handling = Some(handling);
    (view, slots)
}

/// The middle on the screen, in logical points, the scale and the turn of each of `slots` as
/// the table places them in `view`.
fn held(view: &PlayerView, slots: &[ObjectId]) -> Vec<(Vec2, f32, f32)> {
    let show = show();
    let table = show.table();
    let placement = table.placement(view, &[]);
    slots
        .iter()
        .map(|slot| {
            let pose = placement.get(*slot).expect("a place").pose;
            (
                table.camera().to_screen(pose.position),
                pose.scale,
                pose.turn,
            )
        })
        .collect()
}

#[test]
fn an_opening_hand_is_held_up_smaller_and_at_the_top_left() {
    let look = CardHandling::Hand { side: Side::You };
    let (opening, slots) = handling(look, 0);
    let (later, _) = handling(look, 3);
    let show = show();
    let window = show.table().camera().window.size;
    let (opening, later) = (held(&opening, &slots), held(&later, &slots));
    // The middle card: the web centres it 335 of the stage's 900 pixels of height left of the
    // screen's middle, 58 down from its top, and 72 pixels wide against 104.
    let ((at, small, _), (there, large, _)) = (opening[1], later[1]);
    let from_middle = (window.width / 2.0 - at.x) / window.height * 900.0;
    assert!(
        (from_middle - 335.0).abs() < 4.0,
        "{from_middle} left of the middle"
    );
    let down = at.y / window.height * 900.0;
    assert!((down - 58.0).abs() < 4.0, "{down} down");
    assert!(
        (small / large - 72.0 / 104.0).abs() < 0.01,
        "{small} against {large}"
    );
    assert!(there.y > window.height / 2.0, "later it is low: {there}");
    for (_, _, turn) in opening {
        assert!(turn < 0.5, "the viewer's own opening hand shows its faces");
    }
}

#[test]
fn a_counted_look_at_your_own_hand_shows_backs() {
    let (hand, slots) = handling(CardHandling::Hand { side: Side::You }, 3);
    let (pile, _) = handling(
        CardHandling::Pile {
            side: Side::You,
            pile: ZoneName::Hand,
            count: 3,
            bottom: false,
        },
        3,
    );
    for (_, _, turn) in held(&hand, &slots) {
        assert!(turn < 0.5, "a look at the hand shows faces");
    }
    for (_, _, turn) in held(&pile, &slots) {
        assert!(turn > 0.5, "a counted look at a pile shows backs");
    }
}
