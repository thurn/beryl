//! Ending the game ([CR 104](https://yawgatog.com/resources/magic-rules/#R104)) as an
//! effect says.

use beryl_card::Players;

use crate::PlayerSet;
use crate::event::LossCause;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::sba;

/// `player` wins the game by what is resolving
/// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
/// that a player wins the game." The game ends at once, and in a game that has ended
/// nothing more does.
pub fn win<L: Layout>(game: &mut Game<'_, L>, resolution: &Resolution<'_, L>, player: Seat) {
    win_for(game, PlayerSet::only(player), by(resolution));
}

/// Each of `players` loses the game by what is resolving
/// ([CR 104.3e](https://yawgatog.com/resources/magic-rules/#R1043e)): "An effect may state
/// that a player loses the game." They lose at once and together.
pub fn lose<L: Layout>(game: &mut Game<'_, L>, resolution: &Resolution<'_, L>, players: PlayerSet) {
    lose_for(game, players, by(resolution));
}

/// What is resolving, as the cause of a loss.
fn by<L: Layout>(resolution: &Resolution<'_, L>) -> LossCause<L> {
    let entry = &resolution.entry;
    LossCause::Effect {
        source: entry.object(),
        card: entry.card(),
        face: entry.face(),
        ability: entry.ability(),
    }
}

/// Each of `winners` wins the game for `cause` (CR 104.2b): each opponent of one of them
/// loses it for that cause, which leaves the winners alone in the game. Nothing in a game
/// that has ended.
pub(crate) fn win_for<L: Layout>(game: &mut Game<'_, L>, winners: PlayerSet, cause: LossCause<L>) {
    let s = &*game.state;
    let mut losers = PlayerSet::NOBODY;
    // A union: the order the winners are met in is nothing.
    for winner in s.players_in_seat_order(winners) {
        for loser in s.players_in_seat_order(s.opponents_of(winner)) {
            losers.insert(loser);
        }
    }
    lose_for(game, losers, cause);
}

/// Each of `losers` loses the game for `cause` (CR 104.3e). Nothing in a game that has
/// ended.
pub(crate) fn lose_for<L: Layout>(game: &mut Game<'_, L>, losers: PlayerSet, cause: LossCause<L>) {
    if game.state.outcome().is_none() {
        sba::lose_game(game.state, game.sink, losers, cause);
    }
}

/// [`Effect::WinGame`](beryl_card::Effect::WinGame): "[Player] wins the game." The outcome
/// is whether the game ended.
pub(crate) fn win_instruction<L: Layout>(run: &mut Run<'_, '_, L>, players: Players) -> Outcome<L> {
    let winners = run.scene().players(players);
    let (game, resolution) = run.resolving();
    win_for(game, winners, by(resolution));
    ended(run)
}

/// [`Effect::LoseGame`](beryl_card::Effect::LoseGame): "[Player] loses the game." The
/// outcome is whether the game ended.
pub(crate) fn lose_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
) -> Outcome<L> {
    let losers = run.scene().players(players);
    let (game, resolution) = run.resolving();
    lose(game, resolution, losers);
    ended(run)
}

/// The outcome of an instruction that may have ended the game.
fn ended<L: Layout>(run: &Run<'_, '_, L>) -> Outcome<L> {
    Outcome::of(
        crate::bitboard::Bitboard::EMPTY,
        u32::from(run.state().outcome().is_some()),
    )
}
