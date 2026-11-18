//! Every message Ignite's processes exchange, as serde types.
//!
//! - [`client`]: one JSON line from the `ignite` command to its machine's daemon, and one back.
//! - [`tickets`]: the ticket routes, which keep the ticket service's contract.
//! - [`hub`]: the hub's other routes: machines, hand-offs, the result cache, tips and records.
//! - [`host`]: the calls a verb process makes to its machine's daemon.
//!
//! Types that more than one of those use (the command classes, the cost of a command, a
//! check's outcome) are defined here.

pub mod client;
pub mod host;
pub mod hub;
pub mod tickets;

use serde::{Deserialize, Serialize};

/// The class a command asks for cores in. Earlier variants are higher: a lower class starts
/// nothing while a higher one waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Landing gates.
    Land,
    /// Commands a worker runs itself, and the prebuild of a tree leased to a ticket.
    Agent,
    /// Tip verification and finding a regression's culprit.
    Tip,
    /// Long experiments.
    Batch,
    /// Exploration checks, cache audits, rebuild counts, and keeping a free tree built.
    Idle,
}

impl Class {
    pub const ALL: [Class; 5] = [
        Class::Land,
        Class::Agent,
        Class::Tip,
        Class::Batch,
        Class::Idle,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Class::Land => "land",
            Class::Agent => "agent",
            Class::Tip => "tip",
            Class::Batch => "batch",
            Class::Idle => "idle",
        }
    }
}

/// What one finished command cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Seconds spent waiting for cores, apart from the command's own time.
    pub queued_s: f64,
    pub wall_s: f64,
    pub cpu_s: f64,
    /// Instructions retired by the command and its descendants; `None` where the operating
    /// system does not count them.
    pub instructions: Option<u64>,
    pub peak_mb: u64,
}

/// What one run of one check said.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail,
    /// Failed, then passed when run again at once.
    Flaky,
    /// The run ended without reporting the check: never a pass and never a failure.
    Incomplete,
}
