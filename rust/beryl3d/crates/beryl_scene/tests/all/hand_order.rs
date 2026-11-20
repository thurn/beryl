//! Each hand lists its cards in the order they arrived, as the web interface's `Table.order`
//! keeps it: a drawn card joins the right end of the hand, as Arena's does, though the session
//! lists a view's objects by slot and the drawn card has the lowest slot of the hand.

use crate::common::{MS, before, discard, draw, empty_frame, frame, show};
use beryl_protocol::protocol::{ObjectId, ZoneName};
use beryl_scene::play::Show;

/// The viewer's hand cards in the view the show rests on, as it lists them.
fn hand(show: &Show) -> Vec<ObjectId> {
    let shown = show.shown().expect("a view is shown");
    (shown.view.objects.iter())
        .filter(|object| object.zone == ZoneName::Hand)
        .map(|object| object.slot)
        .collect()
}

/// Where `slot` rests across the screen, its position's x.
fn x_of(show: &Show, slot: ObjectId) -> f32 {
    show.latest_rest(usize::from(slot.0))
        .expect("the card rests on the table")
        .position
        .x
}

#[test]
fn a_drawn_card_rests_at_the_right_end_of_the_hand_and_stays_there() {
    let (start, cards) = before();
    let held: Vec<ObjectId> = (start.view.objects.iter())
        .filter(|object| object.zone == ZoneName::Hand)
        .map(|object| object.slot)
        .collect();
    assert!(
        held.iter().all(|&slot| slot > cards.top),
        "the drawn card has a lower slot than every card in the hand"
    );
    let (drawn, timings) = draw(&start, cards);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(drawn.clone()), timings);
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 0);
    frame(&mut show, &mut scratch, 400 * MS);

    let mut expected = held.clone();
    expected.push(cards.top);
    assert_eq!(hand(&show), expected, "the drawn card is listed last");
    let rest = x_of(&show, cards.top);
    for &slot in &held {
        assert!(
            x_of(&show, slot) < rest,
            "card #{} rests left of the drawn card",
            slot.0
        );
    }

    // A later view, which the session again lists by slot, keeps the order.
    let (discarded, timings) = discard(&drawn, cards);
    show.push(Box::new(discarded), timings);
    frame(&mut show, &mut scratch, 401 * MS);
    frame(&mut show, &mut scratch, 900 * MS);
    let kept: Vec<ObjectId> = expected
        .into_iter()
        .filter(|&slot| slot != cards.discard)
        .collect();
    assert_eq!(
        hand(&show),
        kept,
        "the hand keeps its order after a discard"
    );
    let right = (kept.iter().map(|&slot| x_of(&show, slot))).fold(f32::MIN, f32::max);
    assert!(
        (x_of(&show, cards.top) - right).abs() < 1e-3,
        "the drawn card still rests at the right end"
    );
}
