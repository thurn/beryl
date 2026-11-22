//! Instructions that bring cards into the game.
//!
//! Implements: none. Resolves wishes using the kernel’s sideboard store.

use beryl_card::{Filter, Prompt, Purpose};

use crate::actions::reveal;
use crate::card::Stop;
use crate::decision::{Decision, DecisionKind, Opt};
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Reason};
use crate::state::Zone;

/// A wish offers the controller each eligible name once, with a way to decline. The chosen
/// occurrence is revealed to everyone and moves into its owner's hand. No objects are made
/// for the other choices.
///
/// See [CR 400.11]: "An object is outside the game if it isn’t in any of the game’s zones.
/// Outside the game is not a zone."
///
/// [CR 400.11]: https://yawgatog.com/resources/magic-rules/#R40011
pub(crate) fn wish<L: Layout>(
    run: &mut Run<'_, '_, L>,
    filter: Filter,
) -> Result<Outcome<L>, Stop> {
    let (game, resolution) = run.resolving();
    let player = resolution.controller();
    // See [CR 724.4]: "If information about cards outside the game would be visible to
    // the player being controlled, it’s visible only to that player, not the controller
    // of the player." The controller cannot choose an outside card on that player's behalf.
    //
    // [CR 724.4]: https://yawgatog.com/resources/magic-rules/#R7244
    if game.state.decider(player) != player {
        return Ok(Outcome::NOTHING);
    }
    let words = resolution.words(game);
    let mut options: L::Queue<Opt<L>, { crate::decision::OPTIONS }> = Default::default();
    for (index, card) in game.state.sideboard(game.table, player) {
        if crate::filter::matches_outside(
            &filter,
            game.table.def(card),
            player,
            &words,
            resolution.x(),
        ) && !options.iter().any(|&option| {
            matches!(option, Opt::Number(previous)
                    if game.table.sideboard_card(player, previous) == Some(card))
        }) {
            options.push(Opt::Number(index))?;
        }
    }
    if options.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    options.push(Opt::Done)?;
    let decision = Decision::options(
        player,
        DecisionKind::ChooseUpTo,
        options,
        Prompt::new(Purpose::Wish).most(1),
    );
    let answer = resolution.answer(game, &decision)?;
    let Some(Opt::Number(index)) = decision.candidates.option(answer) else {
        return Ok(Outcome::NOTHING);
    };
    let slot = game
        .state
        .prepare_sideboard_card(game.table, player, index)?
        .expect("the wish chose an available sideboard occurrence");
    reveal::reveal(game, player, slot);
    event::move_one(
        game.state,
        game.table,
        game.sink,
        slot,
        Zone::Hand,
        Reason::Effect,
    )?;
    Ok(Outcome::acted_on(std::iter::once(slot).collect()))
}
