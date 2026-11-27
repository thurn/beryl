//! Emergencies and rushes, the work the scheduler serves ahead of every class but a tip round's
//! build checks (docs/ignite.md, "Cores, memory and shared resources").

use serde::{Deserialize, Serialize};

use super::{GrantRequest, Limits, Scheduler};

/// Work scheduled ahead of every class, behind only a tip round's build checks: such a request
/// is ranked in `land`, goes first within it, may take every shared core, and its grant never
/// gives cores back for another request (`ledger/order.rs`). Earlier variants go first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    /// A command of an `incident` or `justiciar` ticket.
    Emergency,
    /// A command of a session the operator rushed with `/rush` (see [`crate::rush`]).
    Rush,
}

impl Limits {
    /// The most cores `request` may hold: every shared core for an urgent command
    /// ([`Urgency`]), which is meant to finish first, and [`Limits::max_per_command`] otherwise.
    pub fn max_for(&self, request: &GrantRequest) -> u32 {
        if request.urgency.is_some() {
            self.cores.max(1)
        } else {
            self.max_per_command()
        }
    }
}

impl Scheduler {
    /// Whether a rushed session's command holds a grant or waits for one, during which the
    /// machine starts no ordinary workers (see [`crate::rush`]).
    pub fn rush_busy(&self) -> bool {
        let status = self.status();
        status.rush_running + status.rush_waiting > 0
    }
}
