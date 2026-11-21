//! Counter ([CR 701.6](https://yawgatog.com/resources/magic-rules/#R7016)).

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::casting;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::rule_change;
use crate::state::{ObjectRef, StackEntry, State, Zone};

/// Counters a spell ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)):
/// "To counter a spell or ability means to cancel it, removing it from the stack. It
/// doesn't resolve and none of its effects occur. A countered spell is put into its
/// owner's graveyard." Or into `to`, where the effect that counters it says so, and a
/// replacement on the spell may still exile it instead. Returns whether it was countered: a
/// spell no longer on the stack is not, nor is one that can't be countered
/// ([`rule_change::cant_be_countered`]), which stays where it is.
pub fn counter<L: Layout>(
    game: &mut Game<'_, L>,
    spell: ObjectRef<L>,
    to: Zone,
) -> Result<bool, Full> {
    if rule_change::cant_be_countered(game.state, game.table, spell) {
        return Ok(false);
    }
    let Some(spell) = game.state.spell(spell) else {
        return Ok(false);
    };
    let Some(entry) = game.state.take_off_stack(game.table, game.sink, spell)? else {
        return Ok(false);
    };
    let reason = Reason::Counter;
    casting::leave_stack(game.state, game.table, game.sink, &entry, to, reason)?;
    Ok(true)
}

/// Counters the activated or triggered ability at `place` on the stack (CR 701.6a). Its
/// source is untouched, and a delayed triggered ability countered this way does not wait
/// again: it left the list of delayed triggers as it triggered
/// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)): "A delayed triggered
/// ability will trigger only once—the next time its trigger event occurs—unless it has a
/// stated duration". Returns whether it was countered.
pub(crate) fn counter_ability<L: Layout>(game: &mut Game<'_, L>, place: usize) -> bool {
    game.state
        .take_ability_off_stack(game.sink, place)
        .is_some()
}

/// The topmost entry on the stack among the spells `spells` and the abilities at the places
/// `abilities`: its place, and its spell if it is one.
fn topmost<L: Layout>(
    s: &State<L>,
    spells: Bitboard<L::Slot>,
    abilities: u64,
) -> Option<(usize, Option<ObjectRef<L>>)> {
    let named = |place: usize, entry: &StackEntry<L>| match entry.spell() {
        Some(spell) => spells.contains(spell.slot),
        None => place < 64 && abilities & (1 << place) != 0,
    };
    let stack = s.stack().iter().enumerate().rev();
    stack
        .filter(|&(place, entry)| named(place, entry))
        .map(|(place, entry)| (place, entry.spell()))
        .next()
}

/// [`Effect::Counter`](beryl_card::Effect::Counter): "Counter [spell or ability]" (CR 701.6a).
/// Of several, the topmost first. The outcome is the spells countered.
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
    let mut abilities = run.scene().abilities(objects);
    run.remember(spells)?;
    let mut countered = Bitboard::EMPTY;
    // The topmost first: an entry leaving moves the places of those above it, and none of
    // those still to counter is above it.
    while let Some((place, spell)) = topmost(run.state(), spells, abilities) {
        if let Some(spell) = spell {
            spells.remove(spell.slot);
            if counter(run.game(), spell, zone)? {
                countered.insert(spell.slot);
            }
        } else {
            abilities &= !(1 << place);
            counter_ability(run.game(), place);
        }
    }
    Ok(Outcome::acted_on(countered))
}
