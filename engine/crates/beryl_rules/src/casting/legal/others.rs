//! The abilities of a permanent other than those of its face up that can be activated: those
//! other objects granted it, and those of the cards under it if it is a merged permanent.
//!
//! Implements: none. Lists the abilities the activation options read besides a face's own.

use crate::card::{AbilityIx, ObjectAbility};
use crate::layout::Layout;
use crate::state::State;
use crate::table::GameTable;

/// The abilities other objects granted the object in `slot`, and those of the cards under it
/// if it is a merged permanent (see [CR 702.140e]), if `any` card of the game grants one or merges
/// ([`crate::table::Capability::Grants`], [`crate::table::Capability::Merges`]).
///
/// Two effects that grant the same ability give the permanent two instances of it (see
/// [CR 113.2c]): "If an object has multiple instances of the same ability, each
/// instance functions independently." Activating one is activating the same ability as the other,
/// so the two are one candidate, the first granted ([`first_instance`]).
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
/// [CR 113.2c]: https://yawgatog.com/resources/magic-rules/#R1132c
pub(super) fn granted<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
    slot: L::Slot,
    any: bool,
) -> impl Iterator<Item = ObjectAbility> + 'a {
    any.then(|| s.abilities(t, slot))
        .into_iter()
        .flatten()
        .map(|(ability, _)| ability)
        .filter(move |&ability| match ability {
            ObjectAbility::Granted(k) => first_instance(s, slot, k),
            ObjectAbility::Merged(..) | ObjectAbility::Lent(..) => true,
            ObjectAbility::Own(_) => false,
        })
}

/// Whether the object in `slot`'s `k`th granted ability is the first it was granted of the
/// abilities the same card's same grant gives ([`granted`]).
#[inline(never)]
fn first_instance<L: Layout>(s: &State<L>, slot: L::Slot, k: u8) -> bool {
    let grant = s.granted_ability(slot, k);
    (0..k).all(|j| s.granted_ability(slot, j) != grant)
}

/// Whether the object in `slot` has an ability other objects granted it, or one of a card
/// under it as a merged permanent. Out of line: only a game with a card that grants an
/// ability or merges asks.
#[inline(never)]
pub(super) fn has_others<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    s.has_any_grant(slot) || s.has(slot, ObjectAbility::Merged(0, AbilityIx::new(0)))
}
