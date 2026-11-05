//! The engine loop: `next` advances to the next decision, `apply` consumes one answer
//! (architecture §3, §7).

use crate::bitboard::Bitboard;
use crate::card::Types;
use crate::decision::{
    Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, StallReason, Step,
};
use crate::event::{self, Entry, Reason, Sink};
use crate::filter::Filter;
use crate::fixed::{Fixed, Full};
use crate::state::{Pending, State, Zone, opponent};
use crate::table::GameTable;
use crate::{combat, layers, sba, start, trigger, turn, zone};

/// Per-thread working memory, never copied with the state: the event sink. Candidate
/// buffers, the mana solver's arrays and the spare state for transactional replay arrive
/// with the work that needs them.
#[derive(Debug, Default)]
pub struct Scratch {
    pub counts: Counts,
    pub events: Sink,
}

impl Scratch {
    /// Working memory whose sink drops events, as search wants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Working memory whose sink records every executed event, for the UI and replays.
    pub fn recording() -> Self {
        Self {
            events: Sink::recording(),
            ..Self::default()
        }
    }
}

/// Deterministic counts of the engine's logical operations, the stable performance signal CI
/// gates on with tolerance bands (architecture §17.4), summed over every game the scratch
/// serves. State copies, layer recomputes, solver runs and events join them with the work
/// that does them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Decisions `next` returned.
    pub decisions: u64,
    /// Decisions `next` settled itself: one candidate, or a priority pass away from a stop.
    pub silent: u64,
    /// State-based action checks.
    pub state_based_checks: u64,
}

/// Advances the game until a player must decide or the game is over.
///
/// What the exact profile settles itself (§7.2): a decision with one candidate is applied
/// silently, so a player whose only legal action is to pass passes silently. Under smart stops
/// a player away from a stop passes silently too (§15).
///
/// [CR 704.3](https://yawgatog.com/resources/magic-rules/#R7043): "If any state-based actions
/// are performed as a result of a check, the check is repeated; otherwise all triggered
/// abilities that are waiting to be put on the stack are put on the stack, then the check is
/// repeated." Waiting triggers come before priority as decisions of their own, one pick at a
/// time ([`DecisionKind::TriggerOrder`]).
pub fn next(s: &mut State, t: &GameTable, x: &mut Scratch, _a: &mut dyn Answers) -> Step {
    let sink = &mut x.events;
    loop {
        if let Some(outcome) = s.outcome {
            return Step::Over(outcome);
        }
        layers::refresh(s, t);
        if combat::in_combat(s.turn.step) {
            combat::prune(s);
        }
        if s.pending == Pending::None {
            // CR 117.5: "Each time a player would get priority, the game first performs all
            // applicable state-based actions as a single event".
            x.counts.state_based_checks += 1;
            match sba::check(s, t, sink) {
                Ok(true) => {
                    s.turn.cleanup_check = false;
                    continue;
                }
                Ok(false) => {}
                Err(Full) => {
                    stall(s, StallReason::Capacity);
                    continue;
                }
            }
            if s.turn.cleanup_check {
                s.turn.cleanup_check = false;
                // CR 514.3a: with no state-based actions and no waiting triggers, no player
                // receives priority and the turn ends.
                if s.triggers.is_empty() {
                    if turn::begin_turn(s, t, sink).is_err() {
                        stall(s, StallReason::Capacity);
                    }
                    continue;
                }
            }
        }
        let Ok(d) = decision(s, t) else {
            stall(s, StallReason::Capacity);
            continue;
        };
        debug_assert!(!d.is_empty());
        let priority = d.kind == DecisionKind::Priority;
        let silent = (d.len() == 1 || (priority && !s.profile.stops_for(s, d.player)))
            && !(priority && s.profile.breaks_at(s.turn));
        if silent {
            x.counts.silent += 1;
            perform_or_stall(s, t, sink, &d, Choice(0));
            continue;
        }
        if s.budgets.decisions >= s.profile.limits.decisions_per_turn {
            stall(s, StallReason::DecisionsPerTurn);
            continue;
        }
        s.budgets.decisions += 1;
        x.counts.decisions += 1;
        return Step::Decide(d);
    }
}

/// Applies an answer to the decision `next` returned for this state.
pub fn apply(s: &mut State, t: &GameTable, x: &mut Scratch, _a: &mut dyn Answers, c: Choice) {
    debug_assert!(s.outcome.is_none(), "apply after the game ended");
    layers::refresh(s, t);
    match decision(s, t) {
        Ok(d) => {
            debug_assert!(usize::from(c.0) < d.len(), "choice out of range");
            perform_or_stall(s, t, &mut x.events, &d, c);
        }
        Err(Full) => stall(s, StallReason::Capacity),
    }
}

#[cold]
fn stall(s: &mut State, reason: StallReason) {
    s.outcome = Some(Outcome::Stalled(reason));
}

fn perform_or_stall(s: &mut State, t: &GameTable, sink: &mut Sink, d: &Decision, c: Choice) {
    if perform(s, t, sink, d, c).is_err() {
        stall(s, StallReason::Capacity);
    }
}

/// The open decision, a pure function of the state, so `apply` recomputes rather than
/// remembers it.
fn decision(s: &State, t: &GameTable) -> Result<Decision, Full> {
    Ok(match s.pending {
        Pending::None => match trigger::next_to_stack(s) {
            Some((player, triggers)) => {
                let mut list = Fixed::new();
                for i in triggers.iter() {
                    list.push(Opt::Trigger(i))?;
                }
                Decision {
                    player,
                    kind: DecisionKind::TriggerOrder,
                    candidates: Candidates::Options(list),
                }
            }
            None => priority_decision(s, t)?,
        },
        Pending::PlayOrDraw { chooser } => {
            options(chooser, DecisionKind::PlayOrDraw, &[Opt::Play, Opt::Draw])?
        }
        Pending::Mulligans => match start::mulligan_question(s) {
            (p, true) => objects(p, DecisionKind::PutOnBottom, collapse(s, turn::hand(s, p))),
            (p, false) => options(p, DecisionKind::KeepOrMulligan, &[Opt::Keep, Opt::Mulligan])?,
        },
        Pending::Discard => {
            let p = s.turn.active;
            objects(p, DecisionKind::Discard, collapse(s, turn::hand(s, p)))
        }
        Pending::LegendRule { kept } => {
            let (p, group) =
                sba::unresolved_legends(s, t, kept).expect("a legend-rule group without a keeper");
            objects(p, DecisionKind::LegendRule, collapse(s, group))
        }
        Pending::EnterPayingLife { slot, .. } => options(
            s.object(slot).owner,
            DecisionKind::PayLife,
            &[Opt::Yes, Opt::No],
        )?,
        Pending::DeclareAttackers { attacker } => combat::attack_decision(s, attacker)?,
        Pending::DeclareBlockers { blocker } => combat::block_decision(s, blocker)?,
        Pending::CombatDamage { done, lethal } => combat::division_decision(s, done, lethal)?,
    })
}

fn objects(player: u8, kind: DecisionKind, candidates: Bitboard) -> Decision {
    Decision {
        player,
        kind,
        candidates: Candidates::Objects(candidates),
    }
}

fn options(player: u8, kind: DecisionKind, opts: &[Opt]) -> Result<Decision, Full> {
    let mut list = Fixed::new();
    for &o in opts {
        list.push(o)?;
    }
    Ok(Decision {
        player,
        kind,
        candidates: Candidates::Options(list),
    })
}

/// The priority holder's legal actions, passing first.
///
/// [CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171): "The player with priority
/// may cast spells, activate abilities, and take special actions."
fn priority_decision(s: &State, t: &GameTable) -> Result<Decision, Full> {
    let player = s.turn.priority;
    let mut list = Fixed::new();
    list.push(Opt::Pass)?;
    if turn::may_play_land(s, player) {
        let lands = Filter::Types(Types::LAND).select(s, t, turn::hand(s, player), player);
        for slot in collapse(s, lands).iter() {
            list.push(Opt::PlayLand(slot))?;
        }
    }
    Ok(Decision {
        player,
        kind: DecisionKind::Priority,
        candidates: Candidates::Options(list),
    })
}

fn perform(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    d: &Decision,
    c: Choice,
) -> Result<(), Full> {
    let object = || d.candidates.object(c).expect("an object choice");
    let option = || d.candidates.option(c).expect("an option choice");
    match d.kind {
        DecisionKind::Priority => match option() {
            Opt::PlayLand(slot) => play_land(s, t, sink, d.player, slot),
            _ => pass(s, t, sink),
        },
        DecisionKind::TriggerOrder => match option() {
            Opt::Trigger(i) => trigger::put_on_stack(s, i),
            _ => unreachable!("a trigger-order decision offers only triggers"),
        },
        DecisionKind::PayLife => {
            let Pending::EnterPayingLife { slot, .. } = s.pending else {
                unreachable!("a pay-life decision without its pending state");
            };
            s.pending = Pending::None;
            enter_from_hand(s, t, sink, slot, option() == Opt::Yes)
        }
        DecisionKind::Attacker | DecisionKind::AttackTarget => combat::attack(s, t, sink, option()),
        DecisionKind::Blocker => {
            match option() {
                Opt::Block(slot) => combat::block_with(s, slot),
                _ => combat::finish_blocks(s),
            }
            Ok(())
        }
        DecisionKind::BlockTarget => {
            combat::block_attacker(s, object());
            Ok(())
        }
        DecisionKind::DamageDivision => {
            let Pending::CombatDamage { done, lethal } = s.pending else {
                unreachable!("a damage division without its pending state");
            };
            combat::divide(s, t, sink, done, lethal, option())
        }
        DecisionKind::PlayOrDraw => {
            let starting = if option() == Opt::Play {
                d.player
            } else {
                opponent(d.player)
            };
            start::set_starting_player(s, t, sink, starting)
        }
        DecisionKind::KeepOrMulligan => {
            start::declare(s, t, sink, d.player, option() == Opt::Mulligan)
        }
        DecisionKind::PutOnBottom => start::put_on_bottom(s, t, sink, d.player, object()),
        DecisionKind::Discard => turn::cleanup_discard(s, t, sink, object()),
        DecisionKind::LegendRule => {
            let Pending::LegendRule { mut kept } = s.pending else {
                unreachable!("a legend-rule decision without its pending state");
            };
            kept.insert(object());
            if sba::unresolved_legends(s, t, kept).is_some() {
                s.pending = Pending::LegendRule { kept };
                Ok(())
            } else {
                sba::apply_legend_rule(s, t, sink, kept)
            }
        }
    }
}

/// Passes priority.
///
/// [CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d): "If a player has priority
/// and chooses not to take any actions, that player passes. ... Then the next player in turn
/// order receives priority."
///
/// [CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174): "If all players pass in
/// succession (that is, if all players pass without taking any actions in between passing),
/// the spell or ability on top of the stack resolves or, if the stack is empty, the phase or
/// step ends."
///
/// [CR 117.3b](https://yawgatog.com/resources/magic-rules/#R1173b): "The active player
/// receives priority after a spell or ability (other than a mana ability) resolves."
fn pass(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<(), Full> {
    s.turn.passes += 1;
    if s.turn.passes < 2 {
        s.turn.priority = opponent(s.turn.priority);
        return Ok(());
    }
    let Some(top) = s.stack.pop() else {
        return turn::end_step(s, t, sink);
    };
    s.turn.passes = 0;
    s.turn.priority = s.turn.active;
    trigger::resolve(s, t, sink, top)
}

/// Plays a land: a special action that puts it onto the battlefield. A land that enters
/// tapped unless its controller pays life asks them first.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "Playing a land is a
/// special action; it doesn't use the stack (see rule 116). Rather, the player simply puts the
/// land onto the battlefield."
///
/// [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
///
/// [CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194): "If a cost or effect allows
/// a player to pay an amount of life greater than 0, the player may do so only if their life
/// total is greater than or equal to the amount of the payment."
fn play_land(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: u8,
    slot: u8,
) -> Result<(), Full> {
    s.player_mut(player).lands_played += 1;
    s.turn.priority = player;
    s.turn.passes = 0;
    let o = s.object(slot);
    match event::life_to_enter(t, o.card, o.face) {
        Some(life) if s.player(player).life >= i16::from(life) => {
            s.pending = Pending::EnterPayingLife { slot, life };
            Ok(())
        }
        _ => enter_from_hand(s, t, sink, slot, false),
    }
}

fn enter_from_hand(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    slot: u8,
    pay_life: bool,
) -> Result<(), Full> {
    event::move_batch(
        s,
        t,
        sink,
        Bitboard::single(slot),
        Zone::Battlefield,
        Reason::Play,
        Entry {
            pay_life,
            ..Entry::default()
        },
    )
}

/// Collapses interchangeable objects to one candidate each, the lowest slot (§7.2): objects
/// that differ only in their incarnation and timestamp lead to the same game. Everything
/// else counts: the face up, the owner (a stolen Mana Crypt goes to another graveyard than
/// your own), copy, link and chosen value as much as card, counters and flags.
pub(crate) fn collapse(s: &State, candidates: Bitboard) -> Bitboard {
    let signature = |slot: u8| zone::signature(s, slot);
    let mut kept = Bitboard::EMPTY;
    for slot in candidates.iter() {
        let sig = signature(slot);
        if !kept.iter().any(|k| signature(k) == sig) {
            kept.insert(slot);
        }
    }
    kept
}
