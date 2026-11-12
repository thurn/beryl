//! Transform ([CR 701.27](https://yawgatog.com/resources/magic-rules/#R70127)), and a card
//! put onto the battlefield transformed
//! ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)).

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{ObjectRef, Zone};
use crate::zones;

/// Exiles a permanent, then returns it to the battlefield transformed under its owner's
/// control. It returns as a new object
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) with its back face up
/// (CR 712.11a): "If a spell or ability puts a double-faced card onto the battlefield
/// 'transformed' or 'converted,' it enters the battlefield with its back face up. If a
/// player is instructed to put a card that isn't a double-faced card onto the battlefield
/// transformed or converted, that card stays in its current zone." If it already left the
/// battlefield, it isn't exiled, so it doesn't return.
pub fn exile_and_return_transformed<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
) -> Result<(), Full> {
    if !game.on_battlefield(object) {
        return Ok(());
    }
    zones::move_to(game, object, Zone::Exile, Reason::Exile)?;
    if game.state.object(object.slot).zone() != Zone::Exile {
        return Ok(());
    }
    return_transformed(game, game.state.object_ref(object.slot))
}

/// Returns a card to the battlefield transformed under its owner's control (CR 712.11a): a
/// double-faced card "enters the battlefield with its back face up", and one that isn't
/// "stays in its current zone." One that has changed zones since does not move
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
pub fn return_transformed<L: Layout>(
    game: &mut Game<'_, L>,
    card: ObjectRef<L>,
) -> Result<(), Full> {
    if !game.state.is_current(card) {
        return Ok(());
    }
    let owner = game.state.object(card.slot).owner();
    let (def, _) = game.state.copiable(card.slot).record(game.table);
    if game.table.def(def).faces.len() < 2 {
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
    for slot in run.state().in_candidate_order(cards) {
        let card = run.state().object_ref(slot);
        return_transformed(run.game(), card)?;
    }
    Ok(run.now_in(cards, Zone::Battlefield))
}
