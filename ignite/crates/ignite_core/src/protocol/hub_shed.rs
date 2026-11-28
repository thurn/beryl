//! Shedding build tickets from a machine that holds more than its limit.
//!
//! The hub hands a machine a build ticket (a ticket expected to compile, see
//! `crate::dispatch_pressure`) only while the machine holds fewer build tickets than its limit,
//! one per `SHARED_CORES_PER_BUILD_TICKET` of its shared cores. That check runs only at hand-off,
//! so nothing acted on a machine that already held more, however that happened: a dispatch bug,
//! a limit lowered by hand, or a machine that lost cores. A machine at 100% CPU then starves its
//! own landings and tip verification.
//!
//! Each heartbeat reply that answers a machine sample carries the machine's [`BuildHoldings`].
//! The daemon sheds down to the limit by itself once it has held more than [`AUTO_SHED_FACTOR`]
//! times its limit for [`AUTO_SHED_AFTER_S`] seconds, and `ignite pool shed` sheds at once.
//! Shedding a ticket stops the worker's running commands, saves its tree's work on the remote,
//! releases the ticket with [`super::ReleaseCause::Shed`] and ends the worker's session, so the
//! worker never has to cooperate.

use serde::{Deserialize, Serialize};

use crate::ids::{MachineId, SessionId, TicketId};
use crate::time::Timestamp;

/// A machine holding more than this many times its build-ticket limit sheds by itself.
pub const AUTO_SHED_FACTOR: u32 = 2;

/// How long, in seconds, a machine must stay over [`AUTO_SHED_FACTOR`] times its limit before
/// it sheds by itself, so that a brief overlap (a ticket landing while the next one starts)
/// never costs a worker its ticket.
pub const AUTO_SHED_AFTER_S: f64 = 300.0;

/// One build ticket a machine holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldBuild {
    pub ticket: TicketId,
    /// The session working it, or `None` for a reservation not claimed yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    /// When it was claimed, or when it was reserved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<Timestamp>,
    /// The ticket holds `master` back (a regression, a justiciar ticket or a repair that tip
    /// verification asked for), so it is never shed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub master_blocker: bool,
}

/// The build tickets a machine holds and its limit, as the hub counts them at hand-off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildHoldings {
    pub limit: u32,
    /// Newest claim first.
    pub tickets: Vec<HeldBuild>,
}

impl BuildHoldings {
    #[must_use]
    pub fn held(&self) -> u32 {
        u32::try_from(self.tickets.len()).unwrap_or(u32::MAX)
    }

    /// The line `ignite pool list` and `ignite status` show.
    #[must_use]
    pub fn line(&self) -> String {
        format!("held {} of limit {} build tickets", self.held(), self.limit)
    }

    /// Whether the machine holds enough more than its limit to shed by itself once that has
    /// lasted [`AUTO_SHED_AFTER_S`].
    #[must_use]
    pub fn far_over(&self) -> bool {
        self.held() > self.limit.saturating_mul(AUTO_SHED_FACTOR)
    }

    /// The tickets to shed to bring the machine down to `to` build tickets, newest claim first.
    /// A reservation not claimed yet, a ticket that holds `master` back and a ticket that `keep`
    /// names (one that is landing) are kept, though they still count against `to`, so fewer may
    /// be shed than the excess.
    pub fn to_shed(&self, to: u32, keep: impl Fn(&HeldBuild) -> bool) -> Vec<&HeldBuild> {
        let excess = self.held().saturating_sub(to) as usize;
        self.tickets
            .iter()
            .filter(|held| held.session.is_some() && !held.master_blocker && !keep(held))
            .take(excess)
            .collect()
    }
}

/// `POST /shed`: `ignite pool shed --machine <name>` orders a machine to shed. The hub keeps
/// the order until that machine's next heartbeat with a sample, whose reply carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShedOrder {
    /// The machine, by name or ID.
    pub machine: String,
    /// The number of build tickets to keep, or `None` for the machine's limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<u32>,
    /// Who ordered it: a session, or the operator.
    pub by: String,
}

/// `POST /shed/done`: what a machine shed, which the hub records in the fleet's recent changes
/// so that `ignite fleet` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShedDone {
    pub machine: MachineId,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(ticket: &str, session: Option<&str>, master_blocker: bool) -> HeldBuild {
        HeldBuild {
            ticket: TicketId::new(ticket).unwrap(),
            session: session.map(|s| SessionId::new(s).unwrap()),
            since: None,
            master_blocker,
        }
    }

    #[test]
    fn shedding_takes_the_newest_claims_and_keeps_reservations_blockers_and_landings() {
        let holdings = BuildHoldings {
            limit: 3,
            tickets: vec![
                held("T6", None, false),
                held("T5", Some("cli-a-5"), false),
                held("T4", Some("cli-a-4"), true),
                held("T3", Some("cli-a-3"), false),
                held("T2", Some("cli-a-2"), false),
                held("T1", Some("cli-a-1"), false),
            ],
        };
        let shed: Vec<&str> = holdings
            .to_shed(3, |held| held.ticket.as_str() == "T3")
            .iter()
            .map(|held| held.ticket.as_str())
            .collect();
        assert_eq!(shed, ["T5", "T2", "T1"]);
        assert_eq!(holdings.line(), "held 6 of limit 3 build tickets");
        assert!(!holdings.far_over());
        assert!(holdings.to_shed(6, |_| false).is_empty());
    }
}
