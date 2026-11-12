//! Casting spells ([CR 601](https://yawgatog.com/resources/magic-rules/#R601)) as an effect
//! allows: at once, as the effect finishes resolving, or later, by a permission
//! ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to
//! cast a spell only if a rule or effect allows that player to cast it and no rule or effect
//! prohibits that player from casting it."). A permission is an entry of the Effects store
//! on the card (`beryl_kernel`'s contract, "Stores"), and its terms become entries on the
//! spell as it is cast.

use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{
    Continuous, CostChange, Duration, MoveReplacement, ObjectRef, Permission, Permitted, State,
};

/// Lets `player` cast the card `object` as an effect of what is resolving, without paying
/// its mana cost when `free`
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin to
/// cast a spell only if a rule or effect allows that player to cast it". The effect's last
/// action: the card is offered as the resolution finishes, which is when a spell cast
/// during it would otherwise have mattered, since "no player receives priority after it's
/// cast" ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)). The engine asks
/// then whether, and how, the player casts it.
pub fn offer<L: Layout>(
    resolution: &mut Resolution<'_, L>,
    object: ObjectRef<L>,
    player: Seat,
    free: bool,
) {
    resolution.offer = Some((object, player, free));
}

/// Lets `player` cast the card in `object`'s slot from the zone it is in, for as long as
/// it stays there, or until end of turn with `this_turn`
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin
/// to cast a spell only if a rule or effect allows that player to cast it". A spell cast
/// this way costs `extra` generic mana more, and with `exile` it is exiled rather than
/// put into its owner's graveyard.
pub fn allow<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &Resolution<'_, L>,
    object: ObjectRef<L>,
    player: Seat,
    extra: u8,
    this_turn: bool,
    exile: bool,
) -> Result<(), Full> {
    let terms = Permission {
        player,
        increase: extra,
        replacement: exile.then_some(MoveReplacement::ExileInsteadOfGraveyard),
    };
    // It lasts while the card stays where it is, which is no duration: the entry ends
    // with the card's object.
    let duration = if this_turn {
        Duration::EndOfTurn
    } else {
        Duration::Indefinite
    };
    permit(game, resolution, object, terms, duration)
}

/// Starts a permission of what is resolving to cast the card `object`, if it is still that
/// object.
pub(crate) fn permit<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &Resolution<'_, L>,
    object: ObjectRef<L>,
    terms: Permission,
    duration: Duration,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    let source = resolution.entry.card;
    (game.state).start_effect(source, Continuous::Cast(object, terms), duration)
}

/// The permission that lets `player` cast the card in `slot`, if one does.
pub(crate) fn permission<L: Layout>(
    s: &State<L>,
    player: Seat,
    slot: L::Slot,
) -> Option<Permitted<L>> {
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
    let spell = s.object_ref(slot);
    let Permitted { terms, source, .. } = permission;
    if terms.increase > 0 {
        let cost = Continuous::Cost(spell, CostChange::Increase(terms.increase));
        s.start_effect(source, cost, Duration::Indefinite)?;
    }
    if let Some(replacement) = terms.replacement {
        let goes = Continuous::Move(spell, replacement);
        s.start_effect(source, goes, Duration::Indefinite)?;
    }
    Ok(())
}
