//! Restriction profiles at work (architecture §12.3, §15): where a [`Profile`], which is the
//! kernel's data (`beryl_kernel::profile`), stops a player in the game as it stands.
//! `crate::profile` re-exports these beside the kernel's types.

use crate::combat;
use crate::decision::{Decision, DecisionKind};
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::observation::Observation;
use crate::profile::{Profile, StepStops, Stops};
use crate::state::{Pending, State, TurnStep};
use crate::turn::any_attacking;

/// Whether `player`'s priority now is a decision, given they have a legal action.
pub(crate) fn stops_for<L: Layout>(profile: Profile, s: &State<L>, player: Seat) -> bool {
    match profile.stops {
        Stops::Exact | Stops::Forced => true,
        Stops::Smart => is_smart_stop(s, player, profile.step_stops[player.index()]),
    }
}

/// Whether `d`, a decision with one candidate, is still asked: a target of a spell its
/// player casts or an ability they activate, while [`Profile::asks_targets`] names them,
/// or a waterbend payment whose human must select even a required bender.
/// A triggered ability's lone target stays silent, as nothing can take it back
/// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)).
pub(crate) fn asks_lone<L: Layout>(profile: Profile, s: &State<L>, d: &Decision<L>) -> bool {
    (d.kind == DecisionKind::Waterbend && profile.taps_mana == Some(d.player))
        || d.kind == DecisionKind::Target
            && profile.asks_targets == Some(d.player)
            && matches!(s.pending, Pending::Casting(c) if !c.triggered)
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
