//! What an Aura enchants when it enters the battlefield other than as a resolving Aura spell
//! ([CR 303.4f]): "If an Aura is entering the battlefield under a player's control by any means
//! other than by resolving as an Aura spell, and the effect putting it onto the battlefield
//! doesn't specify the object or player the Aura will enchant, that player chooses what it will
//! enchant as the Aura enters the battlefield. The player must choose a legal object or player
//! according to the Aura's enchant ability and any other applicable effects."
//!
//! [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f

use beryl_card::Prompt;
use beryl_card::prompt::Purpose;

use super::{answer, selected_copy};
use crate::actions::attach;
use crate::bitboard::Bitboard;
use crate::card::{Subtype, Words};
use crate::decision::{Decision, DecisionKind};
use crate::event::{Entry, Sink};
use crate::fixed::Full;
use crate::layers;
use crate::layout::Layout;
use crate::state::{State, Zone};
use crate::table::GameTable;

/// What an object entering the battlefield as `entry` says, attached to nothing, enchants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Enchants<S> {
    /// It is no Aura.
    NotAnAura,
    /// The object its controller chose.
    Object(S),
    /// Nothing is legal for it to enchant, so it does not enter ([CR 303.4g]): "If an Aura is
    /// entering the battlefield and there is no legal object or player for it to enchant, the
    /// Aura remains in its current zone, unless that zone is the stack. In that case, the Aura
    /// is put into its owner's graveyard instead of entering the battlefield."
    ///
    /// [CR 303.4g]: https://yawgatog.com/resources/magic-rules/#R3034g
    Nothing,
}

/// What the object in `slot`, entering the battlefield as `entry` says with nothing it is to
/// enchant, enchants ([CR 303.4f]): its controller chooses, through `sink`'s record of answers,
/// an object its enchant abilities allow on the battlefield or in a graveyard
/// ([`attach::may_enchant`]). Whether it is an Aura, and what it may enchant, are asked of it
/// as it would exist on the battlefield ([`layers::entered`]): a new object, which no ability
/// of its own put anything onto the battlefield with. It is not a target, so a creature with
/// shroud may be chosen.
///
/// Only an object whose face up is an Aura's is asked of the copy of the state that takes, so
/// another permanent entering costs one test of its face.
///
/// [`Full`] if the copy ran out of room, or as [`answer`] gives it.
///
/// [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f
#[cold]
#[inline(never)]
pub(crate) fn enchants<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Enchants<L::Slot>, Full> {
    let copied = selected_copy(sink, slot);
    let values = copied.unwrap_or_else(|| s.copiable(slot).turned(entry.face));
    let face = values.face(t);
    if !face.subtypes.read(&Words::NONE).contains(Subtype::Aura) {
        return Ok(Enchants::NotAnAura);
    }
    let found = layers::entered(s, t, slot, entry, copied, |copy| {
        if !t
            .subtypes()
            .has(Subtype::Aura)
            .of(copy.derived(slot).subtypes)
        {
            return None;
        }
        let mut within: Bitboard<L::Slot> = copy.in_zone(Zone::Battlefield);
        within |= copy.in_zone(Zone::Graveyard);
        within.remove(slot);
        Some(within.filter(|host| attach::may_enchant(copy, t, slot, host)))
    })?;
    let Some(candidates) = found else {
        return Ok(Enchants::NotAnAura);
    };
    if candidates.is_empty() {
        return Ok(Enchants::Nothing);
    }
    let controller = entry.controller.unwrap_or(s.object(slot).owner());
    let prompt = Prompt::new(Purpose::Choose).most(1);
    let d = Decision::objects(controller, DecisionKind::ChooseObject, candidates, prompt);
    let c = answer(s, sink, &d)?;
    Ok(d.candidates
        .object(c)
        .map_or(Enchants::Nothing, Enchants::Object))
}
