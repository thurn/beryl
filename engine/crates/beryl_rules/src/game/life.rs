//! Life ([CR 119](https://yawgatog.com/resources/magic-rules/#R119)): gaining and losing it.
//!
//! Implements: CR 119

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::continuous::rule_change;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::State;
use crate::table::GameTable;

/// A player gains life ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)): "If an
/// effect causes a player to gain life or lose life, that player's life total is adjusted
/// accordingly." Nothing happens to a player whose life total can't change
/// ([`rule_change::life_can_change`]): there is no life gain event for a replacement effect or
/// a trigger to see ([CR 119.7]): "a replacement effect that would replace a life gain event
/// affecting that player won't do anything."
///
/// [CR 119.7]: https://yawgatog.com/resources/magic-rules/#R1197
#[inline]
pub fn gain<L: Layout>(game: &mut Game<'_, L>, player: Seat, amount: u8) -> Result<(), Full> {
    if !rule_change::life_can_change(game.state, game.table, player) {
        return Ok(());
    }
    let e = game.state.gain_life(player, amount);
    game.settle(e)
}

/// A player loses life (CR 119.3), or nothing happens if their life total can't change
/// ([CR 119.8]): "If an effect says that a player can't lose life, that player can't make an
/// exchange such that the player's life total would become lower".
///
/// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
#[inline]
pub fn lose<L: Layout>(game: &mut Game<'_, L>, player: Seat, amount: i16) -> Result<(), Full> {
    if !rule_change::life_can_change(game.state, game.table, player) {
        return Ok(());
    }
    let e = game.state.lose_life(player, amount);
    game.settle(e)
}

/// Whether `player` can pay `amount` life: they have that much ([`State::has_life_to_pay`],
/// [CR 119.4]), and their life total can change ([CR 119.8]): "a cost that involves having that
/// player pay life can't be paid." Every rule that asks whether life can be paid asks here.
///
/// [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
/// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
#[inline]
pub(crate) fn can_pay<L: Layout>(s: &State<L>, t: &GameTable, player: Seat, amount: u8) -> bool {
    s.has_life_to_pay(player, amount) && (amount == 0 || rule_change::life_can_change(s, t, player))
}

/// [`Effect::GainLife`](beryl_card::Effect::GainLife): "[Player] gains [amount] life." The
/// outcome is the life gained.
pub(crate) fn gain_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    amount: Amount,
) -> Result<Outcome<L>, Full> {
    let amount = run.scene().amount(amount);
    change(
        run,
        players,
        |game, player| gain(game, player, amount),
        amount == 0,
    )
}

/// [`Effect::LoseLife`](beryl_card::Effect::LoseLife): "[Player] loses [amount] life." The
/// outcome is the life lost.
pub(crate) fn lose_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    amount: Amount,
) -> Result<Outcome<L>, Full> {
    let amount = i16::try_from(run.scene().value(amount).max(0)).unwrap_or(i16::MAX);
    change(
        run,
        players,
        |game, player| lose(game, player, amount),
        amount == 0,
    )
}

/// Each of `players` gains or loses `amount` life with `change`, in turn order (see CR 101.4).
/// None of it is no event.
fn change<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    change: impl Fn(&mut Game<'_, L>, Seat) -> Result<(), Full>,
    zero: bool,
) -> Result<Outcome<L>, Full> {
    if zero {
        return Ok(Outcome::NOTHING);
    }
    let mut total = 0;
    for player in run.in_turn_order(players) {
        let game = run.game();
        let before = game.state.player(player).life();
        change(game, player)?;
        let diff = beryl_kernel::Number::to_i64(game.state.player(player).life())
            .abs_diff(beryl_kernel::Number::to_i64(before));
        total += u32::try_from(diff).unwrap_or(u32::MAX);
    }
    Ok(Outcome::of(Bitboard::EMPTY, total))
}
