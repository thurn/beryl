//! Drawing a card ([CR 121](https://yawgatog.com/resources/magic-rules/#R121)).

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, FaceAbility, Replacement};
use crate::event::{Event, LossCause};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{State, Zone, ZoneKey};
use crate::table::GameTable;
use crate::trigger;

/// A player draws cards, one at a time
/// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)): "If a player is instructed
/// to draw multiple cards, that player performs that many individual card draws." The event
/// counts the cards actually drawn, if any.
///
/// Each draw from an empty library is offered first to a replacement effect that has the
/// player win instead ([CR 614.11](https://yawgatog.com/resources/magic-rules/#R61411)):
/// "Some effects replace card draws. These effects are applied even if no cards could be
/// drawn because there are no cards in the affected player's library." Winning ends the game
/// at once ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may
/// state that a player wins the game." The draws left are never made.
pub fn draw<L: Layout>(game: &mut Game<'_, L>, player: Seat, cards: u8) -> Result<(), Full> {
    let s = &mut *game.state;
    let mut drawn = 0;
    let mut wins = None;
    for _ in 0..cards {
        if let Some(&slot) = s.library(player).last() {
            drawn += 1;
            game.sink.note(Event::DrawCard { slot });
        } else {
            wins = wins_instead_of_drawing(s, game.table, player);
            if wins.is_some() {
                break;
            }
        }
        s.draw(game.sink, player)?;
    }
    if drawn > 0 {
        let cards = drawn;
        game.settle(Event::Draw { player, cards })?;
    }
    if let Some(cause) = wins {
        crate::sba::win_game(game.state, game.sink, player, cause);
    }
    Ok(())
}

/// Why `player`'s opponent loses, if `player` controls a permanent whose replacement effect
/// has them win rather than draw from an empty library: that ability.
fn wins_instead_of_drawing<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Option<LossCause<L>> {
    s.battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player)
        .find_map(|&slot| {
            let (index, a) = trigger::face(s, t, slot)
                .abilities
                .iter()
                .enumerate()
                .find(|(_, a)| a.replacement == Replacement::WinInsteadOfDrawingFromEmptyLibrary)?;
            crate::coverage::hit(a);
            let (card, face) = s.copiable(slot).record(t);
            Some(LossCause::Effect {
                source: Some(s.object_ref(slot)),
                card,
                face,
                ability: Some(FaceAbility::Own(AbilityIx::new(
                    u8::try_from(index).expect("a face has fewer than 256 abilities"),
                ))),
            })
        })
}

/// [`Effect::Draw`](beryl_card::Effect::Draw): "[Player] draws [count] cards"
/// ([CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211)): "A player draws a card by
/// putting the top card of their library into their hand." The outcome is the cards drawn.
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
