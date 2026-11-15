//! Drawing a card ([CR 121]).
//!
//! [CR 121]: https://yawgatog.com/resources/magic-rules/#R121

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
#[cfg(doc)]
use crate::card::Replaced;
use crate::card::{Ability, Stop};
use crate::decision::{Answers, Choice, Decision};
#[cfg(doc)]
use crate::event::Sink;
use crate::event::{Event, ReplacementKind};
use crate::filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{self, Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::observation::Observation;
use crate::replace::DrawReplacement;
use crate::replacement::{self, Instead};
use crate::resolution::Resolution;
use crate::state::TriggeredAbility;
use crate::state::{AbilityRef, Count, PendingTrigger, TriggerInfo, TurnStep, Zone, ZoneKey};
use crate::table::Capability;
use crate::{ending, replace};

/// A player draws cards, one at a time ([CR 121.2]): "If a player is instructed to draw multiple
/// cards, that player performs that many individual card draws." The event counts the cards
/// actually drawn, if any.
///
/// Each draw is offered first to the dredge abilities of cards in the player's graveyard
/// ([`replace::dredge`]), which the player may choose one of, then to the replacement effects of
/// permanents ([`Replaced::Draw`], [`replace::draw`]), an empty library or not ([CR 614.11]):
/// "Some effects replace card draws. These effects are applied even if no cards could be drawn
/// because there are no cards in the affected player's library." What happens instead follows
/// the cards drawn before it ([`instead`]). Where it ends the game, the draws left are never
/// made. With no such permanent on the battlefield a draw reads no ability.
///
/// Each card drawn is counted for the player this turn, and in their own draw step as drawn
/// in it ([CR 504.1]): "First, the active player draws a card." A replaced draw never happened
/// ([CR 614.6]): "If an event is replaced, it never happens." Neither is a draw from an empty
/// library, which draws no card.
///
/// [CR 121.2]: https://yawgatog.com/resources/magic-rules/#R1212
/// [CR 614.11]: https://yawgatog.com/resources/magic-rules/#R61411
/// [CR 504.1]: https://yawgatog.com/resources/magic-rules/#R5041
/// [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146
pub fn draw<L: Layout>(game: &mut Game<'_, L>, player: Seat, cards: u8) -> Result<(), Full> {
    let replaceable = game
        .state
        .replacing()
        .unwrap_or(game.table.replaces_any())
        .replaces(ReplacementKind::Draw);
    let dredge = game.table.can(Capability::Dredge);
    let turn = game.state.turn();
    let in_draw_step = turn.step() == TurnStep::Draw && turn.active() == player;
    let mut left = cards;
    loop {
        let mut drawn = 0;
        let mut replaced = None;
        let mut dredged = false;
        while left > 0 {
            left -= 1;
            // A dredge mills and moves cards, events of their own after the cards drawn
            // before it, so the draws so far are settled first.
            if dredge && !replace::dredges(game.state, game.table, player).is_empty() {
                if drawn > 0 {
                    let cards = std::mem::take(&mut drawn);
                    game.settle(Event::Draw { player, cards })?;
                }
                if replace::dredge(game, player)? {
                    dredged = true;
                    break;
                }
            }
            if replaceable {
                replaced = replace::draw(game, player)?;
                if replaced.is_some() {
                    break;
                }
            }
            let s = &mut *game.state;
            if let Some(&slot) = s.library(player).last() {
                drawn += 1;
                game.sink.note(Event::DrawCard { slot });
                s.count(game.table, Count::CardsDrawn, player);
                if in_draw_step {
                    s.count(game.table, Count::DrewInDrawStep, player);
                }
            }
            s.draw(game.sink, player)?;
        }
        if drawn > 0 {
            let cards = drawn;
            game.settle(Event::Draw { player, cards })?;
        }
        if dredged {
            continue;
        }
        let Some(replacement) = replaced else {
            return Ok(());
        };
        instead(game, player, replacement)?;
        if game.state.outcome().is_some() {
            return Ok(());
        }
    }
}

/// Does what `replacement` has happen instead of `player`'s draw. "You" and "each opponent" in
/// it are read for the permanent's controller (CR 109.5).
///
/// A replacement whose body is instructions carries them out as a resolution would
/// ([`interpret::instead_of_drawing`]), with the drawing player as "that player". A draw they
/// make is a modified event, to which this replacement does not apply again, while the others
/// still may ([CR 614.5]): "A replacement effect doesn't invoke itself repeatedly; it gets only
/// one opportunity to affect an event or any modified events that may replace that event." So
/// it is recorded as applied ([`Sink::applied`]) while they run. A question they ask, such as
/// which card to discard, stops a run that may pause and is answered when it runs again
/// ([`crate::asking`]). A run that cannot pause, which a replacement no step could foresee
/// meets, takes each question's first answer.
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
fn instead<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    replacement: DrawReplacement<L>,
) -> Result<(), Full> {
    let s = &*game.state;
    let named = |players| {
        let controller = replacement.controller;
        (s.players()).filter(move |who| filter::is_for(&players, who, controller))
    };
    // The card table holds only draw replacements the engine can do.
    match replacement::instead_of_drawing(replacement.ability) {
        Ok(Instead::Win(players)) => {
            let cause = replacement.cause(s, game.table);
            ending::win_for(game, named(players), cause);
        }
        Ok(Instead::Lose(players)) => {
            let cause = replacement.cause(s, game.table);
            ending::lose_for(game, named(players), cause);
        }
        Ok(Instead::Instructions) => {
            game.sink.applied.push(replacement.applied(s))?;
            let done = instructions(game, player, replacement);
            game.sink.applied.pop();
            done?;
        }
        Err(_) => {}
    }
    Ok(())
}

/// Carries out the instructions of `replacement` in place of `player`'s draw ([`instead`]).
fn instructions<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    replacement: DrawReplacement<L>,
) -> Result<(), Full> {
    let s = &*game.state;
    let source = s.object_ref(replacement.permanent);
    // An ability of a card under a merged permanent is that card's, as its triggered
    // abilities are.
    let (card, face, index) = replacement.card(s, game.table);
    let of = PendingTrigger {
        of: AbilityRef {
            source: Some(source),
            card,
            face,
            ability: TriggeredAbility::own(index),
            controller: replacement.controller,
        },
        info: TriggerInfo::default(),
    };
    let answers: Option<&mut dyn Answers<L>> = if game.sink.asking.is_open() {
        None
    } else {
        // A resolution borrows its answer source for as long as the game: `FirstAnswer` has no
        // size, so leaking one allocates nothing.
        Some(Box::leak(Box::new(FirstAnswer)))
    };
    let mut resolution = Resolution::of_trigger(&of, replacement.ability).answered_by(answers);
    game.sink.enter_source(|| source);
    let flow = interpret::instead_of_drawing(game, &mut resolution, player);
    game.sink.leave();
    match flow {
        Ok(()) => Ok(()),
        Err(Stop::Full) => Err(Full),
        // A question the record could not answer stops the run, to be asked; with no question
        // the game is over.
        Err(Stop::Pause) => match resolution.question.take() {
            Some(question) => Err(game.sink.asking.stop(question)),
            None => Ok(()),
        },
    }
}

/// The answer source of instructions run where nothing can pause ([`instructions`]): each
/// question's first answer.
struct FirstAnswer;

impl<L: Layout> Answers<L> for FirstAnswer {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, _: Observation<'_, L>, _: &Decision<L>) -> Option<Choice> {
        Some(Choice(0))
    }
}

/// Whether `ability` has its controller win the game rather than draw a card, whatever its
/// condition: what a player may count on once that condition holds.
pub fn wins_instead_of_drawing(ability: &Ability) -> bool {
    replacement::replaces_draws(ability)
        && matches!(
            replacement::instead_of_drawing(ability),
            Ok(Instead::Win(Players::You))
        )
}

/// [`Effect::Draw`](beryl_card::Effect::Draw): "[Player] draws [count] cards" ([CR 121.1]): "A
/// player draws a card by putting the top card of their library into their hand." The outcome is
/// the cards drawn.
///
/// [CR 121.1]: https://yawgatog.com/resources/magic-rules/#R1211
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
) -> Result<Outcome<L>, Full> {
    let count = run.scene().amount(count);
    let mut drawn = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let game = run.game();
        let hand = game.state.zone(ZoneKey::Of(player, Zone::Hand));
        draw(game, player, count)?;
        drawn |= game.state.zone(ZoneKey::Of(player, Zone::Hand)) & !hand;
    }
    Ok(Outcome::acted_on(drawn))
}
