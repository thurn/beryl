//! Cards put onto the battlefield transformed
//! ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)).
//!
//! Implements: CR 712.11a, CR 712.11b

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Types};
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Entry, Reason};
use crate::state::{ObjectRef, Zone};
use crate::zones;

/// Exiles a permanent, then returns it to the battlefield transformed under its owner's
/// control. It returns as a new object
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) with its back face up
/// ([`return_transformed`]). If it already left the battlefield, it isn't exiled, so it
/// doesn't return.
pub fn exile_and_return_transformed<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
) -> Result<(), Full> {
    if game.state.permanent(object).is_none() {
        return Ok(());
    }
    zones::move_to(game, object, Zone::Exile, Reason::Exile)?;
    if game.state.object(object.slot).zone() != Zone::Exile {
        return Ok(());
    }
    return_transformed(game, game.state.object_ref(object.slot))
}

/// Returns a card to the battlefield transformed under its owner's control
/// ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)): "If a spell or
/// ability puts a double-faced card onto the battlefield 'transformed' or 'converted,' it
/// enters the battlefield with its back face up. If a player is instructed to put a card
/// that isn't a double-faced card onto the battlefield transformed or converted, that card
/// stays in its current zone." It returns as a new object, and one that has changed zones
/// since it was named does not move
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
pub fn return_transformed<L: Layout>(
    game: &mut Game<'_, L>,
    card: ObjectRef<L>,
) -> Result<(), Full> {
    if !game.state.is_current(card) {
        return Ok(());
    }
    let owner = game.state.object(card.slot).owner();
    let values = game.state.copiable(card.slot);
    if !values.shape(game.table).is_double_faced() {
        return Ok(());
    }
    // CR 712.11b: "If a player is instructed to put a double-faced card onto the
    // battlefield with an instant or sorcery face up, that card stays in its current zone."
    // https://yawgatog.com/resources/magic-rules/#R71211b
    if values
        .turned(game.table, FaceIx::BACK)
        .face(game.table)
        .types
        .intersects(Types::INSTANT.union(Types::SORCERY))
    {
        return Ok(());
    }
    let entry = Entry {
        controller: Some(owner),
        face: FaceIx::BACK,
        ..Entry::default()
    };
    let (card, to) = (Bitboard::single(card.slot), Zone::Battlefield);
    event::move_batch(
        game.state,
        game.table,
        game.sink,
        card,
        to,
        Reason::Effect,
        entry,
    )
}

/// [`Effect::ReturnTransformed`](beryl_card::Effect::ReturnTransformed): "Return [cards] to
/// the battlefield transformed under their owners' control" (CR 712.11a):
/// [`return_transformed`], each in the order the game lists them. The outcome is the
/// permanents they are.
pub(crate) fn return_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    let cards = run.elsewhere_than(cards, Zone::Battlefield);
    for &slot in run.in_game_order(cards)?.iter() {
        let card = run.state().object_ref(slot);
        return_transformed(run.game(), card)?;
    }
    Ok(run.now_in(cards, Zone::Battlefield))
}
