//! Dependency edits and the rule that an epic's prerequisites belong to its own plan.

use rusqlite::{Connection, OptionalExtension};

use ignite_core::protocol::tickets::EdgeKind;

use super::{Run, Step, rejected};
use crate::Result;
use crate::hub::{cycles, edges};

/// Nested epics can share work within the same outer epic.
fn outer_epic(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "WITH RECURSIVE ancestors(id, parent, type, depth) AS (
                 SELECT id, parent, type, 0 FROM tickets WHERE id = ?1
                 UNION ALL
                 SELECT t.id, t.parent, t.type, a.depth + 1
                   FROM tickets t JOIN ancestors a ON t.id = a.parent
             )
             SELECT id FROM ancestors WHERE type = 'epic' ORDER BY depth DESC LIMIT 1",
            [id],
            |row| row.get(0),
        )
        .optional()?)
}

impl Run<'_> {
    /// Adds `id` depends on `on`, or changes the kind or the `external` approval of an existing
    /// edge. `external` lets `on` lie outside `id`'s outermost epic; only the user approves it.
    pub(super) fn dependency(
        &mut self,
        id: &str,
        on: &str,
        kind: Option<EdgeKind>,
        external: bool,
    ) -> Step<()> {
        let dst = self.ticket(on)?.id.to_string();
        let kind = kind.unwrap_or_default();
        if edges::get(self.conn, id, &dst)? == Some((kind, external)) {
            return Ok(());
        }
        if id == dst {
            return rejected(format!("{id} cannot depend on itself"));
        }
        if kind.blocks() {
            if !external {
                self.check_epic_dependency(id, &dst)?;
            }
            cycles::check(self.conn, id, &dst)?;
        }
        edges::add_external(self.conn, id, &dst, kind, external)?;
        self.changed_edge(id, dst);
        Ok(())
    }

    /// Removes `id` depends on `on`. Removing with a kind only removes an edge of that kind.
    pub(super) fn remove_dependency(
        &mut self,
        id: &str,
        on: &str,
        kind: Option<EdgeKind>,
    ) -> Step<()> {
        let dst = self.ticket(on)?.id.to_string();
        let existing = edges::kind(self.conn, id, &dst)?;
        if existing.is_none() || kind.is_some_and(|k| Some(k) != existing) {
            return Ok(());
        }
        edges::remove(self.conn, id, &dst)?;
        self.changed_edge(id, dst);
        Ok(())
    }

    fn changed_edge(&mut self, id: &str, dst: String) {
        self.seq += 1;
        self.changed.insert(id.to_owned());
        self.changed.insert(dst);
    }

    fn check_epic_dependency(&self, src: &str, dst: &str) -> Step<()> {
        if let Some(epic) = outer_epic(self.conn, src)?
            && outer_epic(self.conn, dst)?.as_ref() != Some(&epic)
        {
            return rejected(format!(
                "{src} cannot depend on {dst} outside epic {epic}. Put the prerequisite in the same \
                 epic, or ask the user to approve an external dependency (`--external`)."
            ));
        }
        Ok(())
    }

    /// A move or type change can put either endpoint of an existing link outside the epic.
    /// Releasing a pending epic also checks its outgoing links, including links saved before
    /// this rule. Closed tickets with no live descendants need no check at release, and nor do
    /// links the user approved as external.
    pub(super) fn check_epic_dependencies(&self, root: &str, releasing: bool) -> Step<()> {
        let endpoint = if releasing {
            "src IN (SELECT id FROM live)"
        } else {
            "src IN (SELECT id FROM subtree) OR dst IN (SELECT id FROM subtree)"
        };
        let sql = format!(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT ?1 UNION ALL SELECT t.id FROM tickets t JOIN subtree s ON t.parent = s.id
             ),
             live(id) AS (
                 SELECT t.id FROM tickets t JOIN subtree s ON t.id = s.id WHERE t.status != 'closed'
                 UNION SELECT t.parent FROM tickets t JOIN live l ON t.id = l.id
                   WHERE t.parent IS NOT NULL
             )
             SELECT src, dst, kind, external FROM edges WHERE ({endpoint}) ORDER BY src, dst"
        );
        for edge in edges::collect(self.conn, &sql, &[root])? {
            if edge.kind.blocks() && !edge.external {
                self.check_epic_dependency(edge.src.as_str(), edge.dst.as_str())?;
            }
        }
        Ok(())
    }
}
