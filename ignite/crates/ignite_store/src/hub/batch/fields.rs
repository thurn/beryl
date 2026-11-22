//! Converts validated ticket fields to the column changes shared by creates and updates.

use serde_json::{Map, Value, json};

use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{Status, Ticket, TicketFields, TicketType};
use ignite_core::shares;

use super::{Run, Step, dedupe, rejected, tickets};

impl Run<'_> {
    /// The set fields of an update or create as column changes, checked as the ticket service
    /// checked them. `parent` is left for the caller, which must resolve it.
    pub(super) fn field_changes(fields: &TicketFields) -> Step<Map<String, Value>> {
        if fields.title.as_deref() == Some("") {
            return rejected("title must have a value");
        }
        if matches!(&fields.assignee, Some(Some(a)) if a.is_empty()) {
            return rejected("assignee must have a value");
        }
        if fields.priority.is_some_and(|p| p > 4) {
            return rejected("priority must be at most 4");
        }
        if fields.last.is_some_and(|l| l > 1) {
            return rejected("last must be 0 or 1");
        }
        if matches!(fields.every_hours, Some(Some(0))) {
            return rejected("every_hours must be at least 1");
        }
        for (field, minutes) in [
            ("estimate_minutes", fields.estimate_minutes),
            ("unfiled_minutes", fields.unfiled_minutes),
        ] {
            if minutes == Some(Some(0)) {
                return rejected(format!("{field} must be positive or null"));
            }
        }
        if let Some(saving) = &fields.saving
            && (ignite_core::shares::Unit::parse(&saving.unit).is_none()
                || !saving.amount.is_finite()
                || saving.amount <= 0.0)
        {
            return rejected(
                "a saving needs a unit of dollars, agent-hours or core-hours and an amount above 0",
            );
        }
        let Ok(Value::Object(mut changes)) = serde_json::to_value(fields) else {
            unreachable!("ticket fields serialize to an object")
        };
        changes.remove("parent");
        if let Some(labels) = &fields.labels {
            changes.insert("labels".to_owned(), json!(dedupe(labels.clone())));
        }
        Ok(changes)
    }

    pub(super) fn update_allocation(
        ticket: &Ticket,
        fields: &TicketFields,
        changes: &mut Map<String, Value>,
    ) -> bool {
        let kind = fields.ticket_type.unwrap_or(ticket.ticket_type);
        Self::allocation_changes(fields, kind, changes);
        if !changes.contains_key("allocation")
            && kind.has_worker_share() != ticket.ticket_type.has_worker_share()
        {
            let weight = if kind.has_worker_share() {
                ticket.workers
            } else {
                shares::legacy_priority(fields.priority.unwrap_or(ticket.priority))
            };
            changes.insert("allocation".to_owned(), json!(weight));
        }
        let releasing = ticket.status == Status::PendingApproval
            && (fields.status == Some(Status::Open)
                || (fields.status.is_none() && fields.allocation.is_some_and(|n| n != Some(0))));
        if !kind.has_worker_share()
            && fields.status == Some(Status::Open)
            && fields.allocation.is_none()
            && ticket.allocation == Some(0)
            && matches!(ticket.status, Status::Open | Status::PendingApproval)
        {
            changes.insert(
                "allocation".to_owned(),
                json!(fields.priority.and_then(shares::legacy_priority)),
            );
            changes.insert("priority".to_owned(), json!(fields.priority.unwrap_or(2)));
        }
        if releasing {
            changes.insert("status".to_owned(), json!(Status::Open));
            if ticket.ticket_type == TicketType::Epic
                && ticket.workers.is_none()
                && fields.workers.is_none()
                && fields.allocation.is_none()
            {
                let allocation = json!(shares::DEFAULT_EPIC_ALLOCATION);
                changes.insert("workers".to_owned(), allocation.clone());
                changes.insert("allocation".to_owned(), allocation);
            }
        }
        releasing
    }

    /// Releases the pending line in the same transaction as its root's allocation change.
    /// A descendant with its own allocation of zero stays parked.
    pub(super) fn release_descendants(&mut self, root: &TicketId) -> Step<()> {
        let mut below: Vec<_> = tickets::descendants(self.conn, root.as_str())?
            .into_iter()
            .collect();
        below.sort_unstable();
        for id in below {
            if let Some(ticket) = tickets::load(self.conn, &id)?
                && ticket.status == Status::PendingApproval
            {
                self.save(&ticket, Self::opening())?;
            }
        }
        Ok(())
    }

    /// Translates the legacy fields at the write boundary so old clients and the new editor
    /// use the same allocation. A legacy worker setting on a task may still group its children.
    pub(super) fn allocation_changes(
        fields: &TicketFields,
        kind: TicketType,
        changes: &mut Map<String, Value>,
    ) {
        let container = kind.has_worker_share();
        if let Some(allocation) = fields.allocation {
            let value = json!(allocation);
            if container {
                changes.insert("workers".to_owned(), value);
            } else {
                changes.insert(
                    "priority".to_owned(),
                    json!(shares::priority_band(allocation)),
                );
            }
        } else if container {
            if let Some(workers) = fields.workers {
                changes.insert("allocation".to_owned(), json!(workers));
            }
        } else if let Some(priority) = fields.priority {
            changes.insert(
                "allocation".to_owned(),
                json!(shares::legacy_priority(priority)),
            );
        }
    }
}
