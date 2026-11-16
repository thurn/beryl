//! The stack ([CR 405](https://yawgatog.com/resources/magic-rules/#R405)): what is known of a
//! spell on it, and a spell leaving it without resolving.

use crate::bitboard::Bitboard;
use crate::cast;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{ObjectRef, StackEntry, State, Zone};
use crate::table::GameTable;
use crate::zones;

fn spell_entry<L: Layout>(s: &State<L>, spell: ObjectRef<L>) -> Option<&StackEntry<L>> {
    s.stack().iter().find(|e| e.spell() == Some(spell))
}

/// The controller of a spell on the stack
/// ([CR 112.2](https://yawgatog.com/resources/magic-rules/#R1122)): "A spell's controller
/// is, by default, the player who put it on the stack."
pub fn spell_controller<L: Layout>(s: &State<L>, spell: ObjectRef<L>) -> Option<Seat> {
    spell_entry(s, spell).map(StackEntry::controller)
}

/// The mana value of a spell on the stack, X included
/// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as the
/// number chosen for it while the object is on the stack."
pub fn spell_mana_value<L: Layout>(s: &State<L>, t: &GameTable, spell: ObjectRef<L>) -> Option<u8> {
    let e = spell_entry(s, spell)?;
    let cost = t.def(e.card()).face(e.face()).cost;
    Some(cost.mana_value().saturating_add(cost.x.saturating_mul(e.x)))
}

/// Takes the spells among `objects` off the stack, for an effect that moves their cards to
/// `to` without their resolving, and exiles those that are exiled instead
/// ([`crate::replace::exiled_instead`]). Returns the objects still to move.
pub(crate) fn unstack<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
) -> Result<Bitboard<L::Slot>, Full> {
    if (objects & cast::spells(game.state)).is_empty() {
        return Ok(objects);
    }
    let flashback = remove_spells(game, objects, to)?;
    if !flashback.is_empty() {
        zones::move_all(game, flashback, Zone::Exile, Reason::Effect)?;
    }
    Ok(objects & !flashback)
}

/// Removes the spells in `objects` and returns the ones that go to exile instead. For
/// example, [CR 702.34a] says "If the flashback cost was paid, exile this card instead of
/// putting it anywhere else any time it would leave the stack."
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
        let taken = (game.state)
            .take_off_stack(game.table, game.sink, spell)?
            .is_some();
        if taken && crate::replace::exiled_instead(game.state, spell, to) {
            flashback.insert(slot);
        }
    }
    Ok(flashback)
}
