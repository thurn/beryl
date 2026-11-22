//! Pointer input for a board whose hand is fanned out.

use beryl_protocol::protocol::{Side, ZoneName};

/// A four-second round trip along the inner edge of the fan in its resting frame, card by card
/// in hand order through the cards in view, so through each row of a hand laid in several. The position follows the curvature
/// of the edge across window shapes.
pub(crate) fn pointer_moves(frame: &kit_frame::frame::Frame) -> Vec<(u64, kit_ui::input::Pointer)> {
    use app_client::driver::TICK;
    use kit_frame::frame::{Place, Role};
    use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

    const HALF: u16 = 120;
    let points: Vec<_> = frame
        .resolved
        .iter()
        .filter(|element| {
            element.role == Role::Card
                && matches!(
                    element.place,
                    Place::Card {
                        zone: ZoneName::Hand,
                        side: Side::You,
                        ..
                    }
                )
        })
        .map(|element| {
            // The card's top edge on screen is the edge of its two highest corners, and its
            // strip to press runs down from that edge's left end, in each row of the hand.
            let mut corners = element.corners;
            corners.sort_by(|a, b| a.y.total_cmp(&b.y));
            let [a, b, c, d] = corners;
            let (top_left, top_right) = if a.x < b.x { (a, b) } else { (b, a) };
            let bottom_left = if c.x < d.x { c } else { d };
            top_left.lerp(top_right, 0.09) + 0.08 * (bottom_left - top_left)
        })
        .collect();
    // A phone shows as many of the 60 cards as keep a strip to press, and scrolls the rest.
    assert!(points.len() > 1, "the hand shows its cards");
    // A point, or the nearest point below it where a hit test finds a card of the viewer's
    // hand: a step stop beside the viewer's life orb covers the top of a few cards of a hand
    // this large, and the pointer crosses the hand, not the stops.
    let in_hand = |index: u32| {
        let element = &frame.resolved[index as usize];
        element.role == Role::Card
            && matches!(
                element.place,
                Place::Card {
                    zone: ZoneName::Hand,
                    side: Side::You,
                    ..
                }
            )
    };
    let onto_hand = |point| {
        let mut below = point;
        for _ in 0..40 {
            if kit_scene::hit::hit(frame, below).is_some_and(in_hand) {
                return below;
            }
            below.y += 2.0;
        }
        point
    };
    let gaps = u16::try_from(points.len() - 1).expect("at most 60 points");
    (1..=2 * HALF)
        .map(|step| {
            let (distance, length) = if step <= HALF {
                (step - 1, HALF - 1)
            } else {
                (2 * HALF - step, HALF)
            };
            let along = distance * gaps;
            let index = usize::from(along / length);
            let position = points.get(index + 1).map_or(points[index], |next| {
                points[index].lerp(*next, f32::from(along % length) / f32::from(length))
            });
            let position = onto_hand(position);
            (
                u64::from(step) * TICK.0,
                Pointer {
                    id: 0,
                    kind: PointerKind::Mouse,
                    phase: PointerPhase::Move,
                    position,
                    button: Button::None,
                    #[expect(
                        clippy::default_trait_access,
                        reason = "the vector type comes from the frame"
                    )]
                    wheel: Default::default(),
                    modifiers: Modifiers::default(),
                },
            )
        })
        .collect()
}
