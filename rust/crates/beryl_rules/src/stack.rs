//! The stack ([CR 405](https://yawgatog.com/resources/magic-rules/#R405)): what is known of a
//! spell on it, and a spell leaving it without resolving.

use crate::bitboard::Bitboard;
use crate::cast;
use crate::event::{self, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
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
    spell_entry(s, spell).map(|e| e.controller)
}

/// The mana value of a spell on the stack, X included
/// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as the
/// number chosen for it while the object is on the stack."
pub fn spell_mana_value<L: Layout>(s: &State<L>, t: &GameTable, spell: ObjectRef<L>) -> Option<u8> {
    let e = spell_entry(s, spell)?;
    let cost = t.def(e.card).face(e.face).cost;
    Some(cost.mana_value().saturating_add(cost.x.saturating_mul(e.x)))
}

/// Takes the spells among `objects` off the stack, for an effect that moves their cards to
/// `to` without their resolving, and exiles those that are exiled instead
/// ([`event::exiled_instead`]). Returns the objects still to move.
pub(crate) fn unstack<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
) -> Result<Bitboard<L::Slot>, Full> {
    let mut flashback = Bitboard::EMPTY;
    let spells = objects & cast::spells(game.state);
    // Each spell's entry comes off on its own, with no event, and the spells left on the
    // stack keep their order, so the order these come off in is nothing.
    for slot in game.state.in_candidate_order(spells) {
        let spell = game.state.object_ref(slot);
        let taken = game.state.take_off_stack(spell).is_some();
        if taken && event::exiled_instead(game.state, spell, to) {
            flashback.insert(slot);
        }
    }
    if !flashback.is_empty() {
        zones::move_all(game, flashback, Zone::Exile, Reason::Effect)?;
    }
    Ok(objects & !flashback)
}
