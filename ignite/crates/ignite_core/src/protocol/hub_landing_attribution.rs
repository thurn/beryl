//! Baseline failure reports and historical attribution for landing gates, and culprit searches
//! a slow machine hands to a faster one.

use serde::{Deserialize, Serialize};

use super::RegressionReport;
use crate::ids::TicketId;
use crate::machine_speed::Speed;

/// A landing confirmed a failure at the staging tip it compared against. The inputs select
/// candidate landings for later attribution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingRegression {
    pub report: RegressionReport,
    pub inputs: Vec<String>,
}

/// A recorded baseline failure whose historical attribution is still due.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingAttribution {
    pub ticket: TicketId,
    pub baseline: LandingRegression,
}

/// The result of a historical search. The ticket and original detection commit identify the
/// open failure this result can update.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionAttribution {
    pub ticket: TicketId,
    pub report: RegressionReport,
}

/// The faster machine that runs a culprit search in place of the machine that saw the failure
/// (`GET /regressions/searcher`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CulpritSearcher {
    /// The faster machine's name.
    pub machine: String,
    pub speed: Speed,
    /// The speed of the machine that saw the failure.
    pub here: Speed,
}

impl CulpritSearcher {
    /// Why the machine that saw the failure did not search, for `CulpritSearch::NotRun`.
    pub fn why(&self) -> String {
        format!(
            "handed to {}, the fastest free machine with this system (speed {}, here {})",
            self.machine, self.speed, self.here
        )
    }
}
