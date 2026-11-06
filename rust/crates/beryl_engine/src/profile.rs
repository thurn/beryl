//! Restriction profiles: which decisions are offered, and the budgets (architecture §12.3,
//! §15).

use crate::combat;
use crate::decision::{Decision, DecisionKind};
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
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

/// The steps where a player gets priority as a decision under the smart stops while the stack
/// is empty, on their own turns and on their opponent's: the smart stops' steps by default
/// (§15), which a human may change, as an Arena player sets phase stops. A stop in the declare
/// attackers or declare blockers step holds only once creatures attack, as a player gets no
/// priority in a combat that has no attack to answer
/// ([CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088): "If no creatures are
/// declared as attackers or put onto the battlefield attacking, skip the declare blockers and
/// combat damage steps").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StepStops {
    /// One bit per step of the player's own turns, by [`TurnStep`] order.
    own: u16,
    /// One bit per step of the opponent's turns.
    theirs: u16,
}

impl StepStops {
    /// No step: priority only while the stack holds something.
    pub const NONE: StepStops = StepStops { own: 0, theirs: 0 };
    /// The smart stops' steps (§15): "their own main phases, after attackers and after
    /// blockers are declared, the opponent's end step".
    pub const SMART: StepStops = StepStops {
        own: bit(TurnStep::PrecombatMain)
            | bit(TurnStep::DeclareAttackers)
            | bit(TurnStep::DeclareBlockers)
            | bit(TurnStep::PostcombatMain),
        theirs: bit(TurnStep::DeclareAttackers)
            | bit(TurnStep::DeclareBlockers)
            | bit(TurnStep::End),
    };

    /// Whether the player stops in `step` of their own turn (`own`) or their opponent's.
    pub const fn has(self, step: TurnStep, own: bool) -> bool {
        (if own { self.own } else { self.theirs }) & bit(step) != 0
    }

    /// These stops, with `step` of the player's own turns (`own`) or their opponent's a stop
    /// or not (`on`).
    #[must_use]
    pub const fn with(self, step: TurnStep, own: bool, on: bool) -> StepStops {
        const fn set(bits: u16, step: TurnStep, on: bool) -> u16 {
            if on {
                bits | bit(step)
            } else {
                bits & !bit(step)
            }
        }
        if own {
            StepStops {
                own: set(self.own, step, on),
                theirs: self.theirs,
            }
        } else {
            StepStops {
                own: self.own,
                theirs: set(self.theirs, step, on),
            }
        }
    }

    /// Whether a player who is the active player (`active`) or not stops in `step` with the
    /// stack empty, with creatures attacking (`attacking`, asked only for a combat step).
    fn stops_in(self, step: TurnStep, active: bool, attacking: impl FnOnce() -> bool) -> bool {
        self.has(step, active)
            && (!matches!(step, TurnStep::DeclareAttackers | TurnStep::DeclareBlockers)
                || attacking())
    }
}

/// [`StepStops`]' bit for `step`.
const fn bit(step: TurnStep) -> u16 {
    1 << step as u16
}

/// The budgets whose exhaustion is `Outcome::Stalled` (architecture §12.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    pub decisions_per_turn: u16,
    pub turns: u16,
    pub state_based_iterations: u8,
}

impl Limits {
    /// The budgets of search and rollouts, which stop a runaway line.
    pub const DEFAULT: Limits = Limits {
        decisions_per_turn: 500,
        turns: 200,
        state_based_iterations: 32,
    };
    /// The budgets of a player game: decisions and turns as many as their counters hold, since
    /// a human repeating a loop by hand can always concede. State-based iterations keep the
    /// default: an endless mandatory loop is a rules draw, which needs a loop check rather than
    /// a bigger budget.
    ///
    /// [CR 104.4b](https://yawgatog.com/resources/magic-rules/#R1044b): "If a game that's not
    /// using the limited range of influence option ... somehow enters a 'loop' of mandatory
    /// actions, repeating a sequence of events with no way to stop, the game is a draw."
    pub const PLAYER: Limits = Limits {
        decisions_per_turn: u16::MAX,
        turns: u16::MAX,
        ..Limits::DEFAULT
    };

    /// The budgets search plays under below a position at turn `turn` with `decisions` already
    /// offered this turn: the defaults, or, where a player game has gone past one, that much room
    /// again from where it stands, so search is not stalled before it plays.
    #[must_use]
    pub const fn search(turn: u16, decisions: u16) -> Limits {
        let d = Limits::DEFAULT;
        Limits {
            decisions_per_turn: if decisions < d.decisions_per_turn {
                d.decisions_per_turn
            } else {
                decisions.saturating_add(d.decisions_per_turn)
            },
            turns: if turn < d.turns {
                d.turns
            } else {
                turn.saturating_add(d.turns)
            },
            ..d
        }
    }
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
    /// Each seat's steps under the smart stops, by seat index.
    pub step_stops: [StepStops; 2],
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
    /// Arena player clicks it, and may take a cast begun at their priority back rather than
    /// aim it at their own permanent. The engine otherwise announces a lone legal target
    /// silently (§7.2).
    ///
    /// - [CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "The player
    ///   announces their choice of an appropriate object or player for each target the spell
    ///   requires."
    /// - [CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012): if the casting cannot
    ///   be completed, "the casting of the spell is illegal; the game returns to the moment
    ///   before the casting of that spell was proposed".
    pub asks_targets: Option<Seat>,
    /// The player, if any, whose priority offers each untapped mana source's mana abilities on
    /// their own, the mana floating until spent or the step ends: a human, who taps a land
    /// before casting to choose its mana over the payment the engine would pick, as an Arena
    /// player clicks one. For everyone else tapping ahead of a payment is dominated by tapping
    /// during it (§10), so nothing is offered, and a priority whose only actions are these is
    /// still answered as a lone pass.
    ///
    /// - [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may
    ///   activate an activated mana ability whenever they have priority".
    /// - [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): mana "can stay in the
    ///   player's mana pool as unspent mana. Each player's mana pool empties at the end of each
    ///   step and phase".
    pub taps_mana: Option<Seat>,
    /// By seat, whether the player isn't offered activations that would do nothing
    /// ([`crate::card::Needs`]): everyone under the smart stops, search and rollouts included,
    /// which then spend no effort on them, and a human not in full control. Full control and
    /// the exact stops offer them, as the rules do: an activation that does nothing is still
    /// legal ([CR 602.2](https://yawgatog.com/resources/magic-rules/#R6022)).
    pub hides_pointless: [bool; 2],
}

impl Profile {
    /// Everything the rules offer, less what §7.2 settles silently.
    pub const EXACT: Profile = Profile {
        stops: Stops::Exact,
        step_stops: [StepStops::SMART; 2],
        limits: Limits::DEFAULT,
        breakpoint: None,
        payment: Payment::KeepMostPayable,
        full_control: None,
        asks_targets: None,
        taps_mana: None,
        hides_pointless: [false; 2],
    };
    /// The smart stops, for search and, by default, for humans.
    pub const SMART: Profile = Profile {
        stops: Stops::Smart,
        step_stops: [StepStops::SMART; 2],
        limits: Limits::DEFAULT,
        breakpoint: None,
        payment: Payment::KeepMostPayable,
        full_control: None,
        asks_targets: None,
        taps_mana: None,
        hides_pointless: [true; 2],
    };

    /// Whether `player`'s priority now is a decision, given they have a legal action.
    pub(crate) fn stops_for<L: Layout>(self, s: &State<L>, player: Seat) -> bool {
        match self.stops {
            Stops::Exact | Stops::Forced => true,
            Stops::Smart => is_smart_stop(s, player, self.step_stops[player.index()]),
        }
    }

    /// Whether `player`'s priority offers activating mana abilities on their own: under
    /// manual payment, or while [`Profile::taps_mana`] names them.
    pub(crate) fn floats_for(self, player: Seat) -> bool {
        self.payment == Payment::Manual || self.taps_mana == Some(player)
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
    pub(crate) fn asks_lone<L: Layout>(self, s: &State<L>, d: &Decision<L>) -> bool {
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
/// declared, the opponent's end step, and whenever the stack is non-empty", with the steps
/// the player's `steps` name in place of those.
fn is_smart_stop<L: Layout>(s: &State<L>, player: Seat, steps: StepStops) -> bool {
    !s.stack.is_empty() || steps.stops_in(s.turn.step, s.turn.active == player, || any_attacking(s))
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
/// declared; under smart stops, the next step of the turn that is one of the viewer's `step_stops`
/// ([`is_smart_stop`]), counting the combat in which the viewer, as the active player, can
/// declare attackers; otherwise the next step.
pub fn next_stop<L: Layout>(
    obs: Observation<'_, L>,
    stops: Stops,
    step_stops: StepStops,
) -> NextStop {
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
    next_step_stop(obs, step_stops, step)
}

/// The next step of the turn after `after` that is one of the viewer's `steps` under the smart
/// stops with the stack empty, counting the combat in which the viewer, as the active player,
/// can declare attackers.
pub fn next_step_stop<L: Layout>(
    obs: Observation<'_, L>,
    steps: StepStops,
    after: TurnStep,
) -> NextStop {
    let s = obs.shown();
    let active = s.turn.active == obs.viewer();
    let declares =
        active && s.turn.step < TurnStep::BeginningOfCombat && combat::can_declare_attackers(s);
    let attacking = declares || any_attacking(s);
    for l in TurnStep::ALL.into_iter().filter(|&l| l > after) {
        if (l == TurnStep::BeginningOfCombat && declares) || steps.stops_in(l, active, || attacking)
        {
            return NextStop::Step(l);
        }
    }
    NextStop::NextTurn
}

/// Whether the defending player in the position `obs` shows has a creature that can block
/// now, so that declaring blockers would be a decision of theirs.
pub fn can_declare_blockers<L: Layout>(obs: Observation<'_, L>) -> bool {
    combat::can_declare_blockers(obs.shown())
}
