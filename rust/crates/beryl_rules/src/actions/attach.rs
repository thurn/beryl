//! Attach ([CR 701.3](https://yawgatog.com/resources/magic-rules/#R7013)).

use beryl_card::Objects;

use crate::card::{Subtype, Types};
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::State;
use crate::table::GameTable;

/// Whether the Equipment in `equipment` may be attached to the permanent in `host`, both on
/// the battlefield.
///
/// [CR 301.5c](https://yawgatog.com/resources/magic-rules/#R3015c): "An Equipment that's also
/// a creature can't equip a creature unless that Equipment has reconfigure ... An Equipment
/// that loses the subtype 'Equipment' can't equip a creature. An Equipment can't equip
/// itself." No implemented card has reconfigure.
///
/// [CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b): "If an effect tries to
/// attach an object that isn't an Aura, Equipment, or Fortification to another object or
/// player, the effect does nothing and the first object doesn't move." Only an Equipment is
/// attached by an instruction: an Aura is attached as it enters.
pub(crate) fn may_equip<L: Layout>(game: &Game<'_, L>, equipment: L::Slot, host: L::Slot) -> bool {
    may_equip_in(game.state, game.table, equipment, host)
}

/// [`may_equip`], in a state and its table.
pub(crate) fn may_equip_in<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    equipment: L::Slot,
    host: L::Slot,
) -> bool {
    let (gear, creature) = (s.derived(equipment), s.derived(host));
    let is_equipment = t.subtypes().has(Subtype::Equipment).of(gear.subtypes);
    is_equipment
        && equipment != host
        && !gear.types.contains(Types::CREATURE)
        && creature.types.contains(Types::CREATURE)
}

/// [`Effect::Attach`](beryl_card::Effect::Attach): "Attach [an Equipment] to [a creature]."
/// The outcome is the Equipment, if it moved.
///
/// [CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b): "If an effect tries to
/// attach an Aura, Equipment, or Fortification to an object or player it can't be attached
/// to, the Aura, Equipment, or Fortification doesn't move. If an effect tries to attach an
/// Aura, Equipment, or Fortification to the object or player it's already attached to, the
/// effect does nothing." An Equipment or a creature that has left the battlefield is not
/// there to attach (CR 400.7), and an Equipment can't equip more than one creature (CR
/// 301.5c), so the instruction moves at most one Equipment onto one creature.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    to: Objects,
) -> Outcome<L> {
    let (Some(equipment), Some(host)) = (run.permanents(objects).sole(), run.permanents(to).sole())
    else {
        return Outcome::NOTHING;
    };
    let game = run.game();
    if game.state.object(equipment).attached_to() == Some(host) || !may_equip(game, equipment, host)
    {
        return Outcome::NOTHING;
    }
    game.state.attach(equipment, host);
    Outcome::acted_on(crate::bitboard::Bitboard::single(equipment))
}
