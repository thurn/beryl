//! Casting spells ([CR 601](https://yawgatog.com/resources/magic-rules/#R601)) as an effect
//! allows: at once, while the effect resolves, or later, by a permission
//! ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013): "A player can begin to
//! cast a spell only if a rule or effect allows that player to cast it and no rule or effect
//! prohibits that player from casting it."). A permission is an entry of the Effects store
//! on the card (`beryl_kernel`'s contract, "Stores"), and its terms become entries on the
//! spell as it is cast.
//!
//! Implements: CR 608.2g

use beryl_card::{Amount, CastTerms, Filter, Objects, Players, Types, Words};

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use beryl_card::FaceIx;

use crate::decision::{Decision, DecisionKind, Opt};
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run, no_primitive};
use crate::layout::{Layout, Queue};
use crate::replacement::event::{Cause, StackIx};
use crate::resolution::Resolution;
use crate::state::{
    Continuous, CostChange, Duration, MoveReplacement, ObjectRef, Pending, Permission, Permitted,
    State,
};

use super::{stack, steps};

/// Offers `player` one cast from `cards` during the resolution, with the option to decline.
/// Returns the slot of the spell cast, if any.
///
/// Under [CR 608.2g], "If an effect specifically instructs or allows a player to cast a
/// spell during resolution, they do so by following the steps in rules 601.2a-i, except no
/// player receives priority after it's cast."
///
/// [CR 608.2g]: https://yawgatog.com/resources/magic-rules/#R6082g
///
/// # Errors
///
/// Returns [`Stop::Pause`] for an unanswered choice and [`Stop::Full`] at a capacity limit.
pub fn offer<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    cards: Bitboard<L::Slot>,
    player: Seat,
    free: bool,
) -> Result<Option<L::Slot>, Stop> {
    offer_spell(game, resolution, cards, player, free, None, false)
}

#[allow(clippy::too_many_arguments)]
fn offer_spell<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    cards: Bitboard<L::Slot>,
    player: Seat,
    free: bool,
    below: Option<u8>,
    copy: bool,
) -> Result<Option<L::Slot>, Stop> {
    if cards.is_empty() {
        return Ok(None);
    }
    let mut options = stack::offer_options(game.state, game.table, cards, player, free)?;
    if let Some(below) = below {
        // See CR 702.85a: "if the resulting spell’s mana value is less than this spell’s mana
        // value." Each option names the face that would be on the stack. See CR 107.3b: "the
        // only legal choice for X is 0" when casting without paying its mana cost.
        options.retain(|option| {
            if let Opt::Cast { slot, face, .. } = *option {
                game.state
                    .card_def(game.table, slot)
                    .is_some_and(|def| def.face(face).cost.mana_value() < below)
            } else {
                matches!(option, Opt::Pass)
            }
        });
        if options.len() == 1 {
            return Ok(None);
        }
    }
    let question = Decision::options(
        player,
        DecisionKind::CastOffer,
        options,
        Prompt::new(Purpose::CastOffer),
    )
    .about(cards);
    let choice = resolution.answer_cast_step(game, &question)?;
    let Some(Opt::Cast { slot, face, costs }) = question.candidates.option(choice) else {
        return Ok(None);
    };
    // See CR 707.12: the copy "is created in the same zone the object is in and then cast".
    // The offer names the card, which the state the resolution replays from holds, and the copy
    // is made once it is chosen.
    let slot = if copy {
        game.state
            .make_card_copy(game.table, game.sink, slot, player)?
    } else {
        slot
    };
    let option = Opt::Cast { slot, face, costs };
    cast_during_resolution(game, resolution, player, option, free)?;
    Ok(Some(slot))
}

/// Performs a cast already chosen by the player during a resolution. At priority, the engine
/// commits the steps one at a time and the next step waits in [`Pending::Casting`]. Here that
/// same machine runs on the resolution's state, and every answer comes from its answer source.
/// A pause leaves the cast at its question in the replayed view. The committed state still
/// holds the original resolution and its record.
///
/// # Errors
///
/// Returns [`Stop::Pause`] for an unanswered choice and [`Stop::Full`] at a capacity limit.
///
/// # Panics
///
/// If `option` is not a legal cast.
pub fn cast_during_resolution<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    option: Opt<L>,
    free: bool,
) -> Result<(), Stop> {
    // The resolving entry still occupies its original place in the event stream. The
    // spell goes above it; all of its casting steps belong to that new entry.
    game.sink.enter(|entry| Cause::Player {
        player,
        entry: Some(StackIx::at(
            game.state.stack().len() + usize::from(entry.is_some()),
        )),
    });
    let cast = answer_steps(game, resolution, player, option, free);
    game.sink.leave();
    cast
}

fn answer_steps<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    option: Opt<L>,
    free: bool,
) -> Result<(), Stop> {
    steps::begin_cast(
        game.state,
        game.table,
        game.sink,
        player,
        option,
        Some(free),
    )?;
    while let Pending::Casting(casting) = *game.state.pending() {
        let question = steps::decision(game.state, game.table, game.sink, &casting)?;
        let choice = resolution.answer_cast_step(game, &question)?;
        steps::perform(
            game.state,
            game.table,
            game.sink,
            casting,
            question.candidates.option(choice),
            question.candidates.object(choice),
        )?;
    }
    Ok(())
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
        on_adventure: false,
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
    let source = resolution.entry.card();
    (game.state).start_effect(source, Continuous::Cast(object, terms), duration)
}

/// The permission that lets `player` cast the card in `slot`, if one does.
pub(crate) fn permission<L: Layout>(
    s: &State<L>,
    player: Seat,
    slot: L::Slot,
    face: FaceIx,
) -> Option<Permitted<L>> {
    s.cast_permissions()
        .find(|p| p.card.slot == slot && p.terms.player == player && p.terms.allows_face(face))
}

/// Starts, on the spell now in `slot`, the terms of the permission it was cast under: its
/// cost increase
/// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "plus all additional
/// costs and cost increases") and the replacement of where it goes as it leaves the stack
/// (see [CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)). The permission was
/// about the card where it was and ended as it left
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)); the spell is a new
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

/// [`Effect::MayCast`](beryl_card::Effect::MayCast) offers one cast. Its outcome is the
/// spell the player cast, if any: with `copy`, the copy of the card chosen.
pub(crate) fn may_cast_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    player: Players,
    card: Objects,
    free: bool,
    free_below: Option<Amount>,
    copy: bool,
) -> Result<Outcome<L>, Stop> {
    let cards = run.scene().objects(card);
    let below = free_below.map(|amount| run.scene().amount(amount));
    let Some(player) = run.one(player) else {
        return Ok(Outcome::NOTHING);
    };
    let (game, resolution) = run.resolving();
    Ok(
        offer_spell(game, resolution, cards, player, free, below, copy)?
            .map_or(Outcome::NOTHING, |slot| {
                Outcome::acted_on(Bitboard::single(slot))
            }),
    )
}

/// [`Effect::PermitCast`](beryl_card::Effect::PermitCast): "[Player] may cast [cards]
/// [duration]" (CR 601.3), on `terms`: [`allow`], for each card. The permission has one
/// player. The outcome is the cards.
pub(crate) fn permit_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    player: Players,
    cards: Objects,
    duration: beryl_card::Duration,
    terms: CastTerms,
) -> Result<Outcome<L>, Full> {
    let cards = run.scene().objects(cards);
    let Some(player) = run.one(player) else {
        return Ok(Outcome::NOTHING);
    };
    let this_turn = match duration {
        beryl_card::Duration::EndOfTurn => true,
        beryl_card::Duration::Indefinite => false,
        beryl_card::Duration::UntilYourNextTurn => no_primitive(&duration),
    };
    for &slot in run.in_game_order(cards)?.iter() {
        let card = run.state().object_ref(slot);
        // See CR 701.18b: "To play a card means to play that card as a land or to cast that card
        // as a spell, whichever is appropriate." Only casting is permitted here.
        if terms.played {
            let (game, _) = run.resolving();
            // The engine's own filter, which names no word of a card's text.
            let land = Filter::Types(Types::LAND);
            if land.matches(game.state, game.table, slot, player, &Words::NONE) {
                no_primitive(&terms);
            }
        }
        let (extra, exile) = (terms.costs_more, terms.exiled_instead);
        let (game, resolution) = run.resolving();
        allow(game, resolution, card, player, extra, this_turn, exile)?;
    }
    Ok(Outcome::acted_on(cards))
}
