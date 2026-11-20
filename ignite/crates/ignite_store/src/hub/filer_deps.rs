//! The dependency a ticket gets by default on the unlanded work of the session that filed it.
//!
//! A session that files a follow-up to its own unlanded change usually forgets to make the
//! follow-up depend on that change, and the pool hands the follow-up out within minutes, to a
//! session that cannot find the code it builds on. The hub therefore adds the dependency itself,
//! unless the filer opts out (`ignite ticket create --independent`).

use rusqlite::Connection;

use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{BatchRequest, Dependency, EdgeKind, Operation, TicketType};

use crate::Result;
use crate::hub::tickets::{collect_strings, resolve};

/// A dependency [`add_filer_dependencies`] added.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AddedDependency {
    /// The index of the create operation in the batch.
    pub op: usize,
    /// The ticket the new one now depends on: one the filer holds, unlanded.
    pub on: TicketId,
}

/// Adds a `blocks` dependency to each create in `batch` on every ticket that `actor` holds
/// (assignee, `in_progress`, not landed) that has the new ticket's parent. A create that
/// already lists the ticket, and an epic, gets none. A new ticket under a parent named
/// `$<index>` (created in the same batch) gets none. A root ticket matches a held root ticket,
/// since both have no parent. A child of the held ticket gets none either: a ticket waits for
/// its children, so the child cannot also wait for it (the hub rejects that as a cycle).
/// Returns what it added.
pub fn add_filer_dependencies(
    conn: &Connection,
    batch: &mut BatchRequest,
    actor: &str,
) -> Result<Vec<AddedDependency>> {
    let held = collect_strings(
        conn,
        "SELECT id FROM tickets \
         WHERE assignee = ?1 AND status = 'in_progress' AND landed_at IS NULL ORDER BY id",
        &[actor],
    )?;
    let mut added = Vec::new();
    if held.is_empty() {
        return Ok(added);
    }
    let mut held_parents = Vec::new();
    for id in &held {
        let parent = collect_strings(
            conn,
            "SELECT parent FROM tickets WHERE id = ?1 AND parent IS NOT NULL",
            &[id],
        )?
        .pop();
        held_parents.push(parent);
    }
    for (index, operation) in batch.ops.iter_mut().enumerate() {
        let Operation::Create {
            fields,
            parent,
            deps,
            ..
        } = operation
        else {
            continue;
        };
        if fields.ticket_type == Some(TicketType::Epic) {
            continue;
        }
        let name = parent
            .clone()
            .or_else(|| fields.parent.clone().flatten().map(String::from));
        let new_parent = match name.as_deref() {
            None => None,
            Some(name) if name.starts_with('$') => continue,
            Some(name) => match resolve(conn, name)? {
                Some(id) => Some(id.to_string()),
                None => continue,
            },
        };
        let mut listed = Vec::new();
        for dependency in deps.iter() {
            if let Some(id) = resolve(conn, &dependency.on)? {
                listed.push(id.to_string());
            }
        }
        for (id, held_parent) in held.iter().zip(&held_parents) {
            if *held_parent != new_parent || listed.contains(id) {
                continue;
            }
            deps.push(Dependency {
                on: id.clone(),
                kind: Some(EdgeKind::Blocks),
            });
            added.push(AddedDependency {
                op: index,
                on: id
                    .parse()
                    .map_err(|e| crate::StoreError::Invalid(format!("ticket ID: {e}")))?,
            });
        }
    }
    Ok(added)
}
