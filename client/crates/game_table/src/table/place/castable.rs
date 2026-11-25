//! The cards of the viewer's graveyard that the viewer may cast from there, which the table
//! shows at the end of the viewer's hand rather than on the graveyard pile.

use beryl_protocol::protocol::{ObjectView, Side, ZoneName};

use super::{Zones, side_index, side_of};

/// Whether `object` is a card of the viewer's graveyard that the viewer may cast from there
/// (`ObjectView::castable_from_graveyard`), which the table shows at the end of the viewer's
/// hand, as MTG Arena shows a card with flashback, until it is cast or may no longer be.
pub fn in_hand_fan(object: &ObjectView) -> bool {
    object.castable_from_graveyard
        && object.zone == ZoneName::Graveyard
        && side_of(object) == Side::You
}

/// The cards of `side`'s graveyard, in the pile's order, split into those the pile shows and
/// those the hand shows (`in_hand_fan`).
pub(super) fn graveyard_pile<'a>(
    zones: &Zones<'a>,
    side: Side,
) -> (Vec<&'a ObjectView>, Vec<&'a ObjectView>) {
    zones
        .listed(zones.piles[side_index(side)][1])
        .into_iter()
        .partition(|object| !in_hand_fan(object))
}
