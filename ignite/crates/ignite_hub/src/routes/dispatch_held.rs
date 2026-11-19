//! What a paused machine is still offered: the tickets that hold `master` back.
//!
//! `ignite pool pause` stops new feature work on a machine. A regression ticket (a check that
//! tip verification found failing) or a justiciar ticket (a fault in Ignite itself) keeps
//! `master` from moving until someone works it, and a person who paused the pool to save usage
//! or to work by hand may not be watching. Left until `ignite pool resume`, such tickets once
//! held `master` back for more than 90 minutes. So a paused machine is still offered those
//! tickets, one at a time. Only a pause of every machine (`ignite pool pause --all`, which an
//! emergency also sets) stops them too.

use rusqlite::{Connection, params};

use ignite_core::dispatch::{JUSTICIAR, ReadyTicket, TicketKind};
use ignite_core::protocol::hub::MachineFree;
use ignite_store::Result;
use ignite_store::hub::fleet;

/// The states of a regression whose check still fails, so that its ticket holds `master` back.
pub const FAILING: [&str; 2] = ["open", ignite_store::hub::tips::REOPENED];

/// How far a pause limits what a machine is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paused {
    /// Every ready ticket may be offered.
    No,
    /// Only a ticket that holds `master` back, while the machine holds none already.
    MasterOnly,
    /// Nothing at all.
    Everything,
}

/// How far hand-offs to `free`'s machine are paused: by a pause of every machine, by its own
/// pause at the hub, or by its own pause that the daemon says it keeps (`master_only`), which
/// covers a pause the hub was never told of.
pub fn paused(conn: &Connection, free: &MachineFree) -> Result<Paused> {
    Ok(if fleet::paused_everywhere(conn)? {
        Paused::Everything
    } else if free.master_only || fleet::paused(conn, free.machine.as_str())? {
        Paused::MasterOnly
    } else {
        Paused::No
    })
}

/// Whether the ticket holds `master` back: a justiciar ticket, or the ticket of a failing
/// regression.
pub fn holds_master(ticket: &ReadyTicket) -> bool {
    ticket.kind == TicketKind::Regression || ticket.labels.iter().any(|l| l == JUSTICIAR)
}

/// Whether `machine` already works or has reserved a ticket that holds `master` back, so that
/// a paused machine gives such tickets to one worker at a time.
pub fn holds_one(conn: &Connection, machine: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM tickets t
           WHERE ((t.status = 'in_progress' AND t.machine = ?1)
                  OR t.id IN (SELECT ticket FROM reservations WHERE machine = ?1))
             AND (t.id IN (SELECT ticket FROM regressions WHERE state IN (?2, ?3))
                  OR EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = ?4)))",
        params![machine, FAILING[0], FAILING[1], JUSTICIAR],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
#[path = "dispatch_held_tests.rs"]
mod tests;
