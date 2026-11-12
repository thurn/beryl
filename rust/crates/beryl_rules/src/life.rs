//! Life ([CR 119](https://yawgatog.com/resources/magic-rules/#R119)): gaining and losing it.

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;

/// A player gains life ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)): "If an
/// effect causes a player to gain life or lose life, that player's life total is adjusted
/// accordingly."
#[inline]
pub fn gain<L: Layout>(game: &mut Game<'_, L>, player: Seat, amount: u8) -> Result<(), Full> {
    let e = game.state.gain_life(player, amount);
    game.settle(e)
}

/// A player loses life (CR 119.3).
#[inline]
pub fn lose<L: Layout>(game: &mut Game<'_, L>, player: Seat, amount: u8) -> Result<(), Full> {
    let e = game.state.lose_life(player, amount);
    game.settle(e)
}

/// [`Effect::GainLife`](beryl_card::Effect::GainLife): "[Player] gains [amount] life." The
/// outcome is the life gained.
pub(crate) fn gain_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    amount: Amount,
) -> Result<Outcome<L>, Full> {
    change(run, players, amount, gain)
}

/// [`Effect::LoseLife`](beryl_card::Effect::LoseLife): "[Player] loses [amount] life." The
/// outcome is the life lost.
pub(crate) fn lose_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    amount: Amount,
) -> Result<Outcome<L>, Full> {
    change(run, players, amount, lose)
}

/// Each of `players` gains or loses `amount` life with `change`, in turn order (CR 101.4).
/// None of it is no event.
fn change<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    amount: Amount,
    change: impl Fn(&mut Game<'_, L>, Seat, u8) -> Result<(), Full>,
) -> Result<Outcome<L>, Full> {
    let amount = run.scene().amount(amount);
    if amount == 0 {
        return Ok(Outcome::NOTHING);
    }
    let mut total = 0;
    for player in run.in_turn_order(players) {
        let game = run.game();
        let before = game.state.player(player).life();
        change(game, player, amount)?;
        total += u32::from(game.state.player(player).life().abs_diff(before));
    }
    Ok(Outcome::of(Bitboard::EMPTY, total))
}
