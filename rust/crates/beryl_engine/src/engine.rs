//! The engine loop: `next` advances to the next decision, `apply` consumes one answer
//! (architecture §3, §7).

use crate::bitboard::Bitboard;
use crate::card::Types;
use crate::decision::{
    Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, StallReason, Step,
};
use crate::filter::Filter;
use crate::fixed::{Fixed, Full};
use crate::state::{Object, Pending, State, opponent};
use crate::table::GameTable;
use crate::{sba, start, turn, zone};

/// Per-thread working memory, never copied with the state. Candidate buffers, the mana
/// solver's arrays, the event buffer and the spare state for transactional replay arrive with
/// the work that needs them.
#[derive(Debug, Default)]
pub struct Scratch {
    pub counts: Counts,
}

impl Scratch {
    pub fn new() -> Self {
        Self::default()
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
pub fn next(s: &mut State, t: &GameTable, x: &mut Scratch, _a: &mut dyn Answers) -> Step {
    loop {
        if let Some(outcome) = s.outcome {
            return Step::Over(outcome);
        }
        if s.pending == Pending::None {
            // CR 117.5: "Each time a player would get priority, the game first performs all
            // applicable state-based actions as a single event".
            x.counts.state_based_checks += 1;
            if sba::check(s, t) {
                s.turn.cleanup_check = false;
                continue;
            }
            if s.turn.cleanup_check {
                s.turn.cleanup_check = false;
                // CR 514.3a: with no state-based actions and no waiting triggers, no player
                // receives priority and the turn ends.
                if s.triggers.is_empty() {
                    if turn::begin_turn(s).is_err() {
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
            perform_or_stall(s, t, &d, Choice(0));
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
pub fn apply(s: &mut State, t: &GameTable, _x: &mut Scratch, _a: &mut dyn Answers, c: Choice) {
    debug_assert!(s.outcome.is_none(), "apply after the game ended");
    match decision(s, t) {
        Ok(d) => {
            debug_assert!(usize::from(c.0) < d.len(), "choice out of range");
            perform_or_stall(s, t, &d, c);
        }
        Err(Full) => stall(s, StallReason::Capacity),
    }
}

#[cold]
fn stall(s: &mut State, reason: StallReason) {
    s.outcome = Some(Outcome::Stalled(reason));
}

fn perform_or_stall(s: &mut State, t: &GameTable, d: &Decision, c: Choice) {
    if perform(s, t, d, c).is_err() {
        stall(s, StallReason::Capacity);
    }
}

/// The open decision, a pure function of the state, so `apply` recomputes rather than
/// remembers it.
fn decision(s: &State, t: &GameTable) -> Result<Decision, Full> {
    Ok(match s.pending {
        Pending::None => priority_decision(s, t)?,
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

fn perform(s: &mut State, t: &GameTable, d: &Decision, c: Choice) -> Result<(), Full> {
    let object = || d.candidates.object(c).expect("an object choice");
    let option = || d.candidates.option(c).expect("an option choice");
    match d.kind {
        DecisionKind::Priority => match option() {
            Opt::PlayLand(slot) => zone::play_land(s, d.player, slot),
            _ => pass(s),
        },
        DecisionKind::PlayOrDraw => {
            let starting = if option() == Opt::Play {
                d.player
            } else {
                opponent(d.player)
            };
            start::set_starting_player(s, starting)
        }
        DecisionKind::KeepOrMulligan => start::declare(s, d.player, option() == Opt::Mulligan),
        DecisionKind::PutOnBottom => start::put_on_bottom(s, d.player, object()),
        DecisionKind::Discard => turn::cleanup_discard(s, object()),
        DecisionKind::LegendRule => {
            let Pending::LegendRule { mut kept } = s.pending else {
                unreachable!("a legend-rule decision without its pending state");
            };
            kept.insert(object());
            if sba::unresolved_legends(s, t, kept).is_some() {
                s.pending = Pending::LegendRule { kept };
                Ok(())
            } else {
                sba::apply_legend_rule(s, t, kept)
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
/// step ends." Nothing can be on the stack until casting arrives.
fn pass(s: &mut State) -> Result<(), Full> {
    s.turn.passes += 1;
    if s.turn.passes < 2 {
        s.turn.priority = opponent(s.turn.priority);
        return Ok(());
    }
    debug_assert!(s.stack.is_empty());
    turn::end_step(s)
}

/// Collapses interchangeable objects to one candidate each, the lowest slot (§7.2): objects
/// that differ only in their incarnation and timestamp lead to the same game. Everything
/// else counts: the face up, the owner (a stolen Mana Crypt goes to another graveyard than
/// your own), copy, link and chosen value as much as card, counters and flags.
fn collapse(s: &State, candidates: Bitboard) -> Bitboard {
    let signature = |slot: u8| Object {
        incarnation: 0,
        timestamp: 0,
        ..*s.object(slot)
    };
    let mut kept = Bitboard::EMPTY;
    for slot in candidates.iter() {
        let sig = signature(slot);
        if !kept.iter().any(|k| signature(k) == sig) {
            kept.insert(slot);
        }
    }
    kept
}
