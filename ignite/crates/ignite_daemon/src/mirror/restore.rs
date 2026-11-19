//! `ignite hub restore --from-mirror`: sends the hub each ticket as this machine's mirror
//! recorded it at a moment, through `POST /import`.
//!
//! - With `--at <time>`, each ticket is taken as it stood at that time, and the import is forced,
//!   so it replaces the hub's later copy. This rolls back bad edits. A ticket created after
//!   that time has no version to go back to and is left as it is.
//! - With `--actor <name>` as well, only the tickets that `<name>` changed after that time are
//!   rolled back, as found in the history of each ticket's newest version.
//! - Without `--at`, the newest version of every ticket is sent, without forcing, which seeds a
//!   new or emptied hub.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{ImportReply, ImportRequest, WholeTicket};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::mirror;

/// The most tickets one import carries, as the hub allows.
const PAGE: usize = 200;

/// What a restore sends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The tickets to send, each parent before its children.
    pub tickets: Vec<WholeTicket>,
    /// Whether the import replaces later copies (a rollback to a moment).
    pub force: bool,
    /// Tickets the rollback covers that did not exist yet at its moment, which are left alone.
    pub created_later: Vec<TicketId>,
}

/// The plan for restoring the hub from the mirror in `conn` as of `at`, limited to what `actor`
/// changed after `at` if given. `actor` without `at` is refused.
pub fn plan(conn: &Connection, at: Option<Timestamp>, actor: Option<&str>) -> Result<Plan> {
    if actor.is_some() && at.is_none() {
        return Err(Error::Invalid(
            "--actor needs --at, the moment to roll that actor's changes back to".to_owned(),
        ));
    }
    let at_text = at.map(|at| at.to_string());
    let mut newest = decode(mirror::as_of(conn, None).map_err(|e| store(&e))?)?;
    let mut then = decode(mirror::as_of(conn, at_text.as_deref()).map_err(|e| store(&e))?)?;
    // A ticket that moved to another parent took a new ID, and the mirror still holds its last
    // version under the old one. The new ticket lists the old ID among its aliases, so the old
    // copy is left out; the hub would refuse it as colliding with that alias.
    let moved_away: BTreeSet<String> = newest
        .values()
        .flat_map(|whole| whole.aliases.iter().cloned())
        .collect();
    let kept = |id: &TicketId| !moved_away.contains(&id.as_str().to_ascii_lowercase());
    then.retain(|id, _| kept(id));
    newest.retain(|id, _| kept(id));
    let Some(at) = at else {
        return Ok(Plan {
            tickets: parents_first(then.into_values().collect()),
            force: false,
            created_later: Vec::new(),
        });
    };
    let covered: BTreeSet<TicketId> = newest
        .values()
        .filter(|whole| actor.is_none_or(|actor| changed_by(whole, actor, at)))
        .map(|whole| whole.ticket.id.clone())
        .collect();
    let created_later = covered
        .iter()
        .filter(|id| !then.contains_key(*id))
        .cloned()
        .collect();
    let tickets = then
        .into_iter()
        .filter(|(id, _)| covered.contains(id))
        .map(|(_, whole)| whole)
        .collect();
    Ok(Plan {
        tickets: parents_first(tickets),
        force: true,
        created_later,
    })
}

/// Sends the plan's tickets through `import`, a page at a time; how many tickets the hub changed.
pub fn send(
    plan: &Plan,
    mut import: impl FnMut(&ImportRequest) -> Result<ImportReply>,
) -> Result<u32> {
    let mut imported = 0;
    for page in plan.tickets.chunks(PAGE) {
        let reply = import(&ImportRequest {
            tickets: page.to_vec(),
            force: plan.force,
        })?;
        imported += reply.imported;
    }
    Ok(imported)
}

/// Whether `whole`'s history has a change by `actor` after `at`.
fn changed_by(whole: &WholeTicket, actor: &str, at: Timestamp) -> bool {
    whole
        .history
        .iter()
        .any(|history| history.actor == actor && history.at > at)
}

/// Orders tickets so each comes after its parent, as the hub refuses a ticket whose parent it
/// does not have yet. A parent outside the list counts as present.
fn parents_first(tickets: Vec<WholeTicket>) -> Vec<WholeTicket> {
    let parents: BTreeMap<TicketId, Option<TicketId>> = tickets
        .iter()
        .map(|whole| (whole.ticket.id.clone(), whole.ticket.parent.clone()))
        .collect();
    let depth = |id: &TicketId| {
        let mut depth = 0;
        let mut current = parents.get(id).cloned().flatten();
        // The hub refuses parent cycles, so the walk ends; the bound guards a corrupt mirror.
        while let Some(parent) = current
            && depth < parents.len()
        {
            depth += 1;
            current = parents.get(&parent).cloned().flatten();
        }
        depth
    };
    let mut ordered: Vec<(usize, WholeTicket)> = tickets
        .into_iter()
        .map(|whole| (depth(&whole.ticket.id), whole))
        .collect();
    ordered.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.ticket.id.cmp(&y.ticket.id)));
    ordered.into_iter().map(|(_, whole)| whole).collect()
}

fn decode(versions: Vec<mirror::Version>) -> Result<BTreeMap<TicketId, WholeTicket>> {
    versions
        .into_iter()
        .map(|version| {
            let whole: WholeTicket = serde_json::from_str(&version.whole_json).map_err(|e| {
                Error::Invalid(format!(
                    "the mirror's version {} of {}: {e}",
                    version.id, version.ticket
                ))
            })?;
            Ok((whole.ticket.id.clone(), whole))
        })
        .collect()
}

fn store(error: &ignite_store::StoreError) -> Error {
    Error::Io(format!("ticket mirror: {error}"))
}
