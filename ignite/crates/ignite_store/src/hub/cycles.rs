//! Dependency cycles. A batch refuses a new edge that could never become ready.
//!
//! Readiness (`ignite_core::ready`) holds a ticket back while any of these is live:
//!
//! - a ticket it depends on, or a descendant of one;
//! - a ticket that one of its ancestors depends on (so a ticket inherits its ancestors'
//!   dependencies);
//! - a child of its own.
//!
//! So `a` waits for `b` when `a` or an ancestor of `a` has a blocking edge to `b` or to an
//! ancestor of `b`, or when `b` is a child of `a`. An edge that closes a loop in that relation
//! leaves every ticket on the loop waiting for another one for good. The two common shapes are
//! a ticket that depends on itself and a ticket that depends on its own ancestor, which stays
//! live as long as the ticket does. Closed tickets never hold anything back, so they are left
//! out. Edges of the kinds `related` and `discovered-from` never block and are ignored.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rusqlite::Connection;

use crate::{Result, StoreError};

/// The live tickets and what holds each of them back, read from the database.
struct Graph {
    parent: BTreeMap<String, String>,
    children: BTreeMap<String, Vec<String>>,
    /// Blocking edges between live tickets: `src` depends on each of `edges[src]`.
    edges: BTreeMap<String, Vec<String>>,
    live: BTreeSet<String>,
}

impl Graph {
    fn load(conn: &Connection) -> Result<Graph> {
        let mut graph = Graph {
            parent: BTreeMap::new(),
            children: BTreeMap::new(),
            edges: BTreeMap::new(),
            live: BTreeSet::new(),
        };
        let mut select = conn.prepare("SELECT id, parent, status FROM tickets")?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, parent, status) in rows {
            if let Some(parent) = parent {
                graph.parent.insert(id.clone(), parent);
            }
            if status != "closed" {
                graph.live.insert(id);
            }
        }
        for (child, parent) in &graph.parent {
            if graph.live.contains(child) {
                graph
                    .children
                    .entry(parent.clone())
                    .or_default()
                    .push(child.clone());
            }
        }
        let mut select = conn.prepare(
            "SELECT src, dst FROM edges WHERE kind IN ('blocks', 'conditional-blocks', 'waits-for')",
        )?;
        let edges = select
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (src, dst) in edges {
            if graph.live.contains(&src) && graph.live.contains(&dst) {
                graph.edges.entry(src).or_default().push(dst);
            }
        }
        Ok(graph)
    }

    /// `id` and every live ticket below it.
    fn with_descendants(&self, id: &str) -> Vec<String> {
        let mut found = vec![id.to_owned()];
        let mut next = 0;
        while next < found.len() {
            for child in self.children.get(&found[next]).into_iter().flatten() {
                if !found.contains(child) {
                    found.push(child.clone());
                }
            }
            next += 1;
        }
        found
    }

    /// The live tickets that `id` waits for directly.
    fn waits_for(&self, id: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut current = Some(id.to_owned());
        let mut seen = BTreeSet::new();
        while let Some(ticket) = current {
            if !seen.insert(ticket.clone()) {
                break;
            }
            for dst in self.edges.get(&ticket).into_iter().flatten() {
                found.extend(self.with_descendants(dst));
            }
            current = self.parent.get(&ticket).cloned();
        }
        found.extend(self.children.get(id).into_iter().flatten().cloned());
        found
    }
}

/// The reason to refuse adding "`src` depends on `dst`", naming the loop it would make, or
/// `None` if the edge is safe. Tickets that are closed make no loop.
pub fn refusal(conn: &Connection, src: &str, dst: &str) -> Result<Option<String>> {
    if src == dst {
        return Ok(Some(format!("{src} cannot depend on itself")));
    }
    let graph = Graph::load(conn)?;
    if !graph.live.contains(src) || !graph.live.contains(dst) {
        return Ok(None);
    }
    // The new edge makes `src`, and everything below it, wait for `dst` and everything below it.
    let targets: BTreeSet<String> = graph.with_descendants(src).into_iter().collect();
    let mut came_from: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut queue = VecDeque::new();
    for start in graph.with_descendants(dst) {
        came_from.insert(start.clone(), None);
        queue.push_back(start);
    }
    while let Some(ticket) = queue.pop_front() {
        if targets.contains(&ticket) {
            let mut path = vec![ticket.clone()];
            let mut at = ticket;
            while let Some(Some(before)) = came_from.get(&at) {
                path.push(before.clone());
                at = before.clone();
            }
            path.reverse();
            let mut names = vec![src.to_owned()];
            names.extend(path);
            if names.last().map(String::as_str) != Some(src) {
                names.push(src.to_owned());
            }
            return Ok(Some(format!(
                "{src} depending on {dst} would make a cycle that can never become ready: {}",
                names.join(" waits for ")
            )));
        }
        for next in graph.waits_for(&ticket) {
            if !came_from.contains_key(&next) {
                came_from.insert(next.clone(), Some(ticket.clone()));
                queue.push_back(next);
            }
        }
    }
    Ok(None)
}

/// [`refusal`] as the error a batch returns.
pub fn check(conn: &Connection, src: &str, dst: &str) -> Result<()> {
    match refusal(conn, src, dst)? {
        Some(message) => Err(StoreError::Rejected(message)),
        None => Ok(()),
    }
}
