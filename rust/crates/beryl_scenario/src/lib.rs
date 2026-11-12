//! Scenarios: black-box tests of the rules, one card at a time (architecture §17.1).
//!
//! A [`Scenario`] is a value: a [`Setup`] (where each card is, life, the turn and step), then
//! [`Action`]s, each a scripted decision or an [`Assertion`]. The value types are
//! `beryl_scenario_types`', re-exported here: a card file declares its scenarios against that
//! crate, which depends on the vocabulary alone, so only this crate, the runner, rebuilds with
//! the rules. [`run`] plays one through the engine's public decision API under the exact
//! profile, and is strict: every decision the engine offers must be scripted, so the decision
//! surface itself stays under test. [`to_json`] serializes scenarios for the oracle exporters
//! (§17.2), each with its [`trace`].

mod exhaust;
pub mod facts;
mod json;
mod run;
mod script;
mod setup;
#[cfg(test)]
mod tests;

use beryl_engine::card::Purpose;
use beryl_engine::{DecisionKind, Seat, TurnStep};
use serde::Serialize;

pub use beryl_scenario_types::{
    Action, Assertion, Counters, Ending, P0, P1, Permanent, Pick, PlayerSetup, Scenario, Setup,
};
pub use exhaust::{Bounds, Check, Found, Searched, exhaust, invariants};
pub use json::{Json, to_json};
pub use run::{Live, answer, describe, run, trace, trace_with, visit, watch, watch_with};
pub use script::{script, setup_source, source};
pub use setup::{Slots, position};

/// Why a scenario failed: at which action, and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub scenario: &'static str,
    /// The index of the failing action, or `None` for the setup.
    pub action: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.action {
            Some(i) => write!(f, "{}: action {i}: {}", self.scenario, self.message),
            None => write!(f, "{}: setup: {}", self.scenario, self.message),
        }
    }
}

impl std::error::Error for Failure {}

/// Where the game stood when an action began, as Beryl played it: the turn, the step, the
/// open decision and the stack, with the coin flips, taps and forced answers on the way. Oracles
/// script by turn and step rather than by decision, so the exporters place each action with
/// this (§17.2).
#[derive(Clone, Debug, Serialize)]
pub struct Position {
    pub turn: u16,
    #[serde(serialize_with = "json::variant")]
    pub step: TurnStep,
    #[serde(serialize_with = "json::seat")]
    pub active: Seat,
    /// Who decides at the open decision, and what; both `None` once the game is over.
    #[serde(serialize_with = "json::optional_seat")]
    pub decider: Option<Seat>,
    #[serde(serialize_with = "json::optional_variant")]
    pub kind: Option<DecisionKind>,
    /// What the open decision's prompt is for, by its name ([`Purpose::name`]): a kind says
    /// only the form of the answer, and an oracle asks a cost's picks, say, unlike an
    /// effect's.
    #[serde(serialize_with = "json::optional_purpose")]
    pub purpose: Option<Purpose>,
    /// Bottom first.
    pub stack: Vec<StackItem>,
    /// The coin flips since the previous action began, in order, so that an oracle can
    /// replay Beryl's seeded results with its own.
    pub flips: Vec<CoinFlip>,
    /// The permanents that became tapped since the previous action began, by name: among
    /// them the mana sources a payment tapped, which an oracle may choose differently.
    pub taps: Vec<&'static str>,
    /// The questions with one answer since the previous action began, which Beryl answers
    /// without asking and an oracle may ask.
    pub forced: Vec<Forced>,
    /// The cards that have flashback here, by name, as a Snapcaster Mage grants it: an oracle
    /// casts them with their flashback ability, which a permission to cast a card from its
    /// graveyard is not.
    pub flashback: Vec<&'static str>,
    /// The face-down permanents here, by their cards' names: an oracle names them otherwise,
    /// as they have no name (CR 708.2a).
    pub face_down: Vec<&'static str>,
    /// How many creatures each player controls here, by seat: whether an effect that counts
    /// them, as overloaded Winds of Abandon's searches, does anything, where an oracle asks
    /// regardless of the count.
    pub creatures: [u8; 2],
}

/// A question with one answer, as the answer a scenario would script, what its prompt was
/// for, and the turn and step it came in.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Forced {
    pub turn: u16,
    #[serde(serialize_with = "json::variant")]
    pub step: TurnStep,
    #[serde(serialize_with = "json::purpose")]
    pub purpose: Purpose,
    #[serde(flatten, with = "json::ActionDef")]
    pub answer: Action,
}

/// A coin flip ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "If the call
/// matches the result, the player wins the flip."
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CoinFlip {
    #[serde(serialize_with = "json::seat")]
    pub player: Seat,
    pub won: bool,
}

/// A spell or ability on the stack.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct StackItem {
    /// The spell's name, or the name of the ability's source.
    pub name: &'static str,
    pub spell: bool,
    #[serde(serialize_with = "json::seat")]
    pub controller: Seat,
}
