//! Turn structure: turns, steps and their turn-based actions (architecture §12.1).

use crate::combat;
use crate::decision::{Outcome, StallReason};
use crate::drawing;
use crate::event::{self, Reason, Sink};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layers;
use crate::layout::Layout;
use crate::state::{Duration, Pending, State, TurnStep, Zone, ZoneKey};
use crate::table::GameTable;

/// The maximum hand size ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)),
/// "normally seven".
pub(crate) const MAX_HAND_SIZE: u32 = 7;

/// Whether any creature is attacking.
pub(crate) fn any_attacking<L: Layout>(s: &State<L>) -> bool {
    s.battlefield()
        .iter()
        .any(|&slot| s.object(slot).is_attacking())
}

/// Begins the next turn: the starting player's first, then alternating
/// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)): "The starting player takes
/// their first turn."
pub(crate) fn begin_turn<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if s.turn().number() >= s.profile().limits.turns {
        s.end_game(Outcome::Stalled(StallReason::Turns));
        return Ok(());
    }
    let active = s.begin_turn();
    // CR 611.2a: an effect lasts "as long as stated by the spell or ability creating it";
    // one that lasts until a player's next turn ends as that turn begins.
    s.expire(Duration::UntilYourNextTurn(active));
    layers::refresh(s, t, sink)?;
    // CR 302.6: the active player's permanents have now been under their control since their
    // most recent turn began.
    s.reset_permanents_for_turn(active);
    begin_step(s, t, sink, TurnStep::Untap)
}

/// Begins a step: its turn-based actions, then priority for the active player.
///
/// [CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a): "The active player
/// receives priority at the beginning of most steps and phases, after any turn-based actions
/// (such as drawing a card during the draw step; see rule 703) have been dealt with".
///
/// [CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006): "When a phase or step
/// begins, any abilities that trigger 'at the beginning of' that phase or step trigger. They
/// are put on the stack the next time a player would receive priority."
pub(crate) fn begin_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    step: TurnStep,
) -> Result<(), Full> {
    let began = s.begin_step(step);
    event::settle(s, t, sink, began)?;
    let active = s.turn().active();
    match step {
        TurnStep::Untap => {
            untap(s, t, sink)?;
            // CR 502.4: "No player receives priority during the untap step".
            return end_step(s, t, sink);
        }
        TurnStep::Draw => {
            // CR 103.8a: "In a two-player game, the player who plays first skips the draw
            // step (see rule 504, "Draw Step") of their first turn."
            if s.turn().number() > 1 {
                // CR 504.1: "First, the active player draws a card."
                drawing::draw(&mut Game::new(s, t, sink), active, 1)?;
            }
        }
        TurnStep::DeclareAttackers => combat::begin_attackers(s),
        TurnStep::DeclareBlockers => combat::begin_blockers(s),
        TurnStep::FirstStrikeDamage | TurnStep::CombatDamage => {
            combat::begin_damage(s, t, sink)?;
        }
        TurnStep::Cleanup => cleanup(s),
        _ => {}
    }
    Ok(())
}

/// Untaps the active player's permanents
/// ([CR 502.3](https://yawgatog.com/resources/magic-rules/#R5023)): "the active player
/// determines which permanents they control will untap. Then they untap them all
/// simultaneously."
fn untap<L: Layout>(s: &mut State<L>, t: &GameTable, sink: &mut Sink<L>) -> Result<(), Full> {
    let active = s.turn().active();
    let mut i = 0;
    while i < s.battlefield().len() {
        let slot = s.battlefield()[i];
        if s.controller(slot) == active {
            event::set_tapped(s, t, sink, s.object_ref(slot), false)?;
        }
        i += 1;
    }
    Ok(())
}

/// Ends the current step and begins the next.
///
/// [CR 500.2](https://yawgatog.com/resources/magic-rules/#R5002): "A phase or step in which
/// players receive priority ends when the stack is empty and all players pass in succession."
///
/// [CR 500.5](https://yawgatog.com/resources/magic-rules/#R5005): "As a step or phase ends ...
/// any unspent mana left in a player's mana pool empties."
pub(crate) fn end_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    s.empty_mana_pools();
    let next = match s.turn().step() {
        TurnStep::Untap => TurnStep::Upkeep,
        TurnStep::Upkeep => TurnStep::Draw,
        TurnStep::Draw => TurnStep::PrecombatMain,
        TurnStep::PrecombatMain => TurnStep::BeginningOfCombat,
        TurnStep::BeginningOfCombat => TurnStep::DeclareAttackers,
        // CR 508.8: "If no creatures are declared as attackers or put onto the battlefield
        // attacking, skip the declare blockers and combat damage steps."
        TurnStep::DeclareAttackers if s.turn().attacked() || any_attacking(s) => {
            TurnStep::DeclareBlockers
        }
        TurnStep::DeclareAttackers | TurnStep::CombatDamage => TurnStep::EndOfCombat,
        TurnStep::DeclareBlockers => {
            layers::refresh(s, t, sink)?;
            if combat::first_strike_step(s) {
                TurnStep::FirstStrikeDamage
            } else {
                TurnStep::CombatDamage
            }
        }
        TurnStep::FirstStrikeDamage => TurnStep::CombatDamage,
        TurnStep::EndOfCombat => {
            combat::end_combat(s);
            TurnStep::PostcombatMain
        }
        TurnStep::PostcombatMain => TurnStep::End,
        // CR 514.3a: once players pass with an empty stack in a cleanup step where they
        // received priority, "another cleanup step begins."
        TurnStep::End | TurnStep::Cleanup => TurnStep::Cleanup,
    };
    begin_step(s, t, sink, next)
}

/// The cleanup step's first turn-based action
/// ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)): "if the active player's
/// hand contains more cards than their maximum hand size (normally seven), they discard enough
/// cards to reduce their hand size to that number." The discards are decisions, one card at a
/// time; [`finish_cleanup`] follows them.
fn cleanup<L: Layout>(s: &mut State<L>) {
    if s.hand_size(s.turn().active()) > MAX_HAND_SIZE {
        s.ask(Pending::Discard);
    } else {
        finish_cleanup(s);
    }
}

/// Discards one card in the cleanup step, finishing the step once the hand is small enough.
///
/// [CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a): "To discard a card, move
/// it from its owner's hand to that player's graveyard."
pub(crate) fn cleanup_discard<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
) -> Result<(), Full> {
    event::move_one(s, t, sink, slot, Zone::Graveyard, Reason::Discard)?;
    if s.hand_size(s.turn().active()) <= MAX_HAND_SIZE {
        s.settle_question();
        finish_cleanup(s);
    }
    Ok(())
}

/// The rest of the cleanup step.
///
/// [CR 514.2](https://yawgatog.com/resources/magic-rules/#R5142): "all damage marked on
/// permanents (including phased-out permanents) is removed and all 'until end of turn' and
/// 'this turn' effects end."
///
/// [CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a): "the game checks to see if
/// any state-based actions would be performed and/or any triggered abilities are waiting to
/// be put onto the stack ... If so, ... the active player gets priority." That check runs
/// where every priority check runs, in `next`; `cleanup_check` tells it to end the turn
/// instead when nothing happens.
fn finish_cleanup<L: Layout>(s: &mut State<L>) {
    s.clear_damage();
    s.expire(Duration::EndOfTurn);
    s.set_cleanup_check(true);
}

/// Whether a player may play a land now.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
/// may play a land card from their hand during a main phase of their turn when the stack is
/// empty."
///
/// [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can normally play
/// one land during their turn".
pub(crate) fn may_play_land<L: Layout>(s: &State<L>, player: Seat) -> bool {
    player == s.turn().active()
        && matches!(
            s.turn().step(),
            TurnStep::PrecombatMain | TurnStep::PostcombatMain
        )
        && s.stack().is_empty()
        && s.player(player).lands_played() < 1
}

/// The cards in a player's hand.
pub(crate) fn hand<L: Layout>(s: &State<L>, player: Seat) -> crate::bitboard::Bitboard<L::Slot> {
    s.zone(ZoneKey::Of(player, Zone::Hand))
}
