//! The human's pass on to their next stop, while the session passes their priority for them.

use beryl_engine::{
    DecisionKind, Limits, NextStop, Observation, Profile, Seat, StepPosition, StepStops,
    StepTarget, view,
};

use crate::protocol::StopsSetting;
use crate::replay::Entry;

use super::{At, Boards, Game, Open, on_board};

/// The human's pass on to their next stop ([`Game::pass_on`]), while the session passes their
/// priority for them.
#[derive(Clone, Copy, Debug)]
pub(super) struct PassingOn {
    /// The turn and step of their next stop, where it ends.
    pub(super) until: StepTarget,
}

impl Game {
    /// Begins passing on to the human's next stop from their open decision, if it can pass on,
    /// and records it. Returns whether it began.
    pub(super) fn start_passing(&mut self) -> bool {
        let Some(until) = self.pass_on_target() else {
            return false;
        };
        self.passing = Some(PassingOn { until });
        self.replay.log.push(Entry::PassOn);
        true
    }

    /// The turn and step of the human's next stop in a later step, if their open decision is a
    /// priority from which passing would stop them again in this one
    /// ([`crate::protocol::DecisionView::pass_on`]).
    pub(super) fn pass_on_target(&mut self) -> Option<StepTarget> {
        if self.reviewing()
            || !matches!(&self.at, At::Human(_, Open::Decision(d)) if d.kind == DecisionKind::Priority)
        {
            return None;
        }
        let next = on_board!(&mut self.board, b => {
            let obs = Observation::new(view(&b.state, &self.table, &mut b.scratch), self.human);
            crate::protocol::pass_on(&self.table, obs, self.stops)
        })?;
        let turn = self.board.turn().number();
        Some(match next {
            NextStop::Step(_, target) => target,
            NextStop::NextTurn => StepTarget::Standard(StepPosition {
                turn: turn + 1,
                ordinal: 1,
            }),
            NextStop::Resolve | NextStop::CombatDamage => {
                unreachable!("a later step's stop is a step or the next turn")
            }
        })
    }

    /// Whether the session passes the human's priority for them as they pass on to their next
    /// stop ([`Game::pass_on`]): whenever it is short of that stop, through the opponent's
    /// spells, abilities and attacks as well as the human's own (UI Conventions, "Passing to
    /// the end step"). The human asked for the end step, and a pass button that stopped at
    /// each opponent spell would do no more than "Resolve" beside it.
    pub(super) fn passes_for(&self, passing: PassingOn) -> bool {
        on_board!(&self.board, b => b.state.before_step(passing.until))
    }
}

/// `profile` under the human's `stops`. Full control asks the human every priority they get,
/// as Arena does, even where the engine would pass for them, and offers every activation;
/// their opponent's passes stay silent. Under the smart stops, the human stops in the steps
/// they chose, and the agent in the smart stops' own, as its search plays. The agent is not
/// offered an activation that would do nothing ([`Profile::hides_pointless`]); the human is,
/// but it makes no stop of its own ([`Profile::quiet_pointless`]). Whatever the stops, the
/// human picks each target of their own spells and abilities, even a lone one, so a click never
/// aims a spell for them ([`Profile::asks_targets`]), and may tap their mana sources at
/// priority, floating the mana, as an Arena player clicks a land ([`Profile::taps_mana`]). The
/// game's budgets are a player's ([`Limits::PLAYER`]): a human repeating a loop by hand never
/// runs into a rollout's.
pub fn with_stops(profile: Profile, stops: StopsSetting, human: Seat) -> Profile {
    let mut step_stops = [StepStops::SMART; 2];
    step_stops[human.index()] = stops.step_stops();
    // The human is offered every activation; under the smart stops, those that would do
    // nothing make no stop of their own (Profile::quiet_pointless).
    let mut hides_pointless = profile.hides_pointless;
    hides_pointless[human.index()] = false;
    // The human declares each attacker on its own, as Arena asks; the agent may declare a
    // class of interchangeable attackers in one pick (Profile::group_picks).
    let mut group_picks = profile.group_picks;
    group_picks[human.index()] = false;
    // The human receives priority before each of several identical triggers resolves, as on
    // Arena; the agent passes on the rest once it passed on the first
    // (Profile::passes_identical_triggers).
    let mut passes_identical_triggers = profile.passes_identical_triggers;
    passes_identical_triggers[human.index()] = false;
    Profile {
        stops: stops.stops(),
        step_stops,
        full_control: stops.full_control.then_some(human),
        asks_targets: Some(human),
        taps_mana: Some(human),
        hides_pointless,
        group_picks,
        passes_identical_triggers,
        quiet_pointless: (!stops.full_control).then_some(human),
        limits: Limits::PLAYER,
        ..profile
    }
}
