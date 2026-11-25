//! A spell's card leaving the stack, as the spell resolves or without its resolving.
//!
//! Implements: none. Delegates spell movement to the event pipeline.

use beryl_kernel::reference::Spell;

use crate::fixed::Full;
use crate::layout::Layout;
use crate::replacement::event::{self, Reason, Sink};
use crate::state::{StackEntry, State, Zone};
use crate::table::GameTable;

/// Takes a spell off the stack for an effect that moves its card to `to` without its
/// resolving: where the card goes ([`crate::replacement::exiled_instead`]), or nothing if the
/// spell is not on the stack. [`Full`] if what an ability that copies the spell needs of it
/// finds no room ([`State::take_off_stack`]).
pub(crate) fn unstack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    spell: Spell<L>,
    to: Zone,
) -> Result<Option<Zone>, Full> {
    if s.take_off_stack(t, sink, spell)?.is_none() {
        return Ok(None);
    }
    let exiled = crate::replacement::exiled_instead(s, spell, to);
    Ok(Some(if exiled { Zone::Exile } else { to }))
}

/// Moves the card of a spell whose entry is off the stack, and which did not become a
/// permanent, to `to`, or to exile instead: the move applies the replacements on the spell
/// ([`crate::replacement::exiled_instead`]).
pub(crate) fn leave_stack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    entry: &StackEntry<L>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    let spell = entry.spell().expect("a spell leaving the stack");
    event::move_one(s, t, sink, spell.slot, to, reason)
}
