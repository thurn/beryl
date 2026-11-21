//! Offering to cast a card during a resolution (see CR 608.2g), and a spell leaving the stack
//! without resolving.
//!
//! Implements: none. Offers legal casts during resolution and delegates spell movement to the event pipeline.

use beryl_kernel::reference::Spell;

use crate::bitboard::Bitboard;
use crate::decision::{OPTIONS, Opt};
use crate::event::{self, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{ObjectRef, StackEntry, State, Zone};
use crate::table::GameTable;

use super::costs::From;
use super::legal::{Situation, cast_options};

/// The options of a resolution's offer to cast a card (see CR 608.2g): not casting it, then each
/// way of casting it.
pub(crate) fn offer_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    object: ObjectRef<L>,
    player: Seat,
    free: bool,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    out.push(Opt::Pass)?;
    if s.is_current(object) {
        let mut cards = Bitboard::EMPTY;
        cards.insert(object.slot);
        let now = Situation::of(player);
        cast_options(s, t, &now, cards, From::Resolution { free }, &mut out)?;
        now.complete()?;
    }
    Ok(out)
}

/// Takes a spell off the stack for an effect that moves its card to `to` without its
/// resolving: where the card goes ([`crate::replace::exiled_instead`]), or nothing if the
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
    let exiled = crate::replace::exiled_instead(s, spell.into(), to);
    Ok(Some(if exiled { Zone::Exile } else { to }))
}

/// Moves the card of a spell whose entry is off the stack, and which did not become a
/// permanent, to `to`, or to exile instead: the move applies the replacements on the spell
/// ([`crate::replace::exiled_instead`]).
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
