//! Cards put onto the battlefield transformed
//! ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)).
//!
//! Implements: CR 701.27a, CR 701.27c, CR 701.27d, CR 701.27e, CR 701.27g, CR 712.11a, CR 712.11b

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Types};
use crate::continuous::layers;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Entry, Reason};
use crate::state::{ObjectRef, Zone};
use crate::zones;
use beryl_card::definition::Shape;
use beryl_kernel::reference::Permanent;

/// Turns a permanent to its other face ([CR 701.27a]): "To transform a permanent, turn
/// it over so that its other face is up." A single-faced card that copies a double-faced
/// card cannot transform ([CR 701.27d]): "If a spell or ability instructs a player to
/// transform a permanent that isn’t represented by a double-faced token or a double-faced
/// card, nothing happens." A face-down permanent cannot transform (see CR 712.12a).
///
/// [CR 701.27a]: https://yawgatog.com/resources/magic-rules/#R70127a
/// [CR 701.27d]: https://yawgatog.com/resources/magic-rules/#R70127d
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face a permanent would transform into (see CR 701.27e)"
)]
fn other_face<L: Layout>(game: &Game<'_, L>, permanent: Permanent<L>) -> Option<FaceIx> {
    let slot = permanent.slot();
    let printed = game.table.def(game.state.card_really_is(slot));
    let visible = game.state.copiable(slot);
    if printed.shape != Shape::Transforming || visible.shape(game.table) != Shape::Transforming {
        return None;
    }
    let face = match game.state.object(slot).face() {
        FaceIx::FRONT => FaceIx::BACK,
        FaceIx::BACK => FaceIx::FRONT,
        _ => return None,
    };
    // CR 701.27e: "If a spell or ability instructs a player to transform a permanent,
    // and the face that permanent would transform into is an instant or sorcery face,
    // nothing happens." https://yawgatog.com/resources/magic-rules/#R70127e
    (!printed
        .face(face)
        .types
        .intersects(Types::INSTANT.union(Types::SORCERY)))
    .then_some(face)
}

/// Carries out a transform and refreshes the layers before the next instruction reads
/// characteristics. The affected object stays the same (CR 701.27c).
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let mut affected = Bitboard::EMPTY;
    let objects = run.scene().objects(objects);
    for &slot in run.in_game_order(objects)?.iter() {
        let (game, resolution) = run.resolving();
        let Some(permanent) = game.state.permanent_at(slot) else {
            continue;
        };
        let own_source = resolution.entry.source() == Some(permanent.into());
        // CR 701.27g: "In both cases, if the permanent has already transformed or
        // converted, an instruction to do either is ignored."
        // https://yawgatog.com/resources/magic-rules/#R70127g
        if own_source && resolution.entry.source_has_transformed() {
            continue;
        }
        if let Some(face) = other_face(game, permanent) {
            game.state.transform_permanent(game.table, permanent, face);
            if own_source {
                resolution.entry.mark_source_transformed();
            }
            affected.insert(slot);
        }
    }
    if !affected.is_empty() {
        let game = run.game();
        layers::refresh(game.state, game.table, game.sink)?;
    }
    Ok(Outcome::acted_on(affected))
}

/// Exiles a permanent, then returns it to the battlefield transformed under its owner's
/// control. It returns as a new object
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) with its back face up
/// ([`return_transformed`]). If it already left the battlefield, it isn't exiled, so it
/// doesn't return.
pub fn exile_and_return_transformed<L: Layout>(
    game: &mut Game<'_, L>,
    permanent: Permanent<L>,
) -> Result<(), Full> {
    let Some(permanent) = game.state.permanent(permanent.into()) else {
        return Ok(());
    };
    zones::move_to(game, permanent.into(), Zone::Exile, Reason::Exile)?;
    let slot = permanent.slot();
    if game.state.object(slot).zone() != Zone::Exile {
        return Ok(());
    }
    // Exile is never shared, so the exiled card has an owner.
    let Some(owner) = game.state.object(slot).owner() else {
        return Ok(());
    };
    return_transformed(game, owner, game.state.object_ref(slot))
}

/// Returns a card to the battlefield transformed under its owner's control
/// ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)): "If a spell or
/// ability puts a double-faced card onto the battlefield 'transformed' or 'converted,' it
/// enters the battlefield with its back face up. If a player is instructed to put a card
/// that isn't a double-faced card onto the battlefield transformed or converted, that card
/// stays in its current zone." It returns as a new object, and one that has changed zones
/// since it was named does not move
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)). A card in a shared
/// graveyard has no owner, and `player`, who performs the instruction, takes it.
#[allow(
    clippy::disallowed_methods,
    reason = "reads the back face the card would enter with (see CR 712.11b)"
)]
pub fn return_transformed<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    card: ObjectRef<L>,
) -> Result<(), Full> {
    if !game.state.is_current(card) {
        return Ok(());
    }
    let owner = game.state.object(card.slot).owner().unwrap_or(player);
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
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_by(s, t, sink, owner, card, to, Reason::Effect, entry)
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
    let taker = run.taker();
    for &slot in run.in_game_order(cards)?.iter() {
        let card = run.state().object_ref(slot);
        return_transformed(run.game(), taker, card)?;
    }
    Ok(run.now_in(cards, Zone::Battlefield))
}
