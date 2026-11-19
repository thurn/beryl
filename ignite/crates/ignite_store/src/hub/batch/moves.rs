//! Moving a ticket to another parent, or detaching it from its parent. A ticket's ID always
//! extends its parent's (see `hub::renumber`), so a move gives the ticket its new parent's next
//! child number, or a new root ID when it is detached, and renumbers every ticket below it the
//! same way. The old IDs become aliases.

use rusqlite::params;
use serde_json::{Map, json};

use ignite_core::protocol::tickets::{Ticket, TicketType};

use super::{Run, Step, rejected, roots};
use crate::hub::{renumber, tickets};

impl Run<'_> {
    /// The next child ID under `parent`, which it takes by raising the parent's `next_child`.
    pub(super) fn take_child_id(&mut self, parent: &Ticket) -> Step<String> {
        self.conn.execute(
            "UPDATE tickets SET next_child = ?1 WHERE id = ?2",
            params![parent.next_child + 1, parent.id.as_str()],
        )?;
        self.changed.insert(parent.id.to_string());
        Ok(format!("{}.{}", parent.id, parent.next_child))
    }

    /// Moves `ticket` under the ticket named `target`, or detaches it when `target` is `None`,
    /// and returns it as it stands under its new ID. A move to the parent it already has does
    /// nothing. Refuses a move that would make a ticket its own ancestor, and a move of a
    /// ticket while it or a ticket below it is being worked, since its build tree, branch and
    /// session title carry the ID.
    pub(super) fn move_ticket(&mut self, ticket: &Ticket, target: Option<&str>) -> Step<Ticket> {
        let parent = target.map(|name| self.ticket(name)).transpose()?;
        if parent.as_ref().map(|p| &p.id) == ticket.parent.as_ref() {
            return Ok(ticket.clone());
        }
        let old = ticket.id.to_string();
        if let Some(parent) = &parent
            && (parent.id == ticket.id || self.ancestors(parent.id.as_str())?.contains(&old))
        {
            return rejected("A ticket cannot become its own ancestor");
        }
        if let Some(busy) = self.worked_in_subtree(&old)? {
            return rejected(format!(
                "{busy} is being worked, so {old} cannot move: its tree, branch and session \
                 title carry the ID. Release it first."
            ));
        }
        let new = match &parent {
            Some(parent) => self.take_child_id(parent)?,
            None => roots::allocate(
                self.conn,
                renumber::root_letter(&old, ticket.ticket_type == TicketType::Epic),
            )?,
        };
        let renames = renumber::subtree(self.conn, &old, &new)?;
        // Tickets that depend on a renamed one list it among their edges, so they change too.
        let dependents = self.dependents(&renames)?;
        renumber::apply(self.conn, &renames)?;
        for (old_id, new_id) in &renames {
            let mut change = Map::new();
            change.insert("id".to_owned(), json!([old_id, new_id]));
            change.insert(
                "parent".to_owned(),
                json!([renumber::parent_of(old_id), renumber::parent_of(new_id)]),
            );
            self.seq += 1;
            self.conn.execute(
                "UPDATE tickets SET updated_at = ?1, version = ?2 WHERE id = ?3",
                params![self.now.to_string(), self.seq as i64, new_id],
            )?;
            self.history(new_id, &change)?;
            self.changed.remove(old_id);
            self.changed.insert(new_id.clone());
            for created in self.created.values_mut() {
                if created == old_id {
                    created.clone_from(new_id);
                }
            }
        }
        if let Some(old_parent) = &ticket.parent {
            self.changed.insert(old_parent.to_string());
        }
        self.changed.extend(dependents);
        match tickets::load(self.conn, &new)? {
            Some(moved) => Ok(moved),
            None => rejected(format!("{new} vanished while it moved")),
        }
    }

    /// A ticket at or below `id` that is being worked: claimed, or held by a build tree.
    fn worked_in_subtree(&self, id: &str) -> Step<Option<String>> {
        let mut statement = self.conn.prepare(
            "SELECT id FROM tickets WHERE (id = ?1 OR instr(id, ?1 || '.') = 1) \
             AND status != 'closed' \
             AND (status = 'in_progress' OR assignee IS NOT NULL OR tree IS NOT NULL) \
             ORDER BY id LIMIT 1",
        )?;
        let mut rows = statement.query_map([id], |row| row.get::<_, String>(0))?;
        Ok(rows.next().transpose()?)
    }

    /// The tickets outside `renames` with an edge to a ticket in it, under their current IDs.
    fn dependents(&self, renames: &[(String, String)]) -> Step<Vec<String>> {
        let mut found = Vec::new();
        let mut statement = self.conn.prepare("SELECT src FROM edges WHERE dst = ?1")?;
        for (old, _) in renames {
            for src in statement.query_map([old], |row| row.get::<_, String>(0))? {
                let src = src?;
                if !renames.iter().any(|(renamed, _)| *renamed == src) {
                    found.push(src);
                }
            }
        }
        Ok(found)
    }
}
