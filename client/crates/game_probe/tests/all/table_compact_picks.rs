//! A card a player picked face down while a resolution waits, on the phone layout: it lies in
//! its owner's half of the window, clear of the rows' bands, so no permanent is covered.

use beryl_protocol::protocol::{CardType, Side, ZoneName};
use beryl_protocol::testing::ViewBuilder;
use game_table::table::Place;
use kit_probe::agent::checks::{on_screen, reachable};

use super::table_compact::{
    card_faults, cards_at, centre, compact_bands, compact_sizes, compact_window, frame_in,
};

#[test]
fn a_picked_card_lies_in_its_owners_half_clear_of_the_rows() {
    let mut view = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        let creature = view.card(ZoneName::Battlefield, side, "Bear");
        creature.types = vec![CardType::Creature];
        let pick = view.card(ZoneName::Hand, side, "Pick");
        pick.hidden_pick = true;
    }
    for (width, height) in compact_sizes() {
        let window = compact_window(width, height);
        let bands = compact_bands(window).expect("a phone window");
        let frame = frame_in(window, &view);
        for check in [reachable, on_screen] {
            assert_eq!(
                card_faults(&frame, window, check),
                Vec::<String>::new(),
                "{width}x{height}"
            );
        }
        for side in [Side::You, Side::Opponent] {
            let place = Place::Card {
                zone: ZoneName::Hand,
                side,
                row: None,
            };
            let picked = cards_at(&frame, place);
            assert_eq!(picked.len(), 1, "{width}x{height} {side:?}");
            let y = centre(picked[0]).y;
            let (own_half, rows) = match side {
                Side::You => (y > height / 2.0, [bands.your_creatures, bands.your_back]),
                Side::Opponent => (
                    y < height / 2.0,
                    [bands.opponent_creatures, bands.opponent_back],
                ),
            };
            assert!(own_half, "{width}x{height} {side:?}: the pick is at {y}");
            for band in rows {
                assert!(
                    y < band.top || y > band.bottom(),
                    "{width}x{height} {side:?}: the pick at {y} is in the band {band:?}"
                );
            }
        }
    }
}
