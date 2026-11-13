//! Drawing a card ([CR 121]).
//!
//! [CR 121]: https://yawgatog.com/resources/magic-rules/#R121

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
#[cfg(doc)]
use crate::card::Replacement;
use crate::card::{Ability, AbilityIx, FaceAbility};
use crate::event::{Event, Listens, LossCause};
use crate::filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{self, Outcome, Run};
use crate::layout::Layout;
use crate::replacement::{self, Instead};
use crate::state::{State, Zone, ZoneKey};
use crate::table::GameTable;
use crate::{ending, trigger};

/// A player draws cards, one at a time ([CR 121.2]): "If a player is instructed to draw multiple
/// cards, that player performs that many individual card draws." The event counts the cards
/// actually drawn, if any.
///
/// Each draw is offered first to a replacement effect of the player's permanents
/// ([`Replacement::WouldDraw`]), an empty library or not ([CR 614.11]): "Some effects replace card
/// draws. These effects are applied even if no cards could be drawn because there are no cards in
/// the affected player's library." What happens instead follows the cards drawn before it. Where it
/// ends the game, the draws left are never made. With no such permanent on the battlefield a draw
/// reads no ability.
///
/// [CR 121.2]: https://yawgatog.com/resources/magic-rules/#R1212
/// [CR 614.11]: https://yawgatog.com/resources/magic-rules/#R61411
pub fn draw<L: Layout>(game: &mut Game<'_, L>, player: Seat, cards: u8) -> Result<(), Full> {
    let listening = game.state.listening();
    let replaceable = listening
        .unwrap_or(game.table.listens_any())
        .contains(Listens::DRAW_REPLACEMENT);
    let mut left = cards;
    loop {
        let s = &mut *game.state;
        let mut drawn = 0;
        let mut replaced = None;
        while left > 0 {
            left -= 1;
            if replaceable {
                replaced = replacement(s, game.table, player);
                if replaced.is_some() {
                    break;
                }
            }
            if let Some(&slot) = s.library(player).last() {
                drawn += 1;
                game.sink.note(Event::DrawCard { slot });
            }
            s.draw(game.sink, player)?;
        }
        if drawn > 0 {
            let cards = drawn;
            game.settle(Event::Draw { player, cards })?;
        }
        let Some((instead, cause)) = replaced else {
            return Ok(());
        };
        // "You" and "each opponent" in it are read for the permanent's controller, who is
        // the player drawing (CR 109.5).
        let s = &*game.state;
        let named = |players| (s.players()).filter(|who| filter::is_for(&players, who, player));
        match instead {
            Instead::Win(players) => ending::win_for(game, named(players), cause),
            Instead::Lose(players) => ending::lose_for(game, named(players), cause),
        }
        if game.state.outcome().is_some() {
            return Ok(());
        }
    }
}

/// What happens instead of `player`'s next draw, and the ability that says so, if a
/// permanent they control replaces it now: the first, in the order of the battlefield,
/// whose condition holds.
fn replacement<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Option<(Instead, LossCause<L>)> {
    s.battlefield()
        .iter()
        .filter(|&&slot| {
            s.copiable(slot)
                .may_listen(t)
                .contains(Listens::DRAW_REPLACEMENT)
                && s.controller(slot) == player
        })
        .find_map(|&slot| {
            let source = Some(s.object_ref(slot));
            let (card, face) = s.copiable(slot).record(t);
            let abilities = trigger::face(s, t, slot).abilities;
            (0u8..).zip(abilities).find_map(|(index, a)| {
                let ability = AbilityIx::new(index);
                if !replacement::replaces_draws(a)
                    || !interpret::static_condition_holds(s, t, slot, ability, a)
                {
                    return None;
                }
                crate::coverage::hit(a);
                // The card table has only draw replacements the engine can do.
                let instead = replacement::instead_of_drawing(a).ok()?;
                let ability = Some(FaceAbility::Own(ability));
                Some((
                    instead,
                    LossCause::Effect {
                        source,
                        card,
                        face,
                        ability,
                    },
                ))
            })
        })
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
