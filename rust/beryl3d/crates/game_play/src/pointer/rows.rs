//! Which arranged row a card belongs to.

use beryl_protocol::protocol::{Side, ZoneName};
use game_table::table::Place;
use game_table::table::Scroller;

/// The line of cards that scrolls which a card at `place` stands in, if any.
#[must_use]
pub fn scroller_of(place: kit_frame::frame::Place) -> Option<Scroller> {
    let Some(Place::Card { zone, side, row }) = Place::in_frame(place) else {
        return None;
    };
    match (zone, row) {
        (ZoneName::Battlefield, Some(row)) => Some(Scroller::Row(side, row)),
        (ZoneName::Hand, _) if side == Side::You => Some(Scroller::Hand),
        (ZoneName::Stack, _) => Some(Scroller::Stack),
        _ => None,
    }
}
