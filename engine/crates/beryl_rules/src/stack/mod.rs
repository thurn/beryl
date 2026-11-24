//! The stack ([CR 405](https://yawgatog.com/resources/magic-rules/#R405)): what is known of a
//! spell on it, and a spell leaving it without resolving. A spell has the characteristics
//! of its associated card ([CR 405.4]): "Each spell has all the characteristics of the card
//! associated with it."
//!
//! Implements: CR 405
//!
//! [CR 405.4]: https://yawgatog.com/resources/magic-rules/#R4054

pub mod delayed_triggers;
pub(crate) mod resolve;
pub(crate) mod trigger;

use crate::bitboard::Bitboard;
use crate::casting;
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::{Layout, Queue};
use crate::replacement::event::Reason;
use crate::state::{ObjectRef, StackEntry, Zone};
use crate::zones;

/// Takes the spells among `objects` off the stack, for an effect that moves their cards to
/// `to` without their resolving, and exiles those that are exiled instead
/// ([`crate::replacement::exiled_instead`]). Returns the objects still to move.
pub(crate) fn unstack<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
) -> Result<Bitboard<L::Slot>, Full> {
    if (objects & casting::spells(game.state)).is_empty() {
        return Ok(objects);
    }
    let flashback = remove_spells(game, objects, to)?;
    if !flashback.is_empty() {
        zones::move_all(game, flashback, Zone::Exile, Reason::Effect)?;
    }
    Ok(objects & !flashback)
}

/// Removes the spells in `objects` and returns the ones that go to exile instead, including
/// spells cast for their flashback cost (see [CR 702.34a]): "If the flashback cost was paid,
/// exile this card instead of putting it anywhere else any time it would leave the stack."
///
/// [CR 702.34a]: https://yawgatog.com/resources/magic-rules/#R70234a
#[cold]
#[inline(never)]
fn remove_spells<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
) -> Result<Bitboard<L::Slot>, Full> {
    let mut flashback = Bitboard::EMPTY;
    let mut spells: L::Queue<ObjectRef<L>, 128> = Default::default();
    // Removing a spell reports its current position on the stack. Visit from the top of
    // the stack down so unrelated slot numbers cannot change those events.
    for spell in game
        .state
        .stack()
        .iter()
        .rev()
        .filter_map(StackEntry::spell)
    {
        if objects.contains(spell.slot) {
            spells.push(spell)?;
        }
    }
    for &spell in spells.iter() {
        let slot = spell.slot;
        let Some(stack_spell) = game.state.spell(spell) else {
            continue;
        };
        let taken = (game.state)
            .take_off_stack(game.table, game.sink, stack_spell)?
            .is_some();
        if taken && crate::replacement::exiled_instead(game.state, spell, to) {
            flashback.insert(slot);
        }
    }
    Ok(flashback)
}
