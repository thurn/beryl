//! Tickets: reading, listing, the graph and ID resolution, ported from the ticket service.
//!
//! A ticket row converts to the protocol's [`Ticket`] through a JSON object whose keys are the
//! column names (`type` included, which is how `Ticket` serializes), so that the batch can diff
//! two tickets field by field and write back exactly the columns that changed.
//!
//! Readiness needs every live ticket and all of their ancestors, so each read that reports it
//! loads that set (a few hundred rows) and hands it to [`ignite_core::ready`].

use std::collections::HashSet;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use serde_json::{Map, Value, json};

use ignite_core::ids::TicketId;
use std::collections::BTreeMap;

use ignite_core::ids::Seq;
use ignite_core::protocol::tickets::{
    BatchRequest, Blocker, Comment, Edge, Graph, GraphTicket, History, Operation, Status, Ticket,
    TicketDetail, TicketFields, TicketQuery, TicketType,
};
use ignite_core::ready::Readiness;
use ignite_core::shares;

use crate::hub::batch::roots::PREFIXES as ROOT_PREFIXES;
use ignite_core::time::Timestamp;

use crate::hub::{edges, feed};
use crate::{Result, StoreError};

/// Columns that hold JSON text.
const JSON_COLUMNS: &[&str] = &["labels", "metadata", "landed", "saving"];

/// Every column of `tickets`, in the order of the schema. `select`
/// names them, so a column that a newer build added to the table is never read by an older one.
pub(crate) const TICKET_COLUMNS: &str = "id, parent, next_child, title, body, acceptance, design, notes, \
    type, status, priority, assignee, last, labels, metadata, defer_until, close_reason, \
    created_at, created_by, updated_at, closed_at, version, workers, max_workers, cores, \
    machine, session, tree, claimed_at, landed_at, landed, saving, needs_os, fallback, \
    every_hours, provider, estimate_minutes, unfiled_minutes";

/// One ticket, found by ID or by any alias, with its edges, children and readiness.
pub fn get(
    conn: &Connection,
    id: &str,
    comments: bool,
    history: bool,
    now: Timestamp,
) -> Result<Option<TicketDetail>> {
    let Some(id) = resolve(conn, id)? else {
        return Ok(None);
    };
    let Some(ticket) = load(conn, id.as_str())? else {
        return Ok(None);
    };
    let blocked_by = readiness(conn, std::slice::from_ref(&ticket), now)?.remove(0);
    let children = collect_ids(
        conn,
        "SELECT id FROM tickets WHERE parent = ?1 ORDER BY id",
        &[id.as_str()],
    )?;
    Ok(Some(TicketDetail {
        edges: edges::of(conn, id.as_str())?,
        children,
        ready: blocked_by.is_empty(),
        blocked_by,
        comments: comments
            .then(|| self::comments(conn, id.as_str()))
            .transpose()?,
        history: history
            .then(|| self::history(conn, id.as_str()))
            .transpose()?,
        aliases: collect_strings(
            conn,
            "SELECT alias FROM aliases WHERE id = ?1 ORDER BY alias",
            &[id.as_str()],
        )?,
        ticket,
    }))
}

/// The tickets a `GET /tickets` query selects: live tickets sorted by ID by default; with `all`
/// or a closed `status`, every matching ticket sorted by ID; with `search`, matches by title,
/// ID prefix or alias prefix in order of priority and then newest first.
pub fn list(conn: &Connection, query: &TicketQuery, now: Timestamp) -> Result<Vec<Ticket>> {
    let search = query.search.as_deref();
    let include_closed = query.all
        || query.status == Some(Status::Closed)
        || search.is_some() && query.status.is_none();
    let limit = query.limit.map_or(usize::MAX, |limit| limit as usize);
    if limit == 0 || query.ready && query.status.is_some_and(|s| s != Status::Open) {
        return Ok(Vec::new());
    }
    let required = |name: &Option<TicketId>| -> Result<Option<TicketId>> {
        match name {
            None => Ok(None),
            Some(name) => Ok(Some(
                resolve(conn, name.as_str())?.unwrap_or_else(|| name.clone()),
            )),
        }
    };
    let parent = required(&query.parent)?;
    let ancestor = required(&query.ancestor)?;
    let below = ancestor
        .as_ref()
        .map(|ancestor| descendants(conn, ancestor.as_str()))
        .transpose()?;

    let (live, live_edges) = snapshot(conn, &[])?;
    let readiness = Readiness::new(&live, &live_edges, now);
    let candidates: Vec<Ticket> = if let Some(phrase) = search {
        search_candidates(conn, phrase, query.status)?
    } else if include_closed && !query.ready {
        archived_candidates(conn, query.status)?
    } else {
        live.iter()
            .filter(|t| t.status != Status::Closed)
            .cloned()
            .collect()
    };

    let mut results = Vec::new();
    for ticket in candidates {
        if query.status.is_some_and(|s| s != ticket.status)
            || query.ticket_type.is_some_and(|t| t != ticket.ticket_type)
            || query
                .assignee
                .as_ref()
                .is_some_and(|a| ticket.assignee.as_ref() != Some(a))
            || parent
                .as_ref()
                .is_some_and(|p| ticket.parent.as_ref() != Some(p))
            || below
                .as_ref()
                .is_some_and(|ids| !ids.contains(ticket.id.as_str()))
        {
            continue;
        }
        if query.label_any.is_empty() {
            if !query.label.iter().all(|l| ticket.labels.contains(l)) {
                continue;
            }
        } else if !matches_labels(conn, &ticket, query)? {
            continue;
        }
        if query.ready && !readiness.of(&ticket).is_empty() {
            continue;
        }
        results.push(ticket);
        if results.len() == limit {
            break;
        }
    }
    Ok(results)
}

/// What holds each of `tickets` back, in order; an empty list means ready.
pub fn readiness(
    conn: &Connection,
    tickets: &[Ticket],
    now: Timestamp,
) -> Result<Vec<Vec<Blocker>>> {
    let seeds: Vec<&str> = tickets.iter().map(|t| t.id.as_str()).collect();
    let (known, known_edges) = snapshot(conn, &seeds)?;
    let readiness = Readiness::new(&known, &known_edges, now);
    Ok(tickets.iter().map(|ticket| readiness.of(ticket)).collect())
}

/// The ready tickets, sorted by ID.
pub fn ready(conn: &Connection, now: Timestamp) -> Result<Vec<TicketId>> {
    let (known, known_edges) = snapshot(conn, &[])?;
    Ok(ignite_core::ready::ready(&known, &known_edges, now))
}

/// `GET /graph`: the live tickets sorted by ID without their long text fields (`body`, `design`
/// and `notes` are empty), each with its readiness, and the edges between them. A ticket's own
/// state (rule 1) is left out of its `blocked_by`, as the ticket service did, since the
/// ticket's status already says it. With `all`, closed tickets follow, never ready, with every
/// edge.
pub fn graph(conn: &Connection, all: bool, now: Timestamp) -> Result<Graph> {
    let (known, known_edges) = snapshot(conn, &[])?;
    let readiness = Readiness::new(&known, &known_edges, now);
    let short = |ticket: &Ticket| Ticket {
        body: String::new(),
        design: String::new(),
        notes: String::new(),
        ..ticket.clone()
    };
    let mut tickets: Vec<GraphTicket> = known
        .iter()
        .filter(|t| t.status != Status::Closed)
        .map(|ticket| {
            let blockers = readiness.of(ticket);
            GraphTicket {
                ready: blockers.is_empty(),
                blocked_by: blockers.into_iter().filter(|b| b.rule != 1).collect(),
                ticket: short(ticket),
            }
        })
        .collect();
    if all {
        for ticket in select(conn, "WHERE status = 'closed' ORDER BY id", &[])? {
            tickets.push(GraphTicket {
                ready: false,
                blocked_by: Vec::new(),
                ticket: short(&ticket),
            });
        }
    }
    Ok(Graph {
        tickets,
        edges: edges::graph(conn, all)?,
        seq: feed::current(conn)?,
    })
}

/// The ticket an alias names, or the ID itself if it is a ticket. Names are matched without
/// regard to case. Accepts the ID as stored and shown (`TQ44S.7`), the earlier spellings
/// (`Tq44s.7` and a bare `q44s.7`), an alias from an earlier tracker (`old`), and an imported
/// form with a project prefix (`by-q44s.7` or `by-old`).
pub fn resolve(conn: &Connection, id_or_alias: &str) -> Result<Option<TicketId>> {
    let candidate = candidate_id(conn, id_or_alias)?;
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM tickets WHERE id = ?1",
            [&candidate],
            |row| row.get(0),
        )
        .optional()?;
    exists
        .map(|id| {
            id.parse()
                .map_err(|e| StoreError::Invalid(format!("ticket ID: {e}")))
        })
        .transpose()
}

/// The ID a name refers to, whether or not that ticket exists: the ticket the name spells in
/// any case, with or without its leading letter, the ticket an alias points to, or the name
/// without a project prefix.
pub(crate) fn candidate_id(conn: &Connection, name: &str) -> Result<String> {
    // Ticket IDs are stored in capitals with a leading `T` (`TQ44S.7`). Before that they were
    // stored in lowercase without the `T` and printed with it (`Tq44s.7`), and text written
    // then still uses that spelling, so both are looked up. Root IDs carried over from earlier
    // trackers hold dashes (`TCI-00031CED38`), so a name that spells a ticket is taken as it is
    // before any dash is read as a project prefix.
    if let Some(id) = spelled_ticket(conn, name)? {
        return Ok(id);
    }
    // Otherwise a dash marks an imported name with its project prefix (`by-q44s.7`). Aliases
    // are stored in lowercase.
    let lower = name.to_ascii_lowercase();
    let stripped = lower
        .split_once('-')
        .map_or(lower.as_str(), |(_, rest)| rest);
    let alias: Option<String> = conn
        .query_row(
            "SELECT id FROM aliases WHERE alias = ?1 OR alias = ?2 ORDER BY alias = ?1 DESC LIMIT 1",
            [stripped, lower.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(alias) = alias {
        return Ok(alias);
    }
    if let Some(id) = spelled_ticket(conn, stripped)? {
        return Ok(id);
    }
    Ok(canonical(stripped))
}

/// The ticket `name` spells in any case, with or without its leading letter (`T`, `E`, `M`,
/// `A` or `B`).
/// A root is used under only one letter, so at most one candidate with a letter added exists.
pub(crate) fn spelled_ticket(conn: &Connection, name: &str) -> Result<Option<String>> {
    let upper = name.to_ascii_uppercase();
    let added = ROOT_PREFIXES
        .iter()
        .map(|letter| format!("{letter}{upper}"));
    for candidate in std::iter::once(upper.clone()).chain(added) {
        let found: Option<i64> = conn
            .query_row("SELECT 1 FROM tickets WHERE id = ?1", [&candidate], |row| {
                row.get(0)
            })
            .optional()?;
        if found.is_some() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// A name in the form IDs are stored in: capitals, with a leading `T` (or the `E` of an epic,
/// the `M` of a meta-ticket, the `A` of a ticket Ignite filed on its own or the `B` of a bug
/// report).
pub(crate) fn canonical(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with(ROOT_PREFIXES) {
        upper
    } else {
        format!("T{upper}")
    }
}

/// One ticket by its exact ID.
pub fn load(conn: &Connection, id: &str) -> Result<Option<Ticket>> {
    Ok(select(conn, "WHERE id = ?1", &[id])?.pop())
}

/// A ticket's comments, oldest first.
pub fn comments(conn: &Connection, id: &str) -> Result<Vec<Comment>> {
    let mut statement = conn.prepare(
        "SELECT ticket, n, author, at, text, legacy_id FROM comments WHERE ticket = ?1 ORDER BY n",
    )?;
    let rows = statement.query_map([id], |row| object(row, &[]))?;
    rows.map(|row| from_object(row?)).collect()
}

/// A ticket's history, oldest first, each change parsed from its JSON.
pub fn history(conn: &Connection, id: &str) -> Result<Vec<History>> {
    let mut statement = conn.prepare(
        "SELECT ticket, seq, actor, at, change FROM history WHERE ticket = ?1 ORDER BY seq",
    )?;
    let rows = statement.query_map([id], |row| object(row, &["change"]))?;
    rows.map(|row| from_object(row?)).collect()
}

/// Every live ticket and every ancestor of one, with the ancestors of `seeds` too, sorted by
/// ID, and the outgoing edges of all of them: what [`Readiness`] needs.
pub(crate) fn snapshot(conn: &Connection, seeds: &[&str]) -> Result<(Vec<Ticket>, Vec<Edge>)> {
    let placeholders = vec!["?"; seeds.len()].join(", ");
    let seeded = if seeds.is_empty() {
        String::new()
    } else {
        format!(" OR id IN ({placeholders})")
    };
    let sources = format!(
        "WITH RECURSIVE sources(id, parent) AS ( \
           SELECT id, parent FROM tickets WHERE status != 'closed'{seeded} \
           UNION SELECT tickets.id, tickets.parent FROM sources \
             JOIN tickets ON tickets.id = sources.parent)"
    );
    let tickets = select(
        conn,
        &format!("WHERE id IN ({sources} SELECT id FROM sources) ORDER BY id"),
        seeds,
    )?;
    let edges = edges::collect(
        conn,
        &format!(
            "{sources} SELECT src, dst, kind FROM sources JOIN edges ON edges.src = sources.id \
             ORDER BY src, dst"
        ),
        seeds,
    )?;
    Ok((tickets, edges))
}

/// Tickets selected by a `WHERE` (and `ORDER BY`) clause.
pub(crate) fn select(conn: &Connection, clause: &str, args: &[&str]) -> Result<Vec<Ticket>> {
    let mut statement = conn.prepare(&format!("SELECT {TICKET_COLUMNS} FROM tickets {clause}"))?;
    let rows = statement.query_map(params_from_iter(args), |row| object(row, JSON_COLUMNS))?;
    rows.map(|row| from_object(row?)).collect()
}

/// A row as a JSON object keyed by column name, parsing the named JSON columns.
fn object(row: &Row<'_>, json: &[&str]) -> rusqlite::Result<Result<Map<String, Value>>> {
    let mut map = Map::new();
    for (index, name) in row.as_ref().column_names().into_iter().enumerate() {
        let value = match row.get_ref(index)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::from(n),
            ValueRef::Real(n) => Value::from(n),
            ValueRef::Text(text) | ValueRef::Blob(text) => {
                let text = String::from_utf8_lossy(text).into_owned();
                if json.contains(&name) {
                    match serde_json::from_str(&text) {
                        Ok(value) => value,
                        Err(error) => {
                            return Ok(Err(StoreError::Invalid(format!("{name}: {error}"))));
                        }
                    }
                } else {
                    Value::String(text)
                }
            }
        };
        map.insert(name.to_owned(), value);
    }
    Ok(Ok(map))
}

fn from_object<T: serde::de::DeserializeOwned>(map: Result<Map<String, Value>>) -> Result<T> {
    let map = map?;
    let id = map.get("id").or_else(|| map.get("ticket")).cloned();
    serde_json::from_value(Value::Object(map))
        .map_err(|error| StoreError::Invalid(format!("row {id:?}: {error}")))
}

/// A ticket as a JSON object keyed by column name.
pub(crate) fn to_object(ticket: &Ticket) -> Map<String, Value> {
    match serde_json::to_value(ticket) {
        Ok(Value::Object(map)) => map,
        _ => unreachable!("a ticket serializes to an object"),
    }
}

/// The SQL value stored for a column holding `value`.
pub(crate) fn sql_value(column: &str, value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        _ if JSON_COLUMNS.contains(&column) => SqlValue::Text(value.to_string()),
        Value::Bool(flag) => SqlValue::Integer(i64::from(*flag)),
        Value::Number(n) => n.as_i64().map_or_else(
            || SqlValue::Real(n.as_f64().unwrap_or(0.0)),
            SqlValue::Integer,
        ),
        Value::String(text) => SqlValue::Text(text.clone()),
        Value::Array(_) | Value::Object(_) => SqlValue::Text(value.to_string()),
    }
}

/// Gives each ticket that `batch` creates without a status the one it waits in: an epic, and any
/// ticket filed under a pending ancestor, wait for a person's release (`pending_approval`);
/// any other ticket is filed `open`, so the pool may be offered it at once. A parent created in
/// the same batch (`$<index>`) counts as pending when this pass made it so. A create that names
/// its status, as the hub's own filings and `ignite ticket create --release` do, is kept.
pub fn add_holds(conn: &Connection, batch: &mut BatchRequest) -> Result<()> {
    let mut held = HashSet::new();
    for (index, operation) in batch.ops.iter_mut().enumerate() {
        let Operation::Create { fields, parent, .. } = operation else {
            continue;
        };
        let parent_name = parent
            .clone()
            .or_else(|| fields.parent.clone().flatten().map(String::from));
        let pending = if let Some(status) = fields.status {
            status == Status::PendingApproval
        } else {
            let pending = fields.ticket_type == Some(TicketType::Epic)
                || match parent_name.as_deref() {
                    None => false,
                    Some(name) => match name.strip_prefix('$') {
                        Some(reference) => reference
                            .parse::<usize>()
                            .is_ok_and(|index| held.contains(&index)),
                        None => pending_line(conn, name)?,
                    },
                };
            fields.status = Some(if pending {
                Status::PendingApproval
            } else {
                Status::Open
            });
            pending
        };
        if pending {
            held.insert(index);
        }
    }
    Ok(())
}

/// Whether the ticket `name` names, or one of its ancestors, is pending approval.
fn pending_line(conn: &Connection, name: &str) -> Result<bool> {
    let mut next = resolve(conn, name)?.map(|id| id.to_string());
    let mut seen = HashSet::new();
    while let Some(id) = next {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(ticket) = load(conn, &id)? else {
            break;
        };
        if ticket.status == Status::PendingApproval {
            return Ok(true);
        }
        next = ticket.parent.map(|parent| parent.to_string());
    }
    Ok(false)
}

/// Adds to `batch` the release of every pending descendant of each pending ticket that one of
/// its updates releases (sets `open`), so that releasing an epic releases its tickets whichever
/// client sent the update: `ignite ticket update --release` or the dashboard's Release button.
/// A ticket created in the same batch (`$<index>`) has no descendants yet and is skipped.
///
/// A released epic with no allocation (the `workers` column), such as one imported from Enact,
/// also gets the default ([`shares::DEFAULT_EPIC_ALLOCATION`]) unless the update sets one. The pool offers a
/// ticket only when the nearest ancestor with an allocation has one above 0
/// (`ignite_hub`'s `dispatch_queue`), so without it a released epic inside a larger project
/// whose epics have no number either would never be worked.
pub fn add_releases(conn: &Connection, batch: &mut BatchRequest) -> Result<()> {
    let mut released = Vec::new();
    for operation in &mut batch.ops {
        let Operation::Update {
            id, set: Some(set), ..
        } = operation
        else {
            continue;
        };
        if set.status != Some(Status::Open) || id.starts_with('$') {
            continue;
        }
        let Some(id) = resolve(conn, id)? else {
            continue;
        };
        let Some(ticket) = load(conn, id.as_str())? else {
            continue;
        };
        if ticket.status != Status::PendingApproval {
            continue;
        }
        if ticket.ticket_type == TicketType::Epic
            && ticket.workers.is_none()
            && set.workers.is_none()
        {
            set.workers = Some(Some(shares::DEFAULT_EPIC_ALLOCATION));
        }
        released.push(id);
    }
    let mut added = HashSet::new();
    for id in released {
        let mut below: Vec<String> = descendants(conn, id.as_str())?.into_iter().collect();
        below.sort();
        for descendant in below {
            let pending =
                load(conn, &descendant)?.is_some_and(|t| t.status == Status::PendingApproval);
            if pending && added.insert(descendant.clone()) {
                batch.ops.push(Operation::Update {
                    id: descendant,
                    set: Some(TicketFields {
                        status: Some(Status::Open),
                        ..TicketFields::default()
                    }),
                    add_labels: Vec::new(),
                    remove_labels: Vec::new(),
                    set_meta: BTreeMap::new(),
                    unset_meta: Vec::new(),
                });
            }
        }
    }
    Ok(())
}

/// The IDs of every descendant of `ancestor`, closed or not.
fn descendants(conn: &Connection, ancestor: &str) -> Result<HashSet<String>> {
    let ids = collect_strings(
        conn,
        "WITH RECURSIVE below(id) AS ( \
           SELECT id FROM tickets WHERE parent = ?1 \
           UNION ALL SELECT tickets.id FROM below JOIN tickets ON tickets.parent = below.id) \
         SELECT id FROM below",
        &[ancestor],
    )?;
    Ok(ids.into_iter().collect())
}

/// Whether a ticket, or an ancestor whose status the query accepts, has every `label` and at
/// least one `label_any`, so that a label on an epic selects the tickets under it.
fn matches_labels(conn: &Connection, ticket: &Ticket, query: &TicketQuery) -> Result<bool> {
    let matches = |labels: &[String]| {
        query.label.iter().all(|l| labels.contains(l))
            && query.label_any.iter().any(|l| labels.contains(l))
    };
    if matches(&ticket.labels) {
        return Ok(true);
    }
    let mut current = ticket.parent.clone();
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(parent) = load(conn, id.as_str())? else {
            break;
        };
        if query.status.is_none_or(|s| s == parent.status) && matches(&parent.labels) {
            return Ok(true);
        }
        current = parent.parent;
    }
    Ok(false)
}

/// Tickets whose title holds the phrase, whose ID starts with it, or one of whose aliases does,
/// in order of priority and then newest first.
fn search_candidates(
    conn: &Connection,
    phrase: &str,
    status: Option<Status>,
) -> Result<Vec<Ticket>> {
    if phrase.trim().is_empty() {
        return Err(StoreError::Rejected("search query is required".to_owned()));
    }
    let query = phrase.to_lowercase();
    // IDs are stored in capitals with a leading letter (`T`, `E`, `M`, `A` or `B`), so a phrase is
    // compared with them in capitals, with and without each letter; aliases are stored in
    // lowercase.
    let upper = phrase.to_ascii_uppercase();
    let id_prefixes: Vec<String> = {
        let without_project = upper
            .split_once('-')
            .map_or(upper.as_str(), |(_, rest)| rest);
        let mut prefixes = vec![upper.clone()];
        for letter in ROOT_PREFIXES {
            prefixes.push(format!("{letter}{upper}"));
            if without_project != upper {
                prefixes.push(format!("{letter}{without_project}"));
            }
        }
        // Every ID starts with a letter, so a lone letter would match a whole kind.
        prefixes.retain(|prefix| prefix.len() > 1);
        prefixes
    };
    let alias_prefix = query
        .split_once('-')
        .map_or(query.as_str(), |(_, rest)| rest)
        .to_owned();
    let aliased: HashSet<String> = if alias_prefix.is_empty() {
        HashSet::new()
    } else {
        collect_strings(
            conn,
            "SELECT id FROM aliases WHERE instr(alias, ?1) = 1 OR instr(alias, ?2) = 1",
            &[&query, &alias_prefix],
        )?
        .into_iter()
        .collect()
    };
    let status_name = status.map(status_name);
    let filter = if status_name.is_some() {
        "WHERE status = ?1 "
    } else {
        ""
    };
    let args: Vec<&str> = status_name.into_iter().collect();
    Ok(select(
        conn,
        &format!("{filter}ORDER BY priority, created_at DESC, id"),
        &args,
    )?
    .into_iter()
    .filter(|t| {
        let id = t.id.as_str();
        t.title.to_lowercase().contains(&query)
            || id_prefixes
                .iter()
                .any(|prefix| id.starts_with(prefix.as_str()))
            || aliased.contains(id)
    })
    .collect())
}

fn archived_candidates(conn: &Connection, status: Option<Status>) -> Result<Vec<Ticket>> {
    match status {
        None => select(conn, "ORDER BY id", &[]),
        Some(status) => select(
            conn,
            "WHERE status = ?1 ORDER BY id",
            &[status_name(status)],
        ),
    }
}

/// The stored name of a status, such as `in_progress`.
pub(crate) fn status_name(status: Status) -> &'static str {
    match status {
        Status::Open => "open",
        Status::InProgress => "in_progress",
        Status::Blocked => "blocked",
        Status::Deferred => "deferred",
        Status::Closed => "closed",
        Status::PendingApproval => "pending_approval",
    }
}

pub(crate) fn collect_strings(conn: &Connection, sql: &str, args: &[&str]) -> Result<Vec<String>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(params_from_iter(args), |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn collect_ids(conn: &Connection, sql: &str, args: &[&str]) -> Result<Vec<TicketId>> {
    collect_strings(conn, sql, args)?
        .into_iter()
        .map(|id| {
            id.parse()
                .map_err(|e| StoreError::Invalid(format!("ticket ID: {e}")))
        })
        .collect()
}

/// Gives an epic filed without an allocation (the `workers` column) the default
/// ([`shares::DEFAULT_EPIC_ALLOCATION`]), so releasing it is enough for the pool to work it. An
/// explicit number, `0` included, is kept.
pub(crate) fn default_workers(fields: &TicketFields, changes: &mut Map<String, Value>) {
    if fields.ticket_type == Some(TicketType::Epic) && fields.workers.is_none() {
        changes.insert("workers".to_owned(), json!(shares::DEFAULT_EPIC_ALLOCATION));
    }
}

/// A new ticket with every field at its default, before the create's own fields apply.
pub(crate) fn blank(
    id: TicketId,
    parent: Option<TicketId>,
    title: String,
    status: Status,
    actor: &str,
    now: Timestamp,
    version: Seq,
) -> Ticket {
    Ticket {
        id,
        parent,
        next_child: 1,
        title,
        body: String::new(),
        acceptance: String::new(),
        design: String::new(),
        notes: String::new(),
        ticket_type: TicketType::Task,
        status,
        priority: 2,
        assignee: None,
        last: 0,
        labels: Vec::new(),
        metadata: BTreeMap::new(),
        defer_until: None,
        close_reason: None,
        created_at: now,
        created_by: actor.to_owned(),
        updated_at: now,
        closed_at: (status == Status::Closed).then_some(now),
        version,
        workers: None,
        max_workers: None,
        estimate_minutes: None,
        unfiled_minutes: None,
        cores: None,
        machine: None,
        session: None,
        tree: None,
        claimed_at: None,
        landed_at: None,
        landed: None,
        saving: None,
        needs_os: None,
        fallback: false,
        every_hours: None,
    }
}
