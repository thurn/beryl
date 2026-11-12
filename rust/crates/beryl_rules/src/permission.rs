//! Permissions to cast: who may cast which card from a zone it could not otherwise be cast
//! from, and the terms a spell cast under one is cast on
//! ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to
//! cast a spell only if a rule or effect allows that player to cast it and no rule or effect
//! prohibits that player from casting it."). A permission is an entry of the Effects store
//! on the card (`beryl_kernel`'s contract, "Stores"), which a card body starts
//! (`EffectCtx::allow_cast`). Its terms become entries on the spell as it is cast.

use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{
    Affects, CostChange, Duration, MoveReplacement, ObjectRef, Payload, Permitted, State,
};

/// The permission that lets `player` cast the card in `slot`, if one does.
pub(crate) fn of<L: Layout>(s: &State<L>, player: Seat, slot: L::Slot) -> Option<Permitted<L>> {
    s.cast_permissions()
        .find(|p| p.card.slot == slot && p.terms.player == player)
}

/// Starts, on the spell now in `slot`, the terms of the permission it was cast under: its
/// cost increase
/// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "plus all additional
/// costs and cost increases") and the replacement of where it goes as it leaves the stack
/// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)). The permission was
/// about the card where it was and ended as it left
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)); the spell is a new
/// object, and these end with it.
pub(crate) fn begin_on_spell<L: Layout>(
    s: &mut State<L>,
    slot: L::Slot,
    permission: Permitted<L>,
) -> Result<(), Full> {
    let spell = Affects::object(s.object_ref(slot));
    let Permitted { terms, source, .. } = permission;
    if terms.increase > 0 {
        let cost = Payload::Cost(CostChange::Increase(terms.increase));
        s.start_effect(source, spell, cost, Duration::Indefinite)?;
    }
    if let Some(replacement) = terms.replacement {
        s.start_effect(
            source,
            spell,
            Payload::Move(replacement),
            Duration::Indefinite,
        )?;
    }
    Ok(())
}

/// Whether the spell was cast for a flashback cost: the replacement that paying one starts
/// is on it ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a): "If the
/// flashback cost was paid, exile this card instead of putting it anywhere else any time it
/// would leave the stack.").
pub fn flashback_paid<L: Layout>(s: &State<L>, spell: ObjectRef<L>) -> bool {
    s.move_replacements(spell)
        .any(|r| r == MoveReplacement::Flashback)
}
