//! Offering to cast a card during a resolution (CR 608.2g), and a spell leaving the stack
//! without resolving.

use crate::bitboard::Bitboard;
use crate::decision::Opt;
use crate::event::{self, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{ObjectRef, StackEntry, State, Zone};
use crate::table::GameTable;

use super::costs::From;
use super::legal::{Situation, cast_options};

/// The options of a resolution's offer to cast a card (CR 608.2g): not casting it, then each
/// way of casting it.
pub(crate) fn offer_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    object: ObjectRef<L>,
    player: Seat,
    free: bool,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
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

/// Removes a spell from the stack to its owner's graveyard, or to exile when `exile` or its
/// entry says so. Returns whether it was there.
pub(crate) fn counter<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    spell: ObjectRef<L>,
    exile: bool,
) -> Result<bool, Full> {
    let Some(entry) = s.take_off_stack(spell) else {
        return Ok(false);
    };
    let to = if exile { Zone::Exile } else { Zone::Graveyard };
    leave_stack(s, t, sink, &entry, to, Reason::Counter)?;
    Ok(true)
}

/// Takes a spell off the stack for an effect that moves its card to `to` without its
/// resolving: where the card goes ([`event::exiled_instead`]), or nothing if the spell is
/// not on the stack.
pub(crate) fn unstack<L: Layout>(s: &mut State<L>, spell: ObjectRef<L>, to: Zone) -> Option<Zone> {
    s.take_off_stack(spell)?;
    let exiled = event::exiled_instead(s, spell, to);
    Some(if exiled { Zone::Exile } else { to })
}

/// Moves the card of a spell whose entry is off the stack, and which did not become a
/// permanent, to `to`, or to exile instead: the move applies the replacements on the spell
/// ([`event::exiled_instead`]).
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
