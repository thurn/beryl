//! Counting releases in `POST /release`, and holding back a ticket that different sessions
//! released (see `ignite_store::hub::releases`).
//!
//! A release is counted when it came from a worker giving up or from Ignite taking the ticket
//! back (`ReleaseCause::counts`) and names nothing the ticket needs. A release with `needs`
//! sends the ticket to a person, an operating system or a machine, so the next worker who is
//! offered it does not meet the same obstacle. A justiciar ticket is never counted, because
//! holding back an emergency would stop the work that ends it.

use rusqlite::Connection;

use ignite_core::dispatch::JUSTICIAR;
use ignite_core::protocol::hub::Release;
use ignite_core::protocol::tickets::Ticket;
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::releases::{self, Releases};

/// Records the release in the release's transaction when it counts, and returns the ticket's
/// releases when it is now held back, for [`file_after`] once the transaction commits.
pub(super) fn count(
    tx: &Connection,
    ticket: &Ticket,
    release: &Release,
    now: Timestamp,
) -> Result<Option<Releases>, StoreError> {
    let counted = release.cause.counts()
        && release.needs.is_none()
        && !ticket.labels.iter().any(|label| label == JUSTICIAR);
    if !counted {
        return Ok(None);
    }
    let releases = releases::record(
        tx,
        ticket.id.as_str(),
        release.session.as_str(),
        release.cause,
        &release.reason,
        now,
    )?;
    Ok(releases.held_back().then_some(releases))
}

/// Files or comments on the justiciar ticket for a ticket that is held back. The release has
/// already committed, so a failure here is logged and leaves the ticket held back: it is not
/// offered either way, and the next release of it files again.
pub(super) fn file_after(
    conn: &mut Connection,
    ticket: &Ticket,
    releases: &Releases,
    now: Timestamp,
) {
    let filed = crate::upkeep::justiciar::released_repeatedly(
        conn,
        ticket.id.as_str(),
        &ticket.title,
        releases,
        now,
    );
    if let Err(error) = filed {
        eprintln!(
            "ignite-hub: {} is held back after repeated releases, but its justiciar ticket was \
             not filed: {error}",
            ticket.id
        );
    }
}
