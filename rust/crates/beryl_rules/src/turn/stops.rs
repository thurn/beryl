//! Restriction profiles at work (architecture §12.3, §15): where a [`Profile`], which is the
//! kernel's data (`beryl_kernel::profile`), stops a player in the game as it stands.
//! `crate::profile` re-exports these beside the kernel's types.
//!
//! Implements: none. Applies the player's stop settings.

use beryl_kernel::{StepPosition, StepTarget};

use crate::card::Purpose;
use crate::combat;
use crate::decision::{Decision, DecisionKind};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::observation::Observation;
use crate::profile::{Profile, StepStops, Stops};
use crate::state::{Pending, State, TurnStep};
use crate::table::GameTable;
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
/// (see [CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)).
pub(crate) fn asks_lone<L: Layout>(profile: Profile, s: &State<L>, d: &Decision<L>) -> bool {
    (d.prompt.purpose == Purpose::Waterbend && profile.taps_mana == Some(d.player))
        || d.kind == DecisionKind::Target
            && profile.asks_targets == Some(d.player)
            && (matches!(s.pending(), Pending::Casting(c) if !c.triggered())
                // A lone retarget is already answered inside the resolution. A paused
                // lone target here belongs to a cast asking under this stop setting.
                || matches!(s.pending(), Pending::Resolving(_)))
}

/// The smart stops (§15): "their own main phases, after attackers and after blockers are
/// declared, the opponent's end step, and whenever the stack is non-empty", with the steps
/// the player's `steps` name in place of those.
fn is_smart_stop<L: Layout>(s: &State<L>, player: Seat, steps: StepStops) -> bool {
    !s.stack().is_empty()
        || steps.stops_in(s.turn().step(), s.turn().active() == player, || {
            any_attacking(s)
        })
}

/// Where passing priority now takes the viewer, as the pass button says (decision D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NextStop {
    /// The top of the stack resolves.
    Resolve,
    /// Combat damage is dealt.
    CombatDamage,
    /// The viewer next gets priority, or declares attackers, in this step of the turn.
    Step(TurnStep, StepTarget),
    /// The turn ends.
    NextTurn,
}

/// Where passing priority now takes the viewer of `obs` under `stops`, if the other player
/// passes too: the resolution of the top of the stack; combat damage once blockers are
/// declared; under smart stops, the next step of the turn that is one of the viewer's `step_stops`
/// ([`is_smart_stop`]), counting the combat in which the viewer, as the active player, can
/// declare attackers; otherwise the next step.
pub fn next_stop<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    stops: Stops,
    step_stops: StepStops,
) -> NextStop {
    let s = obs.shown();
    if !s.stack().is_empty() {
        return NextStop::Resolve;
    }
    let step = s.turn().step();
    let attacking = any_attacking(s);
    if attacking
        && matches!(
            step,
            TurnStep::DeclareBlockers | TurnStep::FirstStrikeDamage
        )
        && (!s.has_turn_plan()
            || matches!(
                s.planned_next_step(),
                Some((TurnStep::FirstStrikeDamage | TurnStep::CombatDamage, false))
            ))
    {
        return NextStop::CombatDamage;
    }
    if stops != Stops::Smart {
        return s.later_steps().next().map_or_else(
            || {
                NextStop::Step(
                    TurnStep::Untap,
                    StepTarget::Standard(StepPosition {
                        turn: s.turn().number() + 1,
                        ordinal: 1,
                    }),
                )
            },
            |(step, target)| NextStop::Step(step, target),
        );
    }
    next_step_stop(t, obs, step_stops, None)
}

/// The next step of the turn after `after` that is one of the viewer's `steps` under the smart
/// stops with the stack empty, counting the combat in which the viewer, as the active player,
/// can declare attackers.
pub fn next_step_stop<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    steps: StepStops,
    after: Option<StepTarget>,
) -> NextStop {
    let s = obs.shown();
    let active = s.turn().active() == obs.viewer();
    let mut after = after;
    let mut attacking = any_attacking(s);
    for (step, target) in s.later_steps() {
        let declares =
            step == TurnStep::BeginningOfCombat && active && combat::can_declare_attackers(s, t);
        if step == TurnStep::BeginningOfCombat {
            attacking = declares;
        }
        if after.is_some() {
            if after == Some(target) {
                after = None;
            }
            continue;
        }
        if declares || steps.stops_in(step, active, || attacking) {
            return NextStop::Step(step, target);
        }
    }
    NextStop::NextTurn
}

/// Whether the defending player in the position `obs` shows has a creature that can block
/// now, so that declaring blockers would be a decision of theirs.
pub fn can_declare_blockers<L: Layout>(obs: Observation<'_, L>, t: &GameTable) -> bool {
    combat::can_declare_blockers(obs.shown(), t)
}
