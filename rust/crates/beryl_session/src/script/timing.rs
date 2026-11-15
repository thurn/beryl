//! How a step is played: the records the session sends beside a script so that both clients
//! time it the same way. These are the web interface's `Timing` and `Effect` records
//! (`ui/src/stage/tracks.ts`) with the same meanings. Times are in milliseconds.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Subject;

/// How one step is played. Times are in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StepTiming {
    /// The verb's case in `data/ui-conventions.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub case: Option<String>,
    /// How long its motion takes.
    pub duration: f64,
    /// How long after its turn comes it begins.
    pub delay: f64,
    /// How long the next step waits for its turn.
    pub stagger: f64,
    /// The easing of its motion.
    pub curve: Curve,
    /// For a step that moves several cards one after another, how long each one moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub each: Option<f64>,
    /// It takes the turn of the step before it.
    pub with: bool,
    /// It waits until the steps of its beat that move these have ended.
    pub after: Vec<Subject>,
    /// It ends a span, and waits until the span has stood whole this long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hold: Option<f64>,
    /// What follows it waits until it is over.
    pub leads: bool,
    /// Its beat does not wait for it.
    pub passes: bool,
    /// It waits until nothing passes over the table.
    pub alone: bool,
    /// Its act is applied as its delay begins.
    pub opens: bool,
    /// Its act is applied with the step before's.
    pub ahead: bool,
    /// How long what it left room for takes to close up after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub closes: Option<f64>,
    /// The effects it begins.
    pub effects: Vec<StepEffect>,
}

/// An effect a step begins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StepEffect {
    /// The effect's name in the data.
    pub name: String,
    /// The edge of the step it begins at.
    pub when: Edge,
    /// What it is on, where that is not the step's own subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub on: Option<Subject>,
    /// How far through the step it begins, from 0 to 1, where that is not an edge of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub into: Option<f64>,
    /// How long it lasts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub duration: Option<f64>,
    /// How long after it begins it starts to show.
    pub delay: f64,
    /// The easing of the effect.
    pub curve: Curve,
}

/// The easing of a motion or an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Curve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

/// An end of a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Start,
    End,
}
