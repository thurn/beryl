//! Converts validated ticket fields to the column changes shared by creates and updates.

use serde_json::{Map, Value, json};

use ignite_core::protocol::tickets::TicketFields;

use super::{Run, Step, dedupe, rejected};

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
}
