//! The cards of the viewer's graveyard that the viewer may cast from there, which the table
//! shows at the end of the viewer's hand rather than on the graveyard pile.

use beryl_protocol::protocol::{ObjectView, Side, ZoneName};

use super::{Zones, side_index};

/// Whether `object` is a graveyard card that the viewer may cast from there
/// (`ObjectView::castable_from_graveyard`), which the table shows at the end of the viewer's
/// hand, as MTG Arena shows a card with flashback, until it is cast or may no longer be.
pub fn in_hand_fan(object: &ObjectView) -> bool {
    object.castable_from_graveyard && object.zone == ZoneName::Graveyard
}

/// The cards of `side`'s graveyard (or the shared graveyard when `side` is `None`), in the
/// pile's order, split into those the pile shows and those the hand shows (`in_hand_fan`).
pub(super) fn graveyard_pile<'a>(
    zones: &Zones<'a>,
    side: Option<Side>,
) -> (Vec<&'a ObjectView>, Vec<&'a ObjectView>) {
    let cards = zones.listed(side.map_or(zones.shared_piles[1], |s| zones.piles[side_index(s)][1]));
    if side == Some(Side::Opponent) {
        return (cards, Vec::new());
    }
    cards.into_iter().partition(|object| !in_hand_fan(object))
}
