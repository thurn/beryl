//! Counter ([CR 701.6](https://yawgatog.com/resources/magic-rules/#R7016)).

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::cast;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{ObjectRef, StackEntry, State, Zone};

/// Counters a spell ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)):
/// "To counter a spell or ability means to cancel it, removing it from the stack. It
/// doesn't resolve and none of its effects occur. A countered spell is put into its
/// owner's graveyard." Or into `to`, where the effect that counters it says so, and a
/// replacement on the spell may still exile it instead. Returns whether it was countered: a
/// spell no longer on the stack is not.
pub fn counter<L: Layout>(
    game: &mut Game<'_, L>,
    spell: ObjectRef<L>,
    to: Zone,
) -> Result<bool, Full> {
    let Some(entry) = game.state.take_off_stack(spell) else {
        return Ok(false);
    };
    let reason = Reason::Counter;
    cast::leave_stack(game.state, game.table, game.sink, &entry, to, reason)?;
    Ok(true)
}

/// The topmost spell on the stack among `spells`.
fn top_spell<L: Layout>(s: &State<L>, spells: Bitboard<L::Slot>) -> Option<ObjectRef<L>> {
    let stack = s.stack().iter().rev();
    stack
        .filter_map(StackEntry::spell)
        .find(|spell| spells.contains(spell.slot))
}

/// [`Effect::Counter`](beryl_card::Effect::Counter): "Counter [spell]" (CR 701.6a). Of
/// several, the topmost first. The outcome is the spells countered.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    into_instruction(run, objects, Zone::Graveyard)
}

/// [`Effect::CounterInto`](beryl_card::Effect::CounterInto): "Counter [spell]. If that spell
/// is countered this way, [put it into `zone`] instead of putting it into its owner's
/// graveyard" ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c): "later text
/// on the card may modify the meaning of earlier text"). Of several, the topmost first. The
/// outcome is the spells countered.
pub(crate) fn into_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    zone: Zone,
) -> Result<Outcome<L>, Full> {
    let mut spells = run.scene().objects(objects);
    run.remember(spells)?;
    let mut countered = Bitboard::EMPTY;
    while let Some(spell) = top_spell(run.state(), spells) {
        spells.remove(spell.slot);
        if counter(run.game(), spell, zone)? {
            countered.insert(spell.slot);
        }
    }
    Ok(Outcome::acted_on(countered))
}
