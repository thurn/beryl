//! Casting a copy of a card during a resolution.
//!
//! Implements: none. Finds the definition a copy of a card is cast with (see CR 707.12).

use crate::card::CardDef;
use crate::layout::Layout;
use crate::state::{State, Zone};
use crate::table::GameTable;

use super::From;

/// The definition whose faces a cast of the object in `slot` chooses among: its card's, or
/// during a resolution, a copy of a card's (see CR 707.12): "An effect that instructs a player
/// to cast a copy of an object ... follows the rules for casting spells, except that the copy is
/// created in the same zone the object is in and then cast while another spell or ability is
/// resolving." A copy of a card is a token off the battlefield and the stack, which nothing
/// else lets a player cast.
#[inline]
pub(super) fn castable_def<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    from: From,
) -> Option<&'static CardDef> {
    s.card_def(t, slot)
        .or_else(|| card_copy_def(s, t, slot, from))
}

/// [`castable_def`] of a token: out of line, as only an effect that copies a card offers one.
#[cold]
#[inline(never)]
fn card_copy_def<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    from: From,
) -> Option<&'static CardDef> {
    let o = s.object(slot);
    (matches!(from, From::Resolution { .. }) && !o.on_battlefield() && o.zone() != Zone::Stack)
        .then(|| t.def(s.definition(slot).record(t).0))
}
