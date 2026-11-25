//! A machine that comes back after the hub released its claims for silence.
//!
//! The hub releases every claim of a machine that has sent no heartbeat for two hours (upkeep's
//! `offline` step), because a machine that is gone cannot release its own tickets. A machine
//! that was only away (a reboot, a drive that stopped answering, a lid closed for the night)
//! comes back with its CLI workers still in their trees. Each release records
//! [`ReleasedOffline`] on the ticket under [`RELEASED_OFFLINE_KEY`], and every heartbeat lists
//! the ticket each worker holds a tree for. When a worker on the released machine still lists a
//! ticket that the hub released from that same session, the hub answers in the heartbeat's
//! reply with a [`Reconciled`]:
//!
//! - [`ReturnOutcome::Reclaimed`]: the ticket was still open, unassigned, unreserved and
//!   unchanged since the release, so the hub gave it back to the same session and tree. The
//!   worker carries on.
//! - [`ReturnOutcome::Lost`]: someone else took the ticket, it was closed, or it was changed
//!   since the release. The daemon saves the tree's work to the ticket's saved branch, frees the
//!   tree, retires the worker's slot and says so on the ticket.

use serde::{Deserialize, Serialize};

use super::tickets::{Status, Ticket};
use crate::ids::{MachineId, SessionId, TicketId, TreeId};
use crate::time::Timestamp;

/// The ticket metadata key of [`ReleasedOffline`].
pub const RELEASED_OFFLINE_KEY: &str = "released_offline";

/// Who held a ticket when the hub released it because the holder's machine stopped reporting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasedOffline {
    pub machine: MachineId,
    pub session: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeId>,
    /// The CLI provider the session ran on, which a reclaim restores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// When the hub released the claim. The release also set the ticket's `updated_at` to this
    /// time, so a later `updated_at` means someone changed the ticket since.
    pub at: Timestamp,
}

impl ReleasedOffline {
    /// The ticket's record of a release for silence, if it has one that parses.
    pub fn of(ticket: &Ticket) -> Option<ReleasedOffline> {
        let value = ticket.metadata.get(RELEASED_OFFLINE_KEY)?;
        serde_json::from_value(value.clone()).ok()
    }
}

/// Whether the hub released `ticket` from `session` for silence and nobody has claimed it since.
/// The session's machine then expects the hub to give it back on its next heartbeat, so the
/// daemon must not treat the claim as lost meanwhile: freeing the tree or retiring the worker
/// would throw away a claim the hub is about to restore.
pub fn awaits_return(ticket: &Ticket, session: &SessionId) -> bool {
    ticket.status == Status::Open
        && ticket.assignee.is_none()
        && ReleasedOffline::of(ticket).is_some_and(|record| &record.session == session)
}

/// What the hub did with a ticket that a worker of a returning machine still holds a tree for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnOutcome {
    /// The ticket is the session's again.
    Reclaimed,
    /// The ticket was taken, closed or changed while the machine was away.
    Lost,
}

/// One answer of the heartbeat reply's `reconciled` list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reconciled {
    pub session: SessionId,
    pub ticket: TicketId,
    pub outcome: ReturnOutcome,
}
