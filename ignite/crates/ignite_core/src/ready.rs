//! Ticket readiness. A ticket is ready when nothing holds it back. Each thing that does is a
//! [`Blocker`] naming the ticket in the way and the rule it breaks, checked in this order:
//!
//! 1. The ticket itself is not open, is deferred until a time that has not passed, or is a
//!    meta-ticket, a template that only its copies carry out ([`crate::schedule`]).
//! 2. One of its own dependencies is live.
//! 3. A dependency of one of its ancestors is live.
//! 4. It is a `last` ticket and a live descendant of its parent is not.
//! 5. It has a live child.
//!
//! A dependency is live when its ticket is not closed or still has a live descendant. Edges of
//! the kinds `related` and `discovered-from` never block, and neither does an edge from a ticket
//! to itself. These are the ticket service's rules, and the blockers come back sorted by rule,
//! then by ticket ID, as it sorted them.

use std::collections::{BTreeMap, HashMap};

use crate::ids::TicketId;
use crate::protocol::tickets::{Blocker, Edge, Status, Ticket, TicketType};
use crate::time::Timestamp;

/// The tickets and edges that readiness depends on, indexed once so that many tickets can be
/// checked against them.
///
/// `tickets` must hold every live ticket and every ancestor of one, closed or not, and `edges`
/// every outgoing edge of those tickets. A ticket outside that set may still be checked with
/// [`Readiness::of`] as long as its ancestors are in the set too.
pub struct Readiness<'a> {
    by_id: HashMap<&'a TicketId, &'a Ticket>,
    /// The live descendants of each ticket that has any, in the order the tickets came.
    descendants: HashMap<&'a TicketId, Vec<&'a TicketId>>,
    outgoing: HashMap<&'a TicketId, Vec<&'a Edge>>,
    now: Timestamp,
}

impl<'a> Readiness<'a> {
    pub fn new(tickets: &'a [Ticket], edges: &'a [Edge], now: Timestamp) -> Self {
        let by_id: HashMap<&TicketId, &Ticket> = tickets.iter().map(|t| (&t.id, t)).collect();
        let mut readiness = Readiness {
            by_id,
            descendants: HashMap::new(),
            outgoing: HashMap::new(),
            now,
        };
        // A closed ticket may still have live descendants, so the whole path above each live
        // ticket is indexed, closed ancestors included.
        for ticket in tickets.iter().filter(|t| is_live(t)) {
            for ancestor in readiness.ancestors(ticket.parent.as_ref()) {
                readiness
                    .descendants
                    .entry(ancestor)
                    .or_default()
                    .push(&ticket.id);
            }
        }
        // A ticket that waits on itself would never be ready. No command makes such an edge,
        // but an older tracker's data may hold one, so it is ignored rather than trusted.
        for edge in edges.iter().filter(|e| e.src != e.dst) {
            readiness.outgoing.entry(&edge.src).or_default().push(edge);
        }
        readiness
    }

    /// The chain of ancestors starting at `parent`, nearest first, as far up as the indexed
    /// tickets reach.
    fn ancestors(&self, parent: Option<&'a TicketId>) -> Vec<&'a TicketId> {
        let mut found = Vec::new();
        let mut current = parent;
        while let Some(parent) = current {
            // A broken parent chain cannot loop forever.
            if found.contains(&parent) || found.len() > self.by_id.len() {
                break;
            }
            found.push(parent);
            current = self.by_id.get(parent).and_then(|t| t.parent.as_ref());
        }
        found
    }

    fn live(&self, id: &TicketId) -> Option<&'a Ticket> {
        self.by_id.get(id).copied().filter(|t| is_live(t))
    }

    /// Whether a dependency on `id` holds its dependent back.
    fn blocks(&self, id: &TicketId) -> bool {
        self.live(id).is_some() || self.descendants.contains_key(id)
    }

    /// What holds `ticket` back, sorted by rule and then by ticket ID; empty when it is ready.
    pub fn of(&self, ticket: &'a Ticket) -> Vec<Blocker> {
        let mut blockers = Vec::new();
        let mut push = |id: &TicketId, rule: u8| {
            blockers.push(Blocker {
                id: id.clone(),
                rule,
            });
        };
        let deferred = ticket.defer_until.is_some_and(|until| until >= self.now);
        if ticket.status != Status::Open || deferred || ticket.ticket_type == TicketType::Meta {
            push(&ticket.id, 1);
        }
        let dependencies = |source: &TicketId, rule: u8, push: &mut dyn FnMut(&TicketId, u8)| {
            for edge in self.outgoing.get(source).into_iter().flatten() {
                if edge.kind.blocks() && self.blocks(&edge.dst) {
                    push(&edge.dst, rule);
                }
            }
        };
        dependencies(&ticket.id, 2, &mut push);
        for ancestor in self.ancestors(ticket.parent.as_ref()) {
            dependencies(ancestor, 3, &mut push);
        }
        if ticket.last != 0
            && let Some(parent) = &ticket.parent
        {
            for sibling in self.descendants.get(parent).into_iter().flatten() {
                if **sibling != ticket.id && self.live(sibling).is_some_and(|s| s.last == 0) {
                    push(sibling, 4);
                }
            }
        }
        for child in self.descendants.get(&ticket.id).into_iter().flatten() {
            if self
                .live(child)
                .is_some_and(|c| c.parent.as_ref() == Some(&ticket.id))
            {
                push(child, 5);
            }
        }
        blockers.sort_by(|a, b| a.rule.cmp(&b.rule).then_with(|| a.id.cmp(&b.id)));
        blockers
    }
}

fn is_live(ticket: &Ticket) -> bool {
    ticket.status != Status::Closed
}

/// What holds each live ticket back; an empty list means ready. `tickets` must hold every live
/// ticket and all of their ancestors, and `edges` the outgoing edges of those tickets.
pub fn blockers(
    tickets: &[Ticket],
    edges: &[Edge],
    now: Timestamp,
) -> Vec<(TicketId, Vec<Blocker>)> {
    let readiness = Readiness::new(tickets, edges, now);
    tickets
        .iter()
        .filter(|t| is_live(t))
        .map(|t| (t.id.clone(), readiness.of(t)))
        .collect()
}

/// The ready tickets, in the order `blockers` received them.
pub fn ready(tickets: &[Ticket], edges: &[Edge], now: Timestamp) -> Vec<TicketId> {
    blockers(tickets, edges, now)
        .into_iter()
        .filter(|(_, blockers)| blockers.is_empty())
        .map(|(id, _)| id)
        .collect()
}

/// Blockers keyed by ticket, for callers that look tickets up by ID.
pub fn blockers_by_id(
    tickets: &[Ticket],
    edges: &[Edge],
    now: Timestamp,
) -> BTreeMap<TicketId, Vec<Blocker>> {
    blockers(tickets, edges, now).into_iter().collect()
}

#[cfg(test)]
#[path = "ready_tests.rs"]
mod tests;
