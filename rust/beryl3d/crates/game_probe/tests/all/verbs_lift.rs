//! Where `lift` holds up cards taken from a hand, as the web interface's `handlingPlacements`
//! (`ui/src/game/layout.ts`) holds them: smaller and at the top left before the first turn, and
//! face down for a counted look at a pile.

use beryl_protocol::protocol::{CardHandling, ObjectId, PlayerView, Side, ZoneName};
use beryl_protocol::testing::ViewBuilder;
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

#[test]
fn large_lifted_groups_stay_in_their_bands_without_covering_each_other() {
    let data = crate::test_data::data();
    for size in [
        kit_ui::input::Size {
            width: 1440.0,
            height: 900.0,
        },
        kit_ui::input::Size {
            width: 393.0,
            height: 759.0,
        },
    ] {
        let profile = data.profiles.for_size(size);
        let window = game_table::table::Window {
            safe_area: profile.test_safe_area(),
            ..crate::common::window(size.width, size.height)
        };
        let table = game_table::table::Table::new(data.clone(), window);
        let lower = table.layout().hand.ceiling;
        let hand_top = if lower.is_finite() {
            (1.0 - lower) * size.height / 2.0
        } else {
            size.height / 2.0
        };
        for (zone, side, handling, backs, top, bottom) in [
            (
                ZoneName::Hand,
                Side::You,
                CardHandling::Hand { side: Side::You },
                false,
                hand_top,
                size.height - window.safe_area.bottom,
            ),
            (
                ZoneName::Library,
                Side::Opponent,
                CardHandling::Library {
                    side: Side::Opponent,
                    count: Some(12),
                    scry: Some(true),
                },
                true,
                window.safe_area.top,
                size.height / 2.0,
            ),
        ] {
            let mut builder = ViewBuilder::new();
            let slots: Vec<_> = (0..12)
                .map(|_| builder.card(zone, side, "Island").slot)
                .collect();
            let mut view = builder.build();
            view.turn.number = 3;
            view.handling = Some(handling);
            let placement = table.placement(&view, &[]);
            let mut bounds: Vec<(Vec2, Vec2)> = Vec::new();
            for slot in slots {
                let rest = placement.get(slot).expect("a lifted card's rest");
                assert!(rest.visible, "{zone:?} at {size:?}");
                assert_eq!(rest.pose.turn > 0.5, backs);
                let (corners, depth) = table.camera().card_box(&rest.pose);
                assert!(depth > 0.0 && rest.pose.scale > 0.0, "{rest:?}");
                let low = corners
                    .iter()
                    .copied()
                    .fold(Vec2::splat(f32::INFINITY), Vec2::min);
                let high = corners
                    .iter()
                    .copied()
                    .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
                assert!(
                    low.x >= window.safe_area.left - 0.01
                        && low.y >= top - 0.01
                        && high.x <= size.width - window.safe_area.right + 0.01
                        && high.y <= bottom + 0.01,
                    "{zone:?} at {size:?}: {low:?} to {high:?} outside {top} to {bottom}"
                );
                assert!(
                    bounds
                        .iter()
                        .all(|(a, b)| high.cmple(*a).any() || low.cmpge(*b).any()),
                    "{zone:?} at {size:?}: {low:?} to {high:?} covers another lifted card"
                );
                bounds.push((low, high));
            }
        }
    }
}
