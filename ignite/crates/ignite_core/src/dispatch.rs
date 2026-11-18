//! Dispatch order: which ready ticket the hub offers a machine that has a worker free, and which
//! of its idle workers gets it.
//!
//! The hub answers with the first ticket, in this order, that the machine can take:
//!
//! 1. An open regression ticket. It skips the test of expected cores.
//! 2. An improvement ticket, when improvements are under their share
//!    ([`crate::shares::improvements_due`]). The largest saving goes first, taking the three
//!    units in turn.
//! 3. A ready ticket from the group furthest below its share of workers
//!    ([`crate::shares::epic_order`]). Within a group, the highest priority goes first, then the
//!    longest chain of tickets waiting behind it, then the oldest.
//!
//! A machine can take a ticket when it has a free ticket tree, an idle worker that suits the
//! ticket (its app runs the model of the `tier:` label and is the app of a `provider:` label),
//! room for the ticket's expected cores, and every check family the ticket's files could bring
//! into its gate. A `needs:<os>` label, which a regression ticket carries, limits it to that operating system.
//! Among suitable workers, the one whose last ticket shared the most files gets it; dispatch
//! never waits for a particular worker. Everything here is a pure function of its arguments, so
//! the same snapshot always gives the same answer.

use std::cmp::Ordering;

use crate::admission;
use crate::ids::{SessionId, TicketId};
use crate::protocol::hub::{IdleWorker, MachineFree};
use crate::shares::{Group, Unit};
use crate::time::Timestamp;

/// What kind of work a ready ticket is, which sets its place in the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketKind {
    Regression,
    Improvement,
    Ordinary,
}

/// A ready ticket as dispatch sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadyTicket {
    pub id: TicketId,
    pub kind: TicketKind,
    /// The group it shares workers in ([`crate::shares::group_of`]).
    pub epic: Group,
    /// 0 is the highest.
    pub priority: u8,
    /// The longest chain of tickets waiting behind it.
    pub chain: u32,
    pub created_at: Timestamp,
    pub labels: Vec<String>,
    /// Expected cores while in progress ([`admission::expected_cores`]).
    pub expected_cores: f64,
    /// The check families its named files could bring into its gate.
    pub families: Vec<String>,
    /// The files it names.
    pub files: Vec<String>,
    /// An improvement ticket's estimated saving per week.
    pub saving: Option<(Unit, f64)>,
}

/// A ticket and the idle worker it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub ticket: TicketId,
    pub session: SessionId,
}

/// The value of the first label with `prefix` (`tier:`, `provider:`, `lane:`), if any.
pub fn label<'a>(labels: &'a [String], prefix: &str) -> Option<&'a str> {
    labels.iter().find_map(|l| l.strip_prefix(prefix))
}

/// Within a group: priority, then the longest chain behind, then the oldest, then the ID so that
/// the order is total.
fn queue_order(a: &ReadyTicket, b: &ReadyTicket) -> Ordering {
    a.priority
        .cmp(&b.priority)
        .then(b.chain.cmp(&a.chain))
        .then(a.created_at.cmp(&b.created_at))
        .then(a.id.cmp(&b.id))
}

/// Whether `worker` can work `ticket`: its app runs the model the `tier:` label names and is
/// the app the `provider:` label names.
pub fn suits(worker: &IdleWorker, ticket: &ReadyTicket) -> bool {
    let tier = label(&ticket.labels, "tier:").is_none_or(|t| worker.models.iter().any(|m| m == t));
    let provider = label(&ticket.labels, "provider:").is_none_or(|p| worker.app == p);
    tier && provider
}

/// Whether the machine can take the ticket, leaving aside which worker.
fn machine_fits(free: &MachineFree, ticket: &ReadyTicket) -> bool {
    let families =
        free.families.is_empty() || ticket.families.iter().all(|f| free.families.contains(f));
    // Every `needs:` label names the machine's operating system or its name; `needs:person`
    // tickets are never ready for a worker.
    let os = ticket
        .labels
        .iter()
        .filter_map(|l| l.strip_prefix("needs:"))
        .all(|needed| needed == free.os.as_str() || free.name.as_deref() == Some(needed));
    let cores = ticket.kind == TicketKind::Regression
        || admission::fits(free.free_cores, ticket.expected_cores);
    free.free_ticket_trees > 0 && families && os && cores
}

/// The idle worker that gets the ticket: of those that suit it, the one whose last ticket shared
/// the most files with it, then the first listed. `last_files` gives each session's last
/// ticket's files; a session missing from it shares none.
pub fn pick_worker(
    free: &MachineFree,
    ticket: &ReadyTicket,
    last_files: &[(SessionId, Vec<String>)],
) -> Option<SessionId> {
    let shared = |worker: &IdleWorker| {
        last_files
            .iter()
            .find(|(session, _)| *session == worker.session)
            .map_or(0, |(_, files)| {
                ticket.files.iter().filter(|f| files.contains(f)).count()
            })
    };
    let mut best: Option<(&IdleWorker, usize)> = None;
    for worker in free.idle_workers.iter().filter(|w| suits(w, ticket)) {
        let score = shared(worker);
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((worker, score));
        }
    }
    best.map(|(worker, _)| worker.session.clone())
}

/// Improvement tickets in the order the share takes them: the largest saving in `first`, then
/// the largest in the next unit, and so on in turn; tickets with no saving last.
fn improvement_order(ready: &[ReadyTicket], first: Unit) -> Vec<&ReadyTicket> {
    let improvements = ready.iter().filter(|t| t.kind == TicketKind::Improvement);
    let mut by_unit: Vec<Vec<&ReadyTicket>> = Unit::ALL
        .iter()
        .map(|&unit| {
            let mut tickets: Vec<&ReadyTicket> = improvements
                .clone()
                .filter(|t| t.saving.is_some_and(|(u, _)| u == unit))
                .collect();
            tickets.sort_by(|a, b| {
                let amount = |t: &ReadyTicket| t.saving.map_or(0.0, |(_, n)| n);
                amount(b).total_cmp(&amount(a)).then(queue_order(a, b))
            });
            tickets.reverse();
            tickets
        })
        .collect();
    let mut order = Vec::new();
    let mut unit = first;
    while by_unit.iter().any(|tickets| !tickets.is_empty()) {
        let index = Unit::ALL.iter().position(|&u| u == unit).unwrap_or(0);
        if let Some(ticket) = by_unit[index].pop() {
            order.push(ticket);
        }
        unit = unit.next();
    }
    let mut unsaved: Vec<&ReadyTicket> = improvements.filter(|t| t.saving.is_none()).collect();
    unsaved.sort_by(|a, b| queue_order(a, b));
    order.extend(unsaved);
    order
}

/// The first ticket the machine can take, and the worker it goes to, or `None`.
///
/// - `epic_order` is the groups furthest below their share first ([`crate::shares::epic_order`]);
///   a group missing from it (at its cap) gets nothing here.
/// - `improvements` is `None` when improvements are not under their share, and otherwise the
///   unit whose largest saving goes first ([`crate::shares::first_unit`]). Improvement tickets
///   also take their place in their group like any other ticket.
/// - `last_files` gives each idle session's last ticket's files.
pub fn next_for(
    free: &MachineFree,
    ready: &[ReadyTicket],
    epic_order: &[Group],
    improvements: Option<Unit>,
    last_files: &[(SessionId, Vec<String>)],
) -> Option<Handoff> {
    let mut regressions: Vec<&ReadyTicket> = ready
        .iter()
        .filter(|t| t.kind == TicketKind::Regression)
        .collect();
    regressions.sort_by(|a, b| queue_order(a, b));
    let improvements = improvements.map_or_else(Vec::new, |unit| improvement_order(ready, unit));
    let groups = epic_order.iter().flat_map(|group| {
        let mut tickets: Vec<&ReadyTicket> = ready
            .iter()
            .filter(|t| t.kind != TicketKind::Regression && t.epic == *group)
            .collect();
        tickets.sort_by(|a, b| queue_order(a, b));
        tickets
    });
    regressions
        .into_iter()
        .chain(improvements)
        .chain(groups)
        .filter(|ticket| machine_fits(free, ticket))
        .find_map(|ticket| {
            pick_worker(free, ticket, last_files).map(|session| Handoff {
                ticket: ticket.id.clone(),
                session,
            })
        })
}

/// The next ticket of a `lane:` chain, for the session that just finished one of its tickets:
/// the first ready ticket with the same `lane:` label in queue order. It goes to that session
/// without clearing it, so it skips the dispatch order.
pub fn chain_next<'a>(lane: &str, ready: &'a [ReadyTicket]) -> Option<&'a ReadyTicket> {
    ready
        .iter()
        .filter(|t| label(&t.labels, "lane:") == Some(lane))
        .min_by(|a, b| queue_order(a, b))
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
