//! The allocation policy for the queue snapshot, including which work is parked.

use std::collections::BTreeMap;

use ignite_core::dispatch::{self, ReadyTicket};
use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{REVERTED_KEY, Ticket};
use ignite_core::shares::{self, EpicDemand, Group, Source};

use super::Snapshot;

/// The labels of tickets the hub files itself, which are offered whether or not an epic is
/// active. `hub` is on everything the hub files today (`upkeep::filing::HUB_LABEL`); the other
/// labels are the ones tickets filed before it carry, and `justiciar` marks the judge's
/// reports of faults in the system itself.
const HUB_FILED: [&str; 10] = [
    crate::upkeep::filing::HUB_LABEL,
    "regression",
    "improvement",
    "machine",
    "incomplete",
    "cache-audit",
    "graph-repair",
    "fleet",
    "flaky",
    dispatch::JUSTICIAR,
];

/// The label of a ticket set aside for a person, which no machine is offered.
const NEEDS_PERSON: &str = "needs:person";

impl Snapshot {
    /// The group a ticket shares workers in: its ancestors, nearest first, through
    /// [`shares::group_of`].
    pub(super) fn group(&self, id: &TicketId) -> Group {
        let ancestors: Vec<(TicketId, Option<u32>)> = self
            .ancestors(id)
            .map(|id| {
                let workers = self.nodes.get(&id).and_then(|n| n.workers);
                (id, workers)
            })
            .collect();
        if ancestors.is_empty()
            && let Some(source) = self.nodes.get(id).and_then(|node| node.source)
            && self.allocations.sources.contains_key(&source)
        {
            return Group::Source(source);
        }
        shares::group_of(&ancestors)
    }

    fn allocation(&self, group: &Group) -> Option<u32> {
        match group {
            Group::General => self.allocations.general,
            Group::Epic(id) => self.nodes.get(id).and_then(|node| node.workers),
            Group::Source(source) => self.allocations.sources.get(source).copied(),
        }
    }

    /// Automatic dispatch leaves parked tickets and work set aside for a person alone.
    /// A ticket started by name still follows the claim rules.
    pub(super) fn parked(&self, ticket: &Ticket, group: &Group) -> Option<&'static str> {
        if ticket.labels.iter().any(|label| label == NEEDS_PERSON) {
            return Some("set aside for a person (needs:person)");
        }
        let regression = self.requirements.is_regression(ticket.id.as_str());
        if regression
            || self.tip_blockers.contains(ticket.id.as_str())
            || ticket
                .labels
                .iter()
                .any(|label| label == dispatch::JUSTICIAR)
        {
            return None;
        }
        if ticket.allocation == Some(0) {
            return Some("its allocation is zero");
        }
        let source = self.nodes.get(&ticket.id).and_then(|node| node.source);
        if source.is_some_and(|source| self.allocations.sources.get(&source) == Some(&0)) {
            return Some("its automated source has allocation zero");
        }
        let allocation = self.allocation(group);
        if allocation == Some(0) {
            return Some(match group {
                Group::General => "the general pool has allocation zero",
                Group::Epic(_) => "its epic has allocation zero",
                Group::Source(_) => "its automated source has allocation zero",
            });
        }
        let filed_by_hub = ticket.metadata.contains_key(REVERTED_KEY)
            || ticket
                .labels
                .iter()
                .any(|label| HUB_FILED.contains(&label.as_str()));
        (matches!(group, Group::Epic(_)) && allocation.is_none() && !filed_by_hub)
            .then_some("its epic has no allocation, so the pool is never offered it")
    }

    /// Each group's share, cap, ready tickets and tickets in progress or reserved, across every
    /// machine, for the groups that have a ready ticket. Sorted by group, so the input order that
    /// breaks ties is the same on every request.
    pub(super) fn demand(&self, ready: &[ReadyTicket]) -> Vec<EpicDemand> {
        let mut groups: BTreeMap<Group, EpicDemand> = BTreeMap::new();
        for ticket in ready {
            let group = ticket.epic.clone();
            let cap = match &group {
                Group::Epic(id) => self.cap(id),
                _ => None,
            };
            groups
                .entry(group.clone())
                .or_insert_with(|| EpicDemand {
                    allocation: self.allocation(&group),
                    epic: group,
                    max_workers: cap,
                    ready: 0,
                    held: 0,
                })
                .ready += 1;
        }
        for (id, node) in &self.nodes {
            if (node.in_progress || self.reserved.contains_key(id.as_str()))
                && let Some(demand) = groups.get_mut(&self.group(id))
            {
                demand.held += 1;
            }
        }
        groups.into_values().collect()
    }
}

/// Find the source of both new automatic filings and reports that predate source controls.
pub(super) fn source(
    category: Option<&str>,
    flaky: bool,
    improvement: bool,
    upkeep: bool,
) -> Option<Source> {
    if flaky || matches!(category, Some("flaky" | "flaky-always" | "machine-flaky")) {
        Some(Source::Flaky)
    } else if category == Some("found") {
        Some(Source::Exploration)
    } else if improvement {
        Some(Source::Improvement)
    } else {
        (category.is_some() || upkeep).then_some(Source::Upkeep)
    }
}
