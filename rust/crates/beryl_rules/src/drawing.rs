//! Drawing a card ([CR 121]).
//!
//! [CR 121]: https://yawgatog.com/resources/magic-rules/#R121

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::card::Ability;
#[cfg(doc)]
use crate::card::Replaced;
use crate::event::{Event, ReplacementKind};
use crate::filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::{self, Instead};
use crate::state::{Count, TurnStep, Zone, ZoneKey};
use crate::{ending, replace};

/// A player draws cards, one at a time ([CR 121.2]): "If a player is instructed to draw multiple
/// cards, that player performs that many individual card draws." The event counts the cards
/// actually drawn, if any.
///
/// Each draw is offered first to a replacement effect of a permanent ([`Replaced::Draw`],
/// [`replace::draw`]), an empty library or not ([CR 614.11]): "Some effects replace card draws.
/// These effects are applied even if no cards could be drawn because there are no cards in the
/// affected player's library." What happens instead follows the cards drawn before it. Where it
/// ends the game, the draws left are never made. With no such permanent on the battlefield a draw
/// reads no ability.
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
    let turn = game.state.turn();
    let in_draw_step = turn.step() == TurnStep::Draw && turn.active() == player;
    let mut left = cards;
    loop {
        let s = &mut *game.state;
        let mut drawn = 0;
        let mut replaced = None;
        while left > 0 {
            left -= 1;
            if replaceable {
                replaced = replace::draw(s, game.table, player);
                if replaced.is_some() {
                    break;
                }
            }
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
        let Some((instead, cause, controller)) = replaced else {
            return Ok(());
        };
        // "You" and "each opponent" in it are read for the permanent's controller
        // (CR 109.5).
        let s = &*game.state;
        let named = |players| (s.players()).filter(|who| filter::is_for(&players, who, controller));
        match instead {
            Instead::Win(players) => ending::win_for(game, named(players), cause),
            Instead::Lose(players) => ending::lose_for(game, named(players), cause),
        }
        if game.state.outcome().is_some() {
            return Ok(());
        }
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
