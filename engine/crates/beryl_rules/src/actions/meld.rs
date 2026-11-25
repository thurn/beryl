//! Meld: two cards of a meld pair become one permanent with the pair's combined back face up
//! (see [CR 701.42](https://yawgatog.com/resources/magic-rules/#R70142)).
//!
//! The permanent is the card whose ability melds it, with a copy effect that gives it the
//! combined back face ([`Copiable::melded`]), and the other card waits under it as a merged
//! permanent's other cards do ([`State::meld_under`]). When it leaves the battlefield, both
//! cards go to the new zone, each as itself (`beryl_kernel`'s `State::move_object`).
//!
//! Implements: CR 701.42a, CR 701.42b, CR 701.42c, CR 713.1a, CR 713.6

use beryl_card::Filter;
use beryl_card::definition::Shape;

use crate::bitboard::Bitboard;
use crate::continuous::layers;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run, Scene};
use crate::layout::{Layout, Queue};
use crate::query::Copiable;
use crate::replacement::event::{self, Entry, Reason};
use crate::state::{State, Zone};
use crate::table::GameTable;

use super::exile::exile;

/// The other card of the meld pair of the object in `slot`, by its copiable values: the
/// partner its definition names and the combined back face they meld into, if it is a meld
/// card ([`Shape::Meld`]).
fn pair_of<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<(&'static str, &'static str)> {
    match s.copiable(slot).shape(t) {
        Shape::Meld { with, into } => Some((with, into)),
        Shape::Single
        | Shape::Transforming
        | Shape::Modal
        | Shape::Split { .. }
        | Shape::Adventure
        | Shape::Prepared
        | Shape::Flip
        | Shape::Melded => None,
    }
}

/// Whether `player` both owns and controls the object in `slot`, a permanent.
fn owns_and_controls<L: Layout>(s: &State<L>, player: Seat, slot: L::Slot) -> bool {
    s.object(slot).on_battlefield()
        && s.object(slot).owner() == player
        && s.controller(slot) == player
}

impl<L: Layout> Scene<'_, L> {
    /// The ability's source, if it is still the permanent it was, and a permanent `partner`
    /// accepts that has the name of the other card of the source's meld pair, both owned and
    /// controlled by the ability's controller: what [`beryl_card::Condition::MeldPair`] asks
    /// for, and what [`beryl_card::Effect::Meld`] exiles. Of several such permanents, the
    /// first in the order they entered the battlefield.
    pub(crate) fn meld_pair(&self, partner: Filter) -> Option<(L::Slot, L::Slot)> {
        let (s, t, you) = (self.s, self.t, self.you());
        let source = self
            .subject
            .of
            .source
            .filter(|&source| s.is_current(source))?
            .slot;
        if !owns_and_controls(s, you, source) {
            return None;
        }
        let (with, _) = pair_of(s, t, source)?;
        let named: Bitboard<L::Slot> = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| {
                slot != source
                    && owns_and_controls(s, you, slot)
                    && s.copiable(slot).name(t) == with
            })
            .collect();
        let other = s.next_permanent(&mut s.permanents_in(self.passing(named, partner)))?;
        Some((source, other.slot()))
    }
}

/// Whether the cards in `a` and `b` can be melded ([CR 701.42b]): "Only two cards belonging to
/// the same meld pair can be melded. Tokens, cards that aren’t meld cards, or meld cards that
/// don’t form a meld pair can’t be melded." Each must name the other as its partner, and
/// both the same combined back face, which the game holds. That face's game index.
///
/// [CR 701.42b]: https://yawgatog.com/resources/magic-rules/#R70142b
fn meldable<L: Layout>(s: &State<L>, t: &GameTable, a: L::Slot, b: L::Slot) -> Option<L::Card> {
    let printed = |slot: L::Slot| match s.card_def(t, slot)?.shape {
        Shape::Meld { with, into } => Some((t.def(s.card_really_is(slot)).name, with, into)),
        Shape::Single
        | Shape::Transforming
        | Shape::Modal
        | Shape::Split { .. }
        | Shape::Adventure
        | Shape::Prepared
        | Shape::Flip
        | Shape::Melded => None,
    };
    let (a_name, a_with, a_into) = printed(a)?;
    let (b_name, b_with, b_into) = printed(b)?;
    (a_with == b_name && b_with == a_name && a_into == b_into)
        .then(|| t.index_named(a_into))
        .flatten()
}

/// [`beryl_card::Effect::Meld`]: "exile them, then meld them into [result]" (CR 701.42a,
/// [CR 713.1a]): "One card in each meld pair has an ability that exiles both that object and
/// its counterpart and melds them." Its source and its partner are exiled together; then,
/// if both are cards of one meld pair in exile, the source enters the battlefield under the
/// ability's controller's control as the combined back face, and its partner goes under it
/// ([CR 713.6]): "If a meld card is being melded with its counterpart, those cards enter the
/// battlefield as a single permanent with their back faces up." Otherwise they stay in exile
/// ([CR 701.42c]): "If an effect instructs a player to meld objects that can’t be melded, they
/// stay in their current zone." The outcome is the melded permanent.
///
/// [CR 713.1a]: https://yawgatog.com/resources/magic-rules/#R7131a
/// [CR 713.6]: https://yawgatog.com/resources/magic-rules/#R7136
/// [CR 701.42c]: https://yawgatog.com/resources/magic-rules/#R70142c
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    partner: Filter,
) -> Result<Outcome<L>, Full> {
    let Some((source, other)) = run.scene().meld_pair(partner) else {
        return Ok(Outcome::NOTHING);
    };
    let you = run.scene().subject.of.controller.seat();
    let them = Bitboard::single(source) | Bitboard::single(other);
    run.remember(them)?;
    exile(run.game(), them)?;
    let game = run.game();
    let s = &*game.state;
    if s.object(source).zone() != Zone::Exile || s.object(other).zone() != Zone::Exile {
        return Ok(Outcome::NOTHING);
    }
    let Some(melded) = meldable(s, game.table, source, other) else {
        return Ok(Outcome::NOTHING);
    };
    game.sink.copies.push((source, Copiable::melded(melded)))?;
    let entry = Entry {
        controller: Some(you),
        ..Entry::default()
    };
    let to = Zone::Battlefield;
    let single = Bitboard::single(source);
    event::move_batch(
        game.state,
        game.table,
        game.sink,
        single,
        to,
        Reason::Effect,
        entry,
    )?;
    // A replacement effect that sent it elsewhere left its copy unstarted.
    if let Some(i) = game
        .sink
        .copies
        .iter()
        .position(|&(slot, _)| slot == source)
    {
        game.sink.copies.remove(i);
    }
    let Some(permanent) = game.state.permanent_at(source) else {
        return Ok(Outcome::NOTHING);
    };
    game.state.meld_under(game.sink, permanent, other)?;
    layers::refresh(game.state, game.table, game.sink)?;
    Ok(Outcome::acted_on(single))
}
