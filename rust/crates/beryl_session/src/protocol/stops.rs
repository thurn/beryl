//! Where the player's own stops are set.

use beryl_engine::state::TurnStep;
use beryl_engine::{StepStops, Stops};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Step;

/// Where the viewer gets priority (architecture §15, decision D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StopsSetting {
    /// Full control: every priority the viewer receives, even where passing is all they can
    /// do. Otherwise the smart stops: wherever the viewer can act, an opponent's doings on the
    /// stack and the steps below; and their own precombat main phase in any case, while it is
    /// one of them.
    pub full_control: bool,
    /// The steps of the viewer's own turns where they stop with the stack empty, as Arena's
    /// phase stops: by default their main phases, and combat once creatures attack.
    #[ts(as = "Vec<Step>")]
    pub yours: StepSet,
    /// The steps of the opponent's turns where the viewer stops with the stack empty: by
    /// default combat once creatures attack, and the end step.
    #[ts(as = "Vec<Step>")]
    pub theirs: StepSet,
}

impl StopsSetting {
    /// The smart stops' default steps.
    pub const SMART: StopsSetting = StopsSetting {
        full_control: false,
        yours: StepSet::of(&[
            TurnStep::PrecombatMain,
            TurnStep::DeclareAttackers,
            TurnStep::DeclareBlockers,
            TurnStep::PostcombatMain,
        ]),
        theirs: StepSet::of(&[
            TurnStep::DeclareAttackers,
            TurnStep::DeclareBlockers,
            TurnStep::End,
        ]),
    };
    /// Full control.
    pub const EXACT: StopsSetting = StopsSetting {
        full_control: true,
        ..StopsSetting::SMART
    };

    pub fn stops(self) -> Stops {
        if self.full_control {
            Stops::Exact
        } else {
            Stops::Smart
        }
    }

    /// The steps as the engine holds them for the viewer.
    pub fn step_stops(self) -> StepStops {
        TurnStep::ALL
            .into_iter()
            .fold(StepStops::NONE, |stops, step| {
                stops.with(step, true, self.yours.has(step)).with(
                    step,
                    false,
                    self.theirs.has(step),
                )
            })
    }
}

impl Default for StopsSetting {
    fn default() -> StopsSetting {
        StopsSetting::SMART
    }
}

/// A set of steps of the turn, which the wire carries as a list of [`Step`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StepSet(u16);

impl StepSet {
    pub const fn of(steps: &[TurnStep]) -> StepSet {
        let mut bits = 0;
        let mut i = 0;
        while i < steps.len() {
            bits |= 1 << steps[i] as u16;
            i += 1;
        }
        StepSet(bits)
    }

    pub const fn has(self, step: TurnStep) -> bool {
        self.0 & (1 << step as u16) != 0
    }

    pub fn steps(self) -> impl Iterator<Item = TurnStep> {
        TurnStep::ALL
            .into_iter()
            .filter(move |&step| self.has(step))
    }
}

impl Serialize for StepSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.steps().map(Step::from))
    }
}

impl<'de> Deserialize<'de> for StepSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<StepSet, D::Error> {
        let steps = Vec::<Step>::deserialize(deserializer)?;
        Ok(StepSet(steps.into_iter().fold(0, |bits, step| {
            bits | 1 << TurnStep::from(step) as u16
        })))
    }
}
