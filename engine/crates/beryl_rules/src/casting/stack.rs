//! Offering to cast a card during a resolution (see CR 608.2g), and a spell leaving the stack
//! without resolving.
//!
//! Implements: none. Offers legal casts during resolution and delegates spell movement to the event pipeline.

use beryl_kernel::reference::Spell;

use crate::bitboard::Bitboard;
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Reason, Sink};
use crate::state::{StackEntry, State, Zone};
use crate::table::GameTable;

use super::costs::From;
use super::legal::{Situation, cast_options};

/// The options of a resolution's offer to cast a card (see CR 608.2g): not casting, then
/// each way of casting one of the cards.
pub(super) fn offer_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    cards: Bitboard<L::Slot>,
    player: Seat,
    free: bool,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    out.push(Opt::Pass)?;
    let now = Situation::of(player);
    cast_options::<L, true, false>(s, t, &now, cards, From::Resolution { free }, &mut out)?;
    now.complete()?;
    Ok(out)
}

/// Appends the payable casts whose faces allow casting from the library while searching it
/// (see CR 608.2g). The search supplies the cards in the player's own library.
pub(crate) fn search_cast_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    library: Bitboard<L::Slot>,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let now = Situation::of(player);
    cast_options::<L, true, true>(s, t, &now, library, From::Resolution { free: false }, out)?;
    now.complete()
}

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
