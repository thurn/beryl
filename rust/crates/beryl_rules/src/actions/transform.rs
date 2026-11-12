//! Transform ([CR 701.27](https://yawgatog.com/resources/magic-rules/#R70127)), and a card
//! put onto the battlefield transformed
//! ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)).

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
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
    let o = *game.state.object(object.slot);
    if o.zone() != Zone::Exile {
        return Ok(());
    }
    let (def, _) = game.state.copiable(object.slot).record(game.table);
    if game.table.def(def).faces.len() < 2 {
        return Ok(());
    }
    let entry = Entry {
        controller: Some(o.owner()),
        face: FaceIx::BACK,
        ..Entry::default()
    };
    let (card, to) = (Bitboard::single(object.slot), Zone::Battlefield);
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
