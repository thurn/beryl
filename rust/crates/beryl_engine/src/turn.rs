//! Turn structure: turns, steps and their turn-based actions (architecture §12.1).

use crate::decision::{Outcome, StallReason};
use crate::event::{self, Event, Reason, Sink};
use crate::fixed::Full;
use crate::layers;
use crate::state::{Duration, ObjectFlags, Pending, State, TurnStep, Zone, opponent};
use crate::table::GameTable;
use crate::{combat, zone};

/// The maximum hand size ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)),
/// "normally seven".
pub(crate) const MAX_HAND_SIZE: u32 = 7;

/// The players in APNAP order ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)):
/// "the active player ... makes any choices required, then the next player in turn order".
pub(crate) fn apnap(s: &State) -> [u8; 2] {
    [s.turn.active, opponent(s.turn.active)]
}

/// Whether any creature is attacking.
pub(crate) fn any_attacking(s: &State) -> bool {
    s.battlefield
        .iter()
        .any(|&slot| s.object(slot).flags.has(ObjectFlags::ATTACKING))
}

/// Begins the next turn: the starting player's first, then alternating
/// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)): "The starting player takes
/// their first turn."
pub(crate) fn begin_turn(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<(), Full> {
    if s.turn.number >= s.profile.limits.turns {
        s.outcome = Some(Outcome::Stalled(StallReason::Turns));
        return Ok(());
    }
    s.turn.active = if s.turn.number == 0 {
        s.turn.starting_player
    } else {
        opponent(s.turn.active)
    };
    s.turn.number += 1;
    s.budgets.decisions = 0;
    for p in &mut s.players {
        p.lands_played = 0;
    }
    let active = s.turn.active;
    // CR 611.2a: an effect lasts "as long as stated by the spell or ability creating it";
    // one that lasts until a player's next turn ends as that turn begins.
    let before = s.effects.len();
    s.effects
        .retain(|e| e.duration != Duration::UntilYourNextTurn(active));
    if s.effects.len() != before {
        s.flags.derived_dirty = true;
    }
    layers::refresh(s, t);
    // CR 302.6: the active player's permanents have now been under their control since their
    // most recent turn began. Nothing has entered, and no loyalty ability has been
    // activated (CR 606.3), this turn.
    for i in 0..s.battlefield.len() {
        let slot = s.battlefield.as_slice()[i];
        let controlled = s.controller(slot) == active;
        let o = s.object_mut(slot);
        if controlled {
            o.flags.set(ObjectFlags::SUMMONING_SICK, false);
        }
        o.flags.set(ObjectFlags::ENTERED_THIS_TURN, false);
        o.flags.set(ObjectFlags::LOYALTY_ACTIVATED, false);
    }
    // "Until your next turn" ends as that turn begins.
    s.player_mut(active).sorceries_as_flash = false;
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
pub(crate) fn begin_step(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    step: TurnStep,
) -> Result<(), Full> {
    s.turn.step = step;
    s.turn.priority = s.turn.active;
    s.turn.passes = 0;
    let active = s.turn.active;
    event::settle(s, t, sink, Event::StepBegins { step, active })?;
    match step {
        TurnStep::Untap => {
            untap(s, t, sink)?;
            // CR 502.4: "No player receives priority during the untap step".
            return end_step(s, t, sink);
        }
        TurnStep::Draw => {
            // CR 103.8a: "In a two-player game, the player who plays first skips the draw
            // step (see rule 504, "Draw Step") of their first turn."
            if s.turn.number > 1 {
                // CR 504.1: "First, the active player draws a card."
                event::draw(s, t, sink, active, 1)?;
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
fn untap(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<(), Full> {
    let active = s.turn.active;
    let mut i = 0;
    while i < s.battlefield.len() {
        let slot = s.battlefield.as_slice()[i];
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
pub(crate) fn end_step(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<(), Full> {
    for p in &mut s.players {
        p.mana = [0; 6];
    }
    let next = match s.turn.step {
        TurnStep::Untap => TurnStep::Upkeep,
        TurnStep::Upkeep => TurnStep::Draw,
        TurnStep::Draw => TurnStep::PrecombatMain,
        TurnStep::PrecombatMain => TurnStep::BeginningOfCombat,
        TurnStep::BeginningOfCombat => TurnStep::DeclareAttackers,
        // CR 508.8: "If no creatures are declared as attackers or put onto the battlefield
        // attacking, skip the declare blockers and combat damage steps."
        TurnStep::DeclareAttackers if any_attacking(s) => TurnStep::DeclareBlockers,
        TurnStep::DeclareAttackers | TurnStep::CombatDamage => TurnStep::EndOfCombat,
        TurnStep::DeclareBlockers => {
            layers::refresh(s, t);
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
fn cleanup(s: &mut State) {
    if zone::hand_size(s, s.turn.active) > MAX_HAND_SIZE {
        s.pending = Pending::Discard;
    } else {
        finish_cleanup(s);
    }
}

/// Discards one card in the cleanup step, finishing the step once the hand is small enough.
///
/// [CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a): "To discard a card, move
/// it from its owner's hand to that player's graveyard."
pub(crate) fn cleanup_discard(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    slot: u8,
) -> Result<(), Full> {
    event::move_one(s, t, sink, slot, Zone::Graveyard, Reason::Discard)?;
    if zone::hand_size(s, s.turn.active) <= MAX_HAND_SIZE {
        s.pending = Pending::None;
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
fn finish_cleanup(s: &mut State) {
    for i in 0..s.battlefield.len() {
        let slot = s.battlefield.as_slice()[i];
        s.object_mut(slot).damage = 0;
    }
    let before = s.effects.len();
    s.effects.retain(|e| e.duration != Duration::EndOfTurn);
    if s.effects.len() != before {
        s.flags.derived_dirty = true;
    }
    // Permissions that last this turn end, and those whose card has left its zone are gone.
    let mut i = 0;
    while i < s.permissions.len() {
        let p = s.permissions.as_slice()[i];
        if p.this_turn || !s.is_current(p.object) {
            s.permissions.remove(i);
        } else {
            i += 1;
        }
    }
    s.turn.cleanup_check = true;
}

/// Whether a player may play a land now.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
/// may play a land card from their hand during a main phase of their turn when the stack is
/// empty."
///
/// [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can normally play
/// one land during their turn".
pub(crate) fn may_play_land(s: &State, player: u8) -> bool {
    player == s.turn.active
        && matches!(
            s.turn.step,
            TurnStep::PrecombatMain | TurnStep::PostcombatMain
        )
        && s.stack.is_empty()
        && s.player(player).lands_played < 1
}

/// The cards in a player's hand.
pub(crate) fn hand(s: &State, player: u8) -> crate::bitboard::Bitboard {
    s.zone(player, Zone::Hand)
}
