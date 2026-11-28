//! Turn structure: turns, steps and their turn-based actions (architecture §12.1).
//!
//! Implements: CR 103.8, CR 305.2, CR 500.2, CR 500.5, CR 500.6, CR 502.3, CR 502.4, CR 504.1, CR 514, CR 508.8, CR 703.4d, CR 703.4f

mod end;
pub(crate) use end::end_the_turn;

pub(crate) mod paused_step;
pub(crate) mod stops;

use beryl_kernel::format::{FirstDraw, Format};

use crate::bitboard::Bitboard;
use crate::combat;
use crate::continuous::{layers, rule_change};
use crate::decision::{Opt, Outcome, StallReason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::Outcome as Instructed;
use crate::layout::{Layout, Queue};
use crate::objects::phasing;
use crate::replacement::event::{self, Event, Reason, ReplacementKind, Sink};
use crate::replacement::{self, turns};
use crate::state::{Duration, ObjectRef, Pending, State, TurnStep, Zone, ZoneKey};
use crate::table::{Capability, GameTable};
use crate::zones::drawing;
use beryl_card::TurnPhase;
use beryl_kernel::rule_changes::RuleKind;
use beryl_kernel::{Placement, TurnChange};

/// Whether any creature is attacking.
pub(crate) fn any_attacking<L: Layout>(s: &State<L>) -> bool {
    s.battlefield()
        .iter()
        .any(|&slot| s.object(slot).is_attacking())
}

/// Whether this combat has attackers, so its declare blockers and combat damage steps happen
/// ([CR 508.8]): "If no creatures are declared as attackers or put onto the battlefield
/// attacking, skip the declare blockers and combat damage steps."
///
/// [CR 508.8]: https://yawgatog.com/resources/magic-rules/#R5088
fn has_attackers<L: Layout>(s: &State<L>) -> bool {
    s.turn().attacked() || any_attacking(s)
}

/// Whether `step` happens only in a combat that has attackers ([`has_attackers`]).
fn needs_attackers(step: TurnStep) -> bool {
    matches!(
        step,
        TurnStep::DeclareBlockers | TurnStep::FirstStrikeDamage | TurnStep::CombatDamage
    )
}

/// Begins the next turn: the starting player's first, then alternating
/// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)): "The starting player takes
/// their first turn."
pub(crate) fn begin_turn<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if s.turns_for_limit() >= s.profile().limits.turns {
        s.end_game(Outcome::Stalled(StallReason::Turns));
        return Ok(());
    }
    if t.replaces_any().replaces(ReplacementKind::BeginTurn)
        && let Some(question) = turns::choose(s, t)?
    {
        s.pause_step_actions(Default::default(), question, true);
        return Ok(());
    }
    take_turn(s, t, sink, &[])
}

/// Applies each recorded replacement of [`begin_turn`] before beginning the first unskipped
/// turn, when the replacements' choices are replayed ([`paused_step`]).
/// See [CR 614.10b]: "That action is considered to be the first thing that happens during the
/// next step, phase, or turn to actually occur." Keep the untaps in the order chosen and
/// perform them as the turn begins, before its untap step.
///
/// [CR 614.10b]: https://yawgatog.com/resources/magic-rules/#R61410b
fn replay_turn_replacements<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let mut untaps = <L::Queue<_, 32>>::default();
    while let Some(choice) = turns::choose(s, t)? {
        let answer = replacement::answer(s, sink, &choice)?;
        let Some(Opt::Object(slot)) = choice.candidates.option(answer) else {
            break;
        };
        untaps.push(s.object_ref(slot))?;
        s.skip_turn();
    }
    take_turn(s, t, sink, untaps.as_slice())
}

/// Begins a turn that survived its replacements, carrying out any actions they deferred
/// to this turn before its first step (see CR 614.10b).
fn take_turn<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    untaps: &[ObjectRef<L>],
) -> Result<(), Full> {
    if !s.delayed().is_empty() {
        crate::stack::delayed_triggers::end_turn(s, t);
    }
    let active = s.begin_turn()?;
    // See CR 611.2a: an effect lasts "as long as stated by the spell or ability creating it";
    // one that lasts until a player's next turn ends as that turn begins.
    s.expire(Duration::UntilYourNextTurn(active));
    layers::refresh(s, t, sink)?;
    // See CR 302.6: the active player's permanents have now been under their control since their
    // most recent turn began.
    s.reset_permanents_for_turn(active);
    for &object in untaps {
        let Some(permanent) = s.permanent(object) else {
            continue;
        };
        sink.enter_source(|| object);
        let result = event::set_tapped(s, t, sink, permanent, false);
        sink.leave();
        result?;
    }
    if t.can(Capability::TurnStructure) {
        return begin_planned_turn(s, t, sink);
    }
    begin_step(s, t, sink, TurnStep::Untap)
}

/// Begins a step: its turn-based actions, then priority for the active player.
///
/// See [CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a): "The active player
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
    step_actions(s, t, sink)
}

/// The turn-based actions of the step that has just begun, as the draw step's draw
/// ([CR 703.4d]): "Immediately after
/// the draw step begins, the active player draws a card."
///
/// [CR 703.4d]: https://yawgatog.com/resources/magic-rules/#R7034d
#[inline]
pub(crate) fn step_actions<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let step = s.turn().step();
    let active = s.turn().active();
    match step {
        TurnStep::Untap => {
            untap(s, t, sink)?;
            // CR 502.4: "No player receives priority during the untap step".
            return end_step(s, t, sink);
        }
        TurnStep::Draw => {
            // See CR 103.8a: "In a two-player game, the player who plays first skips the draw
            // step (see rule 504, "Draw Step") of their first turn."
            let skip = match t.format().first_draw() {
                FirstDraw::SkippedByStartingPlayer => s.turn().is_starting_players_first_turn(),
            };
            if !skip || s.has_turn_plan() {
                // CR 504.1: "First, the active player draws a card."
                drawing::draw(&mut Game::new(s, t, sink), active, 1)?;
            }
        }
        // CR 703.4f: "Immediately after a player’s precombat main phase begins, that player
        // puts a lore counter on each Saga enchantment they control with one or more chapter
        // abilities."
        TurnStep::PrecombatMain if t.can(Capability::Saga) => {
            crate::saga::advance(s, t, sink, active)?;
        }
        TurnStep::DeclareAttackers => combat::begin_attackers(s, t),
        TurnStep::DeclareBlockers => combat::begin_blockers(s, t),
        TurnStep::FirstStrikeDamage | TurnStep::CombatDamage => {
            combat::begin_damage(s, t, sink)?;
        }
        TurnStep::Cleanup => cleanup(s, t),
        TurnStep::Upkeep
        | TurnStep::PrecombatMain
        | TurnStep::BeginningOfCombat
        | TurnStep::EndOfCombat
        | TurnStep::PostcombatMain
        | TurnStep::End => {}
    }
    Ok(())
}

/// Untaps the active player's permanents
/// ([CR 502.3](https://yawgatog.com/resources/magic-rules/#R5023)): "the active player
/// determines which permanents they control will untap. Then they untap them all
/// simultaneously." Their events are kept as one group. The rule also says "effects can
/// keep one or more of a player's permanents from untapping".
fn untap<L: Layout>(s: &mut State<L>, t: &GameTable, sink: &mut Sink<L>) -> Result<(), Full> {
    phasing::untap_step(s, t, sink)?;
    sink.together();
    let untapped = if t.rules_of(RuleKind::StaysTapped).is_empty() {
        untap_all::<L, false>(s, t, sink)
    } else {
        untap_all::<L, true>(s, t, sink)
    };
    sink.apart();
    untapped
}

/// [`untap`], inside the group it opens.
#[inline]
fn untap_all<L: Layout, const RESTRICTED: bool>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let active = s.turn().active();
    let mut i = 0;
    while i < s.battlefield().len() {
        let slot = s.battlefield()[i];
        if s.controller(slot) == active
            && let Some(permanent) = s.permanent_at(slot)
            && (!RESTRICTED || !rule_change::own_rule_holds(s, t, slot, RuleKind::StaysTapped))
        {
            event::set_tapped(s, t, sink, permanent, false)?;
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
    if let Some(next) = next_step(s, t, sink)? {
        begin_step(s, t, sink, next)
    } else {
        begin_turn(s, t, sink)
    }
}

/// [`end_step`] up to the turn-based actions of the step it begins ([`step_actions`]).
/// Returns whether the caller still has step actions to run.
pub(crate) fn end_step_before_actions<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<bool, Full> {
    let Some(next) = next_step(s, t, sink)? else {
        begin_turn(s, t, sink)?;
        return Ok(false);
    };
    let began = s.begin_step(next);
    event::settle(s, t, sink, began)?;
    Ok(true)
}

/// Ends the current step, returning the step that begins next, if any remain this turn.
#[inline]
fn next_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<Option<TurnStep>, Full> {
    s.empty_mana_pools();
    if t.can(Capability::TurnStructure) {
        return planned_next(s, t, sink);
    }
    let next = match s.turn().step() {
        TurnStep::Untap => TurnStep::Upkeep,
        TurnStep::Upkeep => TurnStep::Draw,
        TurnStep::Draw => TurnStep::PrecombatMain,
        TurnStep::PrecombatMain => TurnStep::BeginningOfCombat,
        TurnStep::BeginningOfCombat => TurnStep::DeclareAttackers,
        TurnStep::DeclareAttackers if has_attackers(s) => TurnStep::DeclareBlockers,
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
            s.end_combat();
            TurnStep::PostcombatMain
        }
        TurnStep::PostcombatMain => TurnStep::End,
        // CR 514.3a: once players pass with an empty stack in a cleanup step where they
        // received priority, "another cleanup step begins."
        TurnStep::End => {
            s.reset_cleanup();
            TurnStep::Cleanup
        }
        TurnStep::Cleanup => {
            s.repeat_cleanup();
            TurnStep::Cleanup
        }
    };
    Ok(Some(next))
}

/// Begins a turn whose steps can change. The starting player's first draw step is skipped
/// (see [CR 103.8a]: "the player who plays first skips the draw step").
///
/// [CR 103.8a]: https://yawgatog.com/resources/magic-rules/#R1038a
#[cold]
#[inline(never)]
fn begin_planned_turn<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    s.plan_turn(t.format())?;
    if s.turn().is_starting_players_first_turn() {
        let _skipped = s.remove_step(TurnStep::Draw);
    }
    let step = advance_plan(s, t, sink)?.expect("a new turn has a step");
    begin_step(s, t, sink, step)
}

/// Applies conditional combat steps within the current phase and advances the plan: a combat
/// without attackers skips the steps that need them ([`has_attackers`]), and a first-strike
/// damage step happens only when a creature would deal damage in it (see [CR 510.4]): "the
/// only creatures that assign combat damage in that step are those with first strike or double
/// strike".
///
/// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
#[cold]
#[inline(never)]
fn planned_next<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<Option<TurnStep>, Full> {
    if !s.has_turn_plan() {
        s.plan_from_current_step(t.format())?;
    }
    let step = s.turn().step();
    if step == TurnStep::Cleanup {
        if matches!(s.planned_next_step(), Some((TurnStep::Cleanup, _))) {
            return Ok(s.take_planned_step());
        }
        s.repeat_cleanup();
        return Ok(Some(TurnStep::Cleanup));
    }
    advance_plan(s, t, sink)
}

/// Finds the next step allowed by the plan and the combat rules.
fn advance_plan<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<Option<TurnStep>, Full> {
    let mut still_in_combat = combat::in_combat(s.turn().step());
    while let Some((next, starts_phase)) = s.planned_next_step() {
        if still_in_combat && starts_phase {
            s.end_combat();
            still_in_combat = false;
        }
        let mut skip = needs_attackers(next) && !has_attackers(s);
        if next == TurnStep::FirstStrikeDamage && !skip {
            layers::refresh(s, t, sink)?;
            skip = !combat::first_strike_step(s);
        }
        if !skip && !rule_change::skips_step(s, t, next) {
            return Ok(s.take_planned_step());
        }
        let _skipped = s.remove_step(next);
    }
    if still_in_combat {
        s.end_combat();
    }
    Ok(None)
}

/// Adds the step immediately after the one being played (see [CR 500.9]):
/// "Some effects can add steps to a phase. They do this by adding the steps
/// directly after a specified step or directly before a specified step."
///
/// [CR 500.9]: https://yawgatog.com/resources/magic-rules/#R5009
pub(crate) fn additional_step<L: Layout>(
    game: &mut Game<'_, L>,
    step: TurnStep,
) -> Result<Instructed<L>, Full> {
    let current = game.state.turn().step();
    let phase = TurnPhase::of(current);
    if !game.state.has_turn_plan() || phase == TurnPhase::Main || phase != TurnPhase::of(step) {
        return Ok(Instructed::NOTHING);
    }
    game.insert_step(current, Placement::After, step)?;
    Instructed::of(Bitboard::EMPTY, 1)
}

/// Adds the named phases after the current phase (see [CR 500.8]): "They do this by adding
/// the phases directly after the specified phase." If the current phase does not match the
/// instruction's anchor, there is no specified phase to add them after.
///
/// [CR 500.8]: https://yawgatog.com/resources/magic-rules/#R5008
pub(crate) fn additional_phases<L: Layout>(
    game: &mut Game<'_, L>,
    after: TurnPhase,
    phases: &[TurnPhase],
) -> Result<Instructed<L>, Full> {
    if !game.state.has_turn_plan() {
        if phases.is_empty() || TurnPhase::of(game.state.turn().step()) != after {
            return Ok(Instructed::NOTHING);
        }
        game.state.plan_from_current_step(game.table.format())?;
    }
    if !game.state.insert_phases_after_current(after, phases)? {
        return Ok(Instructed::NOTHING);
    }
    for &phase in phases {
        game.sink
            .note(Event::TurnPlanChanged(TurnChange::InsertPhase(phase)));
    }
    Instructed::of(
        Bitboard::EMPTY,
        u32::try_from(phases.len()).expect("the turn plan fits in a u32"),
    )
}

/// Continues after a cleanup that needed no priority (see CR 514.3a).
#[cold]
#[inline(never)]
pub(crate) fn finish_planned_cleanup<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if let Some(step) = advance_plan(s, t, sink)? {
        s.empty_mana_pools();
        begin_step(s, t, sink, step)
    } else {
        begin_turn(s, t, sink)
    }
}

/// The cleanup step's first turn-based action
/// ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)): "if the active player's
/// hand contains more cards than their maximum hand size (normally seven), they discard enough
/// cards to reduce their hand size to that number." The discards are decisions, one card at a
/// time; [`finish_cleanup`] follows them.
fn cleanup<L: Layout>(s: &mut State<L>, t: &GameTable) {
    if cleanup_discards(s, t) > 0 {
        s.ask(Pending::Discard);
    } else {
        finish_cleanup(s);
    }
}

/// How many cards the active player still has to discard in the cleanup step (CR 514.1): the
/// cards in their hand over their maximum hand size.
pub(crate) fn cleanup_discards<L: Layout>(s: &State<L>, t: &GameTable) -> u32 {
    let active = s.turn().active();
    // Excess cards to discard in cleanup; a hand at or below the maximum discards 0.
    s.hand_size(active)
        .saturating_sub(rule_change::maximum_hand(s, t, active))
}

/// Discards one card in the cleanup step, finishing the step once the hand is small enough.
///
/// See [CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a): "To discard a card, move
/// it from its owner's hand to that player's graveyard."
pub(crate) fn cleanup_discard<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
) -> Result<(), Full> {
    // A card with no owner is in a shared library or graveyard, not in a hand, so it
    // cannot be discarded.
    let Some(discarder) = s.object(slot).owner() else {
        return Ok(());
    };
    event::move_one_by(
        s,
        t,
        sink,
        discarder,
        slot,
        Zone::Graveyard,
        Reason::Discard,
    )?;
    if cleanup_discards(s, t) == 0 {
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
/// See [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
/// may play a land card from their hand during a main phase of their turn when the stack is
/// empty."
///
/// [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can normally play
/// one land during their turn".
pub(crate) fn may_play_land<L: Layout>(s: &State<L>, player: Seat, format: Format) -> bool {
    player == s.turn().active()
        && matches!(
            s.turn().step(),
            TurnStep::PrecombatMain | TurnStep::PostcombatMain
        )
        && s.stack().is_empty()
        && s.player(player).lands_played() < format.lands_each_turn()
}

/// The cards in a player's hand.
pub(crate) fn hand<L: Layout>(s: &State<L>, player: Seat) -> Bitboard<L::Slot> {
    s.zone(ZoneKey::Of(player, Zone::Hand))
}
