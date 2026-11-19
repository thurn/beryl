//! What closing a ticket does to its ancestors.
//!
//! Each ancestor left with no live descendant closes, nearest first, except an epic with an
//! acceptance. Such an epic gets one new child, "Verify <epic>'s acceptance", whose session
//! measures the acceptance, records the numbers on the epic, and either closes the child (which
//! closes the epic) or files the missing work under the epic first. When that verify child is
//! the ticket that just closed, the epic closes; when anything else was last, such as the
//! missing work a verify session filed, a new verify child is filed. An epic with an empty
//! acceptance closes as soon as its last descendant does.

use rusqlite::OptionalExtension;
use serde_json::json;

use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{Status, Ticket, TicketFields, TicketType};

use super::{Run, Step, parse_id};
use crate::hub::tickets;

/// The metadata key that marks a verify child, holding the ID of the epic it verifies.
const VERIFIES_ACCEPTANCE: &str = "verifies_acceptance";

/// The label prefix that gives a ticket its model tier.
const TIER_PREFIX: &str = "tier:";

impl Run<'_> {
    /// Closes each ancestor of `id`, nearest first, until one that still has a live
    /// descendant, or an epic that needs its acceptance verified, which gets a verify child
    /// instead. Returns the ancestors it closed and the verify children it filed.
    pub(super) fn close_ancestors(
        &mut self,
        id: &str,
        reason: Option<&str>,
    ) -> Step<(Vec<TicketId>, Vec<TicketId>)> {
        let mut closed = Vec::new();
        let closing = tickets::load(self.conn, id)?;
        for parent in self.ancestors(id)? {
            if self.has_live_descendant(&parent)? {
                break;
            }
            let Some(ticket) = tickets::load(self.conn, &parent)? else {
                continue;
            };
            if ticket.status == Status::Closed {
                continue;
            }
            if needs_verifying(&ticket, closing.as_ref()) {
                let filed = self.file_verify_child(&ticket)?;
                return Ok((closed, vec![parse_id(&filed)?]));
            }
            self.save(&ticket, self.closing(reason))?;
            closed.push(ticket.id);
        }
        Ok((closed, Vec::new()))
    }

    /// The ancestors of `id`, nearest first.
    pub(super) fn ancestors(&self, id: &str) -> Step<Vec<String>> {
        let mut found: Vec<String> = Vec::new();
        let mut current = id.to_owned();
        loop {
            let parent: Option<Option<String>> = self
                .conn
                .query_row(
                    "SELECT parent FROM tickets WHERE id = ?1",
                    [&current],
                    |row| row.get(0),
                )
                .optional()?;
            match parent.flatten() {
                Some(parent) if !found.contains(&parent) => {
                    found.push(parent.clone());
                    current = parent;
                }
                _ => return Ok(found),
            }
        }
    }

    fn has_live_descendant(&self, id: &str) -> Step<bool> {
        let live: Option<i64> = self
            .conn
            .query_row(
                "WITH RECURSIVE below(id) AS ( \
                   SELECT id FROM tickets WHERE parent = ?1 \
                   UNION ALL SELECT tickets.id FROM below JOIN tickets ON tickets.parent = below.id) \
                 SELECT 1 FROM below JOIN tickets USING (id) WHERE status != 'closed' LIMIT 1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(live.is_some())
    }

    /// Files the child that verifies `epic`'s acceptance, released and at the epic's
    /// priority, with the model tier of its most recently closed child that has one.
    fn file_verify_child(&mut self, epic: &Ticket) -> Step<String> {
        let id = epic.id.as_str();
        let fields = TicketFields {
            title: Some(format!("Verify {id}'s acceptance")),
            body: Some(format!(
                "Every other child of epic {id} is closed. Measure the epic's acceptance, \
                 quoted below, and record the numbers on {id} as a comment.\n\
                 - If it holds, close this ticket, which closes {id}.\n\
                 - If it does not, file the missing work as children of {id}, then close this \
                 ticket. When that work closes, another verify ticket is filed.\n\n\
                 {id}'s acceptance:\n{}",
                epic.acceptance.trim()
            )),
            acceptance: Some(format!(
                "The numbers for {id}'s acceptance are recorded on it, and either it holds or the \
                 missing work is filed under {id}."
            )),
            ticket_type: Some(TicketType::Task),
            status: Some(Status::Open),
            priority: Some(epic.priority),
            labels: Some(self.sibling_tier(id)?.into_iter().collect()),
            metadata: Some([(VERIFIES_ACCEPTANCE.to_owned(), json!(id))].into()),
            ..TicketFields::default()
        };
        self.create(&fields, Some(id), &[], None)
    }

    /// The tier label of `epic`'s most recently closed child that has one.
    fn sibling_tier(&self, epic: &str) -> Step<Option<String>> {
        let mut statement = self.conn.prepare(
            "SELECT labels FROM tickets WHERE parent = ?1 ORDER BY closed_at DESC, id DESC",
        )?;
        let rows = statement.query_map([epic], |row| row.get::<_, String>(0))?;
        for labels in rows {
            let labels: Vec<String> = serde_json::from_str(&labels?).unwrap_or_default();
            if let Some(tier) = labels.into_iter().find(|l| l.starts_with(TIER_PREFIX)) {
                return Ok(Some(tier));
            }
        }
        Ok(None)
    }
}

/// Whether `ancestor`, now left with no live descendant, gets a verify child instead of
/// closing: it is an epic with an acceptance, and the ticket that just closed is not its own
/// verify child.
fn needs_verifying(ancestor: &Ticket, closing: Option<&Ticket>) -> bool {
    if ancestor.ticket_type != TicketType::Epic || ancestor.acceptance.trim().is_empty() {
        return false;
    }
    let verified = closing.is_some_and(|closing| {
        closing.parent.as_ref() == Some(&ancestor.id)
            && closing.metadata.get(VERIFIES_ACCEPTANCE) == Some(&json!(ancestor.id.as_str()))
    });
    !verified
}
