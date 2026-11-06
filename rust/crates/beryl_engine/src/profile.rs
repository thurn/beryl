//! Restriction profiles: which decisions are offered, and the budgets (architecture §12.3,
//! §15).

use crate::combat;
use crate::decision::{Decision, DecisionKind};
use crate::ids::Seat;
use crate::observation::Observation;
use crate::state::{Pending, State, Turn, TurnStep};
use crate::turn::any_attacking;

/// Where players get priority as a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stops {
    /// Every priority with a legal action is a decision (§7.2).
    Exact,
    /// Priority only at the smart stops (§15, decision D18); elsewhere both players pass
    /// silently.
    Smart,
    /// As `Exact`, and a question with one answer is a decision too rather than answered
    /// silently, for a caller that records every answer: the scenario trace, whose oracles
    /// must script the answers Beryl takes for granted (§17.2). Passing priority when it is
    /// all a player can do stays silent.
    Forced,
}

/// The budgets whose exhaustion is `Outcome::Stalled` (architecture §12.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    pub decisions_per_turn: u16,
    pub turns: u16,
    pub state_based_iterations: u8,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        decisions_per_turn: 500,
        turns: 200,
        state_based_iterations: 32,
    };
}

/// A turn and step where the active player's first priority is a decision even when passing
/// is all they can do, so a caller can stop the game where the engine would otherwise pass
/// silently. The scenario runner uses one to advance to a step (architecture §17.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Breakpoint {
    pub turn: u16,
    pub step: TurnStep,
}

/// How mana payments are chosen (architecture §10, decision D7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Payment {
    /// Among plans that differ only in which sources are tapped, the one that keeps the most
    /// of the player's other costs payable; plans that differ in what they consume are a
    /// decision. For tree nodes and the exact profile.
    KeepMostPayable,
    /// Tap the least flexible sources that pay, and offer nothing. For rollouts.
    Greedy,
    /// Every tap and every floating mana spent is a decision, and mana abilities may be
    /// activated on their own: the `manual_mana` flag, for a human who wants it.
    Manual,
}

/// A restriction of the decision surface plus budgets. Every profile is sound: any line it
/// produces is a legal line of the real game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Profile {
    pub stops: Stops,
    pub limits: Limits,
    pub breakpoint: Option<Breakpoint>,
    pub payment: Payment,
    /// The player, if any, whose every priority is a decision, even when passing is all they
    /// can do: a human under full control, as on Arena, who sees each step and each spell
    /// they receive priority over. A player receives priority whether or not they can act;
    /// for everyone else the engine answers a lone pass silently (§7.2).
    ///
    /// - [CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a): "The active player
    ///   receives priority at the beginning of most steps and phases".
    /// - [CR 117.3c](https://yawgatog.com/resources/magic-rules/#R1173c): "If a player has
    ///   priority when they cast a spell, activate an ability, or take a special action, that
    ///   player receives priority afterward."
    /// - [CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d): "If a player has
    ///   priority and chooses not to take any actions, that player passes. ... the next
    ///   player in turn order receives priority."
    pub full_control: Option<Seat>,
    /// The player, if any, whose target for a spell they cast or an ability they activate is
    /// a decision even when only one is legal: a human, who chooses it on the board, as an
    /// Arena player clicks it, and may still take the cast back rather than aim it at their
    /// own permanent. The engine otherwise announces a lone legal target silently (§7.2).
    ///
    /// - [CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "The player
    ///   announces their choice of an appropriate object or player for each target the spell
    ///   requires."
    /// - [CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012): if the casting cannot
    ///   be completed, "the casting of the spell is illegal; the game returns to the moment
    ///   before the casting of that spell was proposed".
    pub asks_targets: Option<Seat>,
}

impl Profile {
    /// Everything the rules offer, less what §7.2 settles silently.
    pub const EXACT: Profile = Profile {
        stops: Stops::Exact,
        limits: Limits::DEFAULT,
        breakpoint: None,
        payment: Payment::KeepMostPayable,
        full_control: None,
        asks_targets: None,
    };
    /// The smart stops, for search and, by default, for humans.
    pub const SMART: Profile = Profile {
        stops: Stops::Smart,
        limits: Limits::DEFAULT,
        breakpoint: None,
        payment: Payment::KeepMostPayable,
        full_control: None,
        asks_targets: None,
    };

    /// Whether `player`'s priority now is a decision, given they have a legal action.
    pub(crate) fn stops_for(self, s: &State, player: Seat) -> bool {
        match self.stops {
            Stops::Exact | Stops::Forced => true,
            Stops::Smart => is_smart_stop(s, player),
        }
    }

    /// Whether `player`'s priority now is a decision even when passing is all they can do: at
    /// the breakpoint, or under their full control.
    pub(crate) fn asks_pass(self, turn: Turn, player: Seat) -> bool {
        self.breaks_at(turn) || self.full_control == Some(player)
    }

    /// Whether `d`, a decision with one candidate, is still asked: a target of a spell its
    /// player casts or an ability they activate, while [`Profile::asks_targets`] names them.
    /// A triggered ability's lone target stays silent, as nothing can take it back
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)).
    pub(crate) fn asks_lone(self, s: &State, d: &Decision) -> bool {
        d.kind == DecisionKind::Target
            && self.asks_targets == Some(d.player)
            && matches!(s.pending, Pending::Casting(c) if !c.triggered)
    }

    /// Whether the open priority is the active player's first in the breakpoint's step.
    pub(crate) fn breaks_at(self, turn: Turn) -> bool {
        self.breakpoint.is_some_and(|b| {
            b.turn == turn.number
                && b.step == turn.step
                && turn.priority == turn.active
                && turn.passes == 0
        })
    }
}

/// The smart stops (§15): "their own main phases, after attackers and after blockers are
/// declared, the opponent's end step, and whenever the stack is non-empty."
fn is_smart_stop(s: &State, player: Seat) -> bool {
    !s.stack.is_empty() || smart_stop_in(s.turn.step, s.turn.active == player, any_attacking(s))
}

/// Whether a player gets priority at a smart stop in `step` with an empty stack, as the
/// active player or not, with creatures attacking or not.
fn smart_stop_in(step: TurnStep, active: bool, attacking: bool) -> bool {
    match step {
        TurnStep::PrecombatMain | TurnStep::PostcombatMain => active,
        TurnStep::DeclareAttackers | TurnStep::DeclareBlockers => attacking,
        TurnStep::End => !active,
        _ => false,
    }
}

/// Where passing priority now takes the viewer, as the pass button says (decision D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NextStop {
    /// The top of the stack resolves.
    Resolve,
    /// Combat damage is dealt.
    CombatDamage,
    /// The viewer next gets priority, or declares attackers, in this step of the turn.
    Step(TurnStep),
    /// The turn ends.
    NextTurn,
}

/// Where passing priority now takes the viewer of `obs` under `stops`, if the other player
/// passes too: the resolution of the top of the stack; combat damage once blockers are
/// declared; under smart stops, the next step of the turn that is one of the viewer's
/// ([`is_smart_stop`]), counting the combat in which the viewer, as the active player, can
/// declare attackers; otherwise the next step.
pub fn next_stop(obs: Observation<'_>, stops: Stops) -> NextStop {
    let s = obs.shown();
    if !s.stack.is_empty() {
        return NextStop::Resolve;
    }
    let step = s.turn.step;
    let attacking = any_attacking(s);
    if attacking
        && matches!(
            step,
            TurnStep::DeclareBlockers | TurnStep::FirstStrikeDamage
        )
    {
        return NextStop::CombatDamage;
    }
    let later = TurnStep::ALL.into_iter().filter(|&l| l > step);
    if stops != Stops::Smart {
        return NextStop::Step(later.min().unwrap_or(TurnStep::Untap));
    }
    let active = s.turn.active == obs.viewer();
    let declares = active && step < TurnStep::BeginningOfCombat && combat::can_declare_attackers(s);
    for l in later {
        if (l == TurnStep::BeginningOfCombat && declares)
            || smart_stop_in(l, active, attacking || declares)
        {
            return NextStop::Step(l);
        }
    }
    NextStop::NextTurn
}
