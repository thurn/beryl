//! Linked abilities ([CR 607](https://yawgatog.com/resources/magic-rules/#R607)): the cards
//! one ability of an object exiles, which another of its abilities refers to.

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{Memo, ObjectRef, State, Zone};
use crate::zones;

/// Exiles a card, linked to `source`, the object whose ability exiles it, so that the
/// source's other abilities can find it among the cards "exiled with" it
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "If an object has an
/// activated or triggered ability printed on it that instructs a player to exile one or
/// more cards and an ability printed on it that refers either to "the exiled cards" or to
/// cards "exiled with [this object]," these abilities are linked." A source that has left
/// its zone since is gone, and so is the link: nothing can refer to it any more.
pub fn exile_linked<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    source: ObjectRef<L>,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    zones::move_to(game, object, Zone::Exile, Reason::Exile)?;
    if game.state.is_current(source) && game.state.object(object.slot).zone() == Zone::Exile {
        (game.state).remember(object.slot, Memo::ExiledWith(source.slot))?;
    }
    Ok(())
}

/// The cards exiled with `source` by [`exile_linked`] (CR 607.2a), whether or not the source
/// is still there: an ability of an object that has left the battlefield still refers to the
/// cards that object exiled.
pub fn exiled_with<L: Layout>(s: &State<L>, source: ObjectRef<L>) -> Bitboard<L::Slot> {
    s.exiled_with(source.slot)
}

/// [`Effect::ExileLinked`](beryl_card::Effect::ExileLinked): "Exile [cards]", linked to the
/// object the ability is on (CR 607.2a): [`exile_linked`], each in the order the game lists
/// them. The outcome is the cards that are in exile.
///
/// # Panics
///
/// For an emblem's ability, which has no source.
pub(crate) fn exile_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    let cards = run.elsewhere_than(cards, Zone::Exile);
    run.remember(cards);
    let source = run.resolution().source();
    for slot in run.state().in_candidate_order(cards) {
        let card = run.state().object_ref(slot);
        exile_linked(run.game(), card, source)?;
    }
    Ok(run.now_in(cards, Zone::Exile))
}
