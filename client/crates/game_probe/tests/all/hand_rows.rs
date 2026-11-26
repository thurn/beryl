//! A phone hand too large for one row: every row above the front one stands right of the
//! viewer's piles, so the piles stay in view and every card keeps a square to press.

use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::testing::ViewBuilder;
use game_table::table::Place;
use kit_frame::frame::Role;
use kit_probe::agent::checks::reachable;

use crate::table_compact::{
    bounds, card_faults, cards_at, compact_sizes, compact_window, frame_in, uneven,
};

fn meets(first: [f32; 4], second: [f32; 4]) -> bool {
    first[0] < second[2] && second[0] < first[2] && first[1] < second[3] && second[1] < first[3]
}

#[test]
fn a_hand_of_several_rows_leaves_the_viewers_piles_in_view() {
    let mut windows: Vec<_> = compact_sizes()
        .into_iter()
        .map(|(width, height)| compact_window(width, height))
        .collect();
    windows.push(uneven(393.0, 759.0));
    for count in [6, 7, 8, 9, 14, 30] {
        let mut view = ViewBuilder::new();
        for index in 0..count {
            view.card(ZoneName::Hand, Side::You, &format!("Card {index}"));
        }
        for index in 0..3 {
            view.card(ZoneName::Library, Side::You, &format!("Library {index}"));
            view.card(
                ZoneName::Graveyard,
                Side::You,
                &format!("Graveyard {index}"),
            );
        }
        for &window in &windows {
            let size = window.size;
            let label = format!("{count} cards at {}x{}", size.width, size.height);
            let frame = frame_in(window, &view);
            assert_eq!(
                card_faults(&frame, window, reachable),
                Vec::<String>::new(),
                "{label}"
            );
            let hand = cards_at(
                &frame,
                Place::Card {
                    zone: ZoneName::Hand,
                    side: Side::You,
                    row: None,
                },
            );
            assert!(!hand.is_empty(), "{label}");
            // A pile is one element, and its top card may also be drawn as a card.
            let piles: Vec<_> = (frame.resolved.iter())
                .filter(|element| {
                    element.role == Role::Pile
                        && matches!(
                            Place::in_frame(element.place),
                            Some(Place::Card {
                                side: Side::You,
                                zone: ZoneName::Library | ZoneName::Graveyard,
                                ..
                            })
                        )
                })
                .chain(
                    [ZoneName::Library, ZoneName::Graveyard]
                        .into_iter()
                        .flat_map(|zone| {
                            cards_at(
                                &frame,
                                Place::Card {
                                    zone,
                                    side: Side::You,
                                    row: None,
                                },
                            )
                        }),
                )
                .collect();
            assert!(piles.len() >= 2, "{label}: {} piles", piles.len());
            for pile in &piles {
                for card in &hand {
                    assert!(
                        !meets(bounds(card), bounds(pile)),
                        "{label}: a hand card at {:?} meets a pile at {:?}",
                        bounds(card),
                        bounds(pile)
                    );
                }
            }
        }
    }
}
