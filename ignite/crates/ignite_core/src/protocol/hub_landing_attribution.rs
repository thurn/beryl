//! Baseline failure reports and historical attribution for landing gates.

use serde::{Deserialize, Serialize};

use super::RegressionReport;
use crate::ids::TicketId;

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
