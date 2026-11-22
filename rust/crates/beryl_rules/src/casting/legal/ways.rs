//! Casting permissions and other ways a player can cast a card at priority.
//!
//! Implements: none. Looks up ways to cast under the casting procedure.

use beryl_kernel::reference::Graveyard;

use crate::card::{Castings, Face, FaceIx};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{State, Zone};
use crate::table::GameTable;

use super::super::costs::{From, option_sets};
use super::super::start;

/// How `player` may cast one card now, apart from its face's own options: for free during a
/// resolution, under a flashback permission, under another permission or from their hand,
/// and with escape.
#[derive(Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each flag describes a separate condition checked when offering a cast"
)]
pub(super) struct Ways {
    /// Without escape, and so with or without the face's own alternative cost: from a hand or
    /// under a permission, or during a resolution.
    pub(super) plain: bool,
    /// With an alternative cost no face lists: free, or flashback's. Then the face's own
    /// alternative costs can't be paid (see CR 118.9a).
    pub(super) alternative: bool,
    /// Generic mana the casting permission adds to this face's cost.
    pub(super) increase: u8,
    /// With the escape an effect grants a card in a graveyard ([`Castings::ESCAPE`]).
    pub(super) escape: bool,
    /// The back needs a separate permission lookup when the front's permission came
    /// from resolving an Adventure (see [CR 716.3d]).
    ///
    /// [CR 716.3d]: https://yawgatog.com/resources/magic-rules/#R7163d
    pub(super) on_adventure: bool,
}

impl Ways {
    pub(super) fn of<L: Layout>(
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        slot: L::Slot,
        from: From,
    ) -> Ways {
        match from {
            From::Resolution { free } => Ways {
                plain: true,
                alternative: free,
                increase: 0,
                escape: false,
                on_adventure: false,
            },
            From::Priority => {
                let permission = start::permission(s, player, slot, FaceIx::FRONT);
                // See CR 702.138a: escape "functions while the card with escape is in a player's
                // graveyard", and lets its owner cast it from there.
                let escape = s.reaches_elsewhere() && {
                    let o = s.object(slot);
                    o.owner() == player
                        && s.card_in(s.object_ref(slot), Graveyard)
                            .is_some_and(|card| s.escape_from(t, card).is_some())
                };
                Ways {
                    plain: !escape || permission.is_some() || s.object(slot).zone() == Zone::Hand,
                    alternative: permission.is_some_and(|p| p.terms.is_flashback()),
                    increase: permission.map_or(0, |p| p.terms.increase),
                    escape,
                    on_adventure: permission.is_some_and(|p| p.terms.on_adventure),
                }
            }
        }
    }

    /// The permission from resolving the Adventure does not allow the Adventure face.
    #[cold]
    #[inline(never)]
    pub(super) fn back<L: Layout>(self, s: &State<L>, player: Seat, slot: L::Slot) -> Self {
        let permission = start::permission(s, player, slot, FaceIx::BACK);
        Self {
            plain: permission.is_some(),
            alternative: permission.is_some_and(|p| p.terms.is_flashback()),
            increase: permission.map_or(0, |p| p.terms.increase),
            ..self
        }
    }

    /// The sets of casting options of `face` these ways allow: each set the face's own
    /// options allow, and with escape, each of those that pays none of the face's own
    /// alternative costs, with [`Castings::ESCAPE`].
    pub(super) fn option_sets(self, face: &Face) -> impl Iterator<Item = Castings> + '_ {
        let plain = self.plain.then(|| option_sets(face, self.alternative));
        let escape = self
            .escape
            .then(|| option_sets(face, true).map(Castings::with_escape));
        plain
            .into_iter()
            .flatten()
            .chain(escape.into_iter().flatten())
    }
}
