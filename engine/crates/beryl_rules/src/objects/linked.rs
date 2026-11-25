//! Linked abilities (see [CR 607](https://yawgatog.com/resources/magic-rules/#R607)): the cards
//! one ability of an object exiles, which another of its abilities refers to.
//!
//! Implements: CR 607.2a

use beryl_card::Objects;

use crate::actions::exile;
use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::state::{Memo, ObjectRef, Zone};

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
    let Some(object) = game.state.current(object) else {
        return Ok(());
    };
    exile_and_link(game, Bitboard::single(object.reference().slot), source)
}

fn exile_and_link<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    source: ObjectRef<L>,
) -> Result<(), Full> {
    exile::exile(game, objects)?;
    let exiled = objects & game.state.in_zone(Zone::Exile);
    link(game, exiled, source, Link::ExiledWith)
}

/// What an object moved by an ability remembers of the ability's source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Link {
    /// A card in exile is "exiled with" the source (CR 607.2a).
    ExiledWith,
    /// A permanent was put onto the battlefield with the source, as Animate Dead's "enchant
    /// creature put onto the battlefield with this Aura" reads.
    PutOntoBattlefieldWith,
}

/// Has each of `moved`, in game order, remember `source` as `link` says, if `source` is still
/// the object whose ability moved them. A source that has left its zone since is a new object
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), so nothing is linked
/// to it.
pub(crate) fn link<L: Layout>(
    game: &mut Game<'_, L>,
    moved: Bitboard<L::Slot>,
    source: ObjectRef<L>,
    link: Link,
) -> Result<(), Full> {
    if game.state.current(source).is_none() {
        return Ok(());
    }
    let memo = match link {
        Link::ExiledWith => Memo::exiled_with(source),
        Link::PutOntoBattlefieldWith => Memo::PutOntoBattlefieldWith(source.slot),
    };
    for &slot in game.state.in_game_order(game.table, moved)?.iter() {
        game.state.remember(slot, memo)?;
    }
    Ok(())
}

/// [`Effect::ExileLinked`](beryl_card::Effect::ExileLinked): "Exile [cards]", linked to the
/// object the ability is on (CR 607.2a). The outcome is the cards that are in exile.
///
/// # Panics
///
/// For an emblem's ability, which has no source.
pub(crate) fn exile_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    let cards = run.elsewhere_than(cards, Zone::Exile);
    run.remember(cards)?;
    let source = run.resolution().source();
    exile_and_link(run.game(), cards, source)?;
    Ok(run.now_in(cards, Zone::Exile))
}
