//! `POST /batch`: preconditions and operations applied in one transaction that succeeds or fails
//! as a whole, ported from the ticket service.
//!
//! - The transaction starts `IMMEDIATE`, so two batches on two connections run one after the
//!   other: of two claims of one ticket, the second sees the first's assignee and conflicts.
//! - Every change to a ticket's fields takes the next position (`seq`) and makes it the ticket's
//!   `version`, with a history row of `{field: [old, new]}`. Comments and edges take a position
//!   too but leave the version alone, as in the ticket service.
//! - An operation's `id` may be `$<index>`, the ticket that the create at that index made.
//! - Closing a ticket closes each ancestor left with no live descendant.
//! - At the end, `meta.seq` moves to the last position used, and the feed gets one change per
//!   ticket touched, at that position.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{
    Connection, OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter,
};
use serde_json::{Map, Value, json};

use ignite_core::ids::{Seq, TicketId};
use ignite_core::protocol::tickets::{
    BatchConflict, BatchReply, BatchRequest, Dependency, EdgeKind, FeedChange, Operation,
    OperationResult, Precondition, Status, Ticket, TicketFields,
};
use ignite_core::time::Timestamp;

use crate::hub::{cycles, edges, feed, tickets};
use crate::{Result, StoreError};

/// The outcome of a batch: applied, or refused at one precondition or operation.
#[derive(Debug)]
pub enum BatchOutcome {
    Applied(BatchReply),
    Conflict(Box<BatchConflict>),
}

/// How long a create's `request_key` makes a retry return the first create's ticket.
const REQUEST_KEY_SECONDS: f64 = 86_400.0;

/// The letters of a new root ID. The first is never a digit, so a root never looks like a
/// number, and `i`, `l`, `o` and `u` are left out as easy to misread.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Applies `batch` for `actor` (the session writing) on `machine`. A failed precondition, or a
/// claim of a ticket someone else holds, returns a conflict naming its index and the ticket as
/// it stands; an unknown ticket or a malformed operation returns an error. Either way nothing
/// is written.
pub fn apply(
    conn: &mut Connection,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
) -> Result<BatchOutcome> {
    let mut transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let outcome = apply_in(&mut transaction, batch, actor, machine, now)?;
    if matches!(outcome, BatchOutcome::Applied(_)) {
        transaction.commit()?;
    }
    Ok(outcome)
}

/// Applies `batch` inside a transaction the caller started, so that the caller can do more
/// reads and writes in the same transaction. The batch runs in a savepoint: a conflict or an
/// error undoes the whole batch and leaves the caller's earlier changes in place. The caller
/// commits, or drops the transaction to undo everything.
pub fn apply_in(
    transaction: &mut Transaction,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
) -> Result<BatchOutcome> {
    let mut savepoint = transaction.savepoint()?;
    let outcome = {
        let mut run = Run::new(&savepoint, actor, machine, now)?;
        run.batch(batch)
            .and_then(|results| Ok((run.finish()?, results)))
    };
    match outcome {
        Ok((seq, results)) => {
            savepoint.commit()?;
            Ok(BatchOutcome::Applied(BatchReply { seq, results }))
        }
        Err(Stop::Conflict { failed, id }) => {
            savepoint.rollback()?;
            savepoint.finish()?;
            let current = match id {
                Some(id) => tickets::load(transaction, &id)?,
                None => None,
            };
            Ok(BatchOutcome::Conflict(Box::new(BatchConflict {
                failed,
                current,
            })))
        }
        Err(Stop::Store(error)) => Err(error),
    }
}

/// Why a batch stopped early.
enum Stop {
    /// The precondition or operation at `failed` did not hold; `id` is the ticket to report.
    Conflict {
        failed: usize,
        id: Option<String>,
    },
    Store(StoreError),
}

impl From<StoreError> for Stop {
    fn from(error: StoreError) -> Self {
        Stop::Store(error)
    }
}

impl From<rusqlite::Error> for Stop {
    fn from(error: rusqlite::Error) -> Self {
        Stop::Store(StoreError::Sqlite(error))
    }
}

type Step<T> = std::result::Result<T, Stop>;

fn rejected<T>(message: impl Into<String>) -> Step<T> {
    Err(Stop::Store(StoreError::Rejected(message.into())))
}

/// One batch in progress inside its transaction.
struct Run<'a> {
    conn: &'a Connection,
    actor: &'a str,
    machine: &'a str,
    now: Timestamp,
    at: Value,
    start: Seq,
    seq: u64,
    /// The tickets made by creates, by the index of their operation.
    created: HashMap<usize, String>,
    /// Every ticket whose row, comments or edges changed, for the feed.
    changed: BTreeSet<String>,
}

impl<'a> Run<'a> {
    fn new(conn: &'a Connection, actor: &'a str, machine: &'a str, now: Timestamp) -> Result<Self> {
        let start = feed::current(conn)?;
        Ok(Run {
            conn,
            actor,
            machine,
            now,
            at: json!(now),
            start,
            seq: start.0,
            created: HashMap::new(),
            changed: BTreeSet::new(),
        })
    }

    fn batch(&mut self, batch: &BatchRequest) -> Step<Vec<OperationResult>> {
        for (index, condition) in batch.preconditions.iter().enumerate() {
            self.check(index, condition)?;
        }
        let mut results = Vec::with_capacity(batch.ops.len());
        for (index, operation) in batch.ops.iter().enumerate() {
            results.push(self.operation(index, operation)?);
        }
        Ok(results)
    }

    /// Moves `meta.seq` and writes the feed. Returns the batch's position.
    fn finish(&mut self) -> Step<Seq> {
        let seq = Seq(self.seq);
        if seq != self.start {
            feed::set_current(self.conn, seq)?;
        }
        for id in &self.changed {
            feed::record(
                self.conn,
                &FeedChange {
                    seq,
                    kind: "ticket".to_owned(),
                    ticket: Some(parse_id(id)?),
                    name: None,
                },
            )?;
        }
        Ok(seq)
    }

    fn check(&mut self, index: usize, condition: &Precondition) -> Step<()> {
        let (id, passed): (&TicketId, fn(&Ticket, &Precondition) -> bool) = match condition {
            Precondition::Seq { seq } => {
                if seq.0 != self.seq {
                    return Err(Stop::Conflict {
                        failed: index,
                        id: None,
                    });
                }
                return Ok(());
            }
            Precondition::Version { id, .. } => (
                id,
                |t, c| matches!(c, Precondition::Version { version, .. } if t.version == *version),
            ),
            Precondition::Assignee { id, .. } => (
                id,
                |t, c| matches!(c, Precondition::Assignee { assignee, .. } if t.assignee == *assignee),
            ),
            Precondition::Status { id, .. } => (
                id,
                |t, c| matches!(c, Precondition::Status { status, .. } if t.status == *status),
            ),
        };
        let id = self.resolve(id.as_str())?;
        let ticket = tickets::load(self.conn, &id)?;
        if ticket.as_ref().is_some_and(|t| passed(t, condition)) {
            return Ok(());
        }
        Err(Stop::Conflict {
            failed: index,
            id: ticket.map(|t| t.id.to_string()),
        })
    }

    fn operation(&mut self, index: usize, operation: &Operation) -> Step<OperationResult> {
        let ticket = match operation {
            Operation::Create {
                fields,
                parent,
                deps,
                request_key,
            } => {
                let id = self.create(fields, parent.as_deref(), deps, request_key.as_deref())?;
                self.created.insert(index, id.clone());
                return Ok(plain(parse_id(&id)?));
            }
            Operation::Update { id, .. }
            | Operation::Claim { id }
            | Operation::Close { id, .. }
            | Operation::Reopen { id }
            | Operation::Defer { id, .. }
            | Operation::Comment { id, .. }
            | Operation::DepAdd { id, .. }
            | Operation::DepRemove { id, .. } => self.ticket(id)?,
        };
        match operation {
            Operation::Create { .. } => unreachable!("handled above"),
            Operation::Update {
                set,
                add_labels,
                remove_labels,
                set_meta,
                unset_meta,
                ..
            } => {
                let (labels, metadata) = edits(
                    &ticket,
                    set.as_ref(),
                    (add_labels, remove_labels),
                    (set_meta, unset_meta),
                );
                let fields = set.clone().unwrap_or_default();
                self.update(&ticket, &fields, labels, metadata)
            }
            Operation::Claim { .. } => {
                self.claim(index, &ticket)?;
                Ok(plain(ticket.id.clone()))
            }
            Operation::Close {
                reason,
                include_ready,
                ..
            } => {
                if ticket.status != Status::Closed {
                    self.save(&ticket, self.closing(Some(reason)))?;
                }
                let closed = self.close_ancestors(ticket.id.as_str(), Some(reason))?;
                let ready = if *include_ready {
                    Some(tickets::ready(self.conn, self.now)?)
                } else {
                    None
                };
                Ok(OperationResult {
                    id: ticket.id.clone(),
                    closed,
                    ready,
                })
            }
            Operation::Reopen { .. } => {
                self.save(&ticket, Self::opening())?;
                Ok(plain(ticket.id.clone()))
            }
            Operation::Defer { until, .. } => {
                let mut changes = Self::opening();
                changes.insert("defer_until".to_owned(), json!(until));
                self.save(&ticket, changes)?;
                Ok(plain(ticket.id.clone()))
            }
            Operation::Comment { text, .. } => {
                self.conn.execute(
                    "INSERT INTO comments (ticket, n, author, at, text) \
                     SELECT ?1, COALESCE(MAX(n), 0) + 1, ?2, ?3, ?4 FROM comments WHERE ticket = ?1",
                    params![ticket.id.as_str(), self.actor, self.now.to_string(), text],
                )?;
                self.seq += 1;
                self.changed.insert(ticket.id.to_string());
                Ok(plain(ticket.id.clone()))
            }
            Operation::DepAdd { on, kind, .. } => {
                self.dependency(ticket.id.as_str(), on, *kind, false)?;
                Ok(plain(ticket.id.clone()))
            }
            Operation::DepRemove { on, kind, .. } => {
                self.dependency(ticket.id.as_str(), on, *kind, true)?;
                Ok(plain(ticket.id.clone()))
            }
        }
    }

    /// Assigns `ticket` to the actor and starts it, unless someone else holds it. The first
    /// claim records when and on which machine the work started.
    fn claim(&mut self, index: usize, ticket: &Ticket) -> Step<()> {
        if ticket.assignee.as_deref().is_some_and(|a| a != self.actor) {
            return Err(Stop::Conflict {
                failed: index,
                id: Some(ticket.id.to_string()),
            });
        }
        let mut changes = object(&[
            ("assignee", json!(self.actor)),
            ("status", json!(Status::InProgress)),
            ("closed_at", Value::Null),
            ("close_reason", Value::Null),
        ]);
        if ticket.assignee.as_deref() != Some(self.actor) {
            changes.insert("claimed_at".to_owned(), self.at.clone());
            if self.machine.parse::<ignite_core::ids::MachineId>().is_ok() {
                changes.insert("machine".to_owned(), json!(self.machine));
            }
        }
        self.save(ticket, changes)
    }

    /// The ID a name in an operation refers to: `$<index>` for a ticket this batch created, or
    /// any name [`tickets::resolve`] accepts.
    fn resolve(&self, name: &str) -> Step<String> {
        let Some(reference) = name.strip_prefix('$') else {
            return Ok(tickets::candidate_id(self.conn, name)?);
        };
        match reference
            .parse()
            .ok()
            .and_then(|index: usize| self.created.get(&index))
        {
            Some(id) => Ok(id.clone()),
            None => rejected(format!("Unknown batch reference {name}")),
        }
    }

    fn ticket(&self, name: &str) -> Step<Ticket> {
        let id = self.resolve(name)?;
        match tickets::load(self.conn, &id)? {
            Some(ticket) => Ok(ticket),
            None => Err(Stop::Store(StoreError::NotFound(format!(
                "Unknown ticket {id}"
            )))),
        }
    }

    fn create(
        &mut self,
        fields: &TicketFields,
        parent: Option<&str>,
        deps: &[Dependency],
        request_key: Option<&str>,
    ) -> Step<String> {
        let key = request_key.map(|key| format!("create:{key}"));
        if let Some(key) = &key
            && let Some(id) = self.created_for(key)?
        {
            return Ok(id);
        }
        let Some(title) = fields.title.clone() else {
            return rejected("A ticket needs a title");
        };
        let mut changes = Self::field_changes(fields)?;
        let parent_name = match parent {
            Some(parent) => Some(parent.to_owned()),
            None => fields.parent.clone().flatten().map(String::from),
        };
        let parent = parent_name.map(|name| self.ticket(&name)).transpose()?;
        let id = match &parent {
            Some(parent) => {
                let next_child = parent.next_child + 1;
                self.conn.execute(
                    "UPDATE tickets SET next_child = ?1 WHERE id = ?2",
                    params![next_child, parent.id.as_str()],
                )?;
                self.changed.insert(parent.id.to_string());
                format!("{}.{}", parent.id, parent.next_child)
            }
            None => self.allocate_root()?,
        };
        self.seq += 1;
        let ticket = tickets::blank(
            parse_id(&id)?,
            parent.map(|p| p.id),
            title,
            fields.status.unwrap_or_default(),
            self.actor,
            self.now,
            Seq(self.seq),
        );
        let mut row = tickets::to_object(&ticket);
        changes.remove("parent");
        row.extend(changes);
        let columns: Vec<&String> = row.keys().collect();
        self.conn.execute(
            &format!(
                "INSERT INTO tickets ({}) VALUES ({})",
                columns
                    .iter()
                    .map(|c| quoted(c))
                    .collect::<Vec<_>>()
                    .join(", "),
                vec!["?"; columns.len()].join(", ")
            ),
            params_from_iter(row.iter().map(|(k, v)| tickets::sql_value(k, v))),
        )?;
        let change: Map<String, Value> = row
            .iter()
            .map(|(k, v)| (k.clone(), json!([Value::Null, v])))
            .collect();
        self.history(&id, &change)?;
        self.changed.insert(id.clone());
        for dep in deps {
            self.dependency(&id, &dep.on, dep.kind, false)?;
        }
        if let Some(key) = key {
            self.conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, json!({"id": id, "at": self.at}).to_string()],
            )?;
        }
        Ok(id)
    }

    /// The ticket an earlier create with this request key made, if it was less than a day ago.
    fn created_for(&self, key: &str) -> Step<Option<String>> {
        let saved: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        let Some(saved) = saved.and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
            return Ok(None);
        };
        let at = saved["at"]
            .as_str()
            .and_then(|at| Timestamp::parse(at).ok());
        Ok(match (saved["id"].as_str(), at) {
            (Some(id), Some(at)) if at.plus_seconds(REQUEST_KEY_SECONDS) > self.now => {
                Some(id.to_owned())
            }
            _ => None,
        })
    }

    fn update(
        &mut self,
        ticket: &Ticket,
        fields: &TicketFields,
        labels: Option<Vec<String>>,
        metadata: Option<BTreeMap<String, Value>>,
    ) -> Step<OperationResult> {
        let mut changes = Self::field_changes(fields)?;
        match &fields.parent {
            Some(Some(parent)) => {
                let parent = self.ticket(parent.as_str())?.id.to_string();
                if parent == ticket.id.as_str()
                    || self.ancestors(&parent)?.contains(&ticket.id.to_string())
                {
                    return rejected("A ticket cannot become its own ancestor");
                }
                changes.insert("parent".to_owned(), json!(parent));
            }
            // `"parent": null` detaches the ticket from its parent.
            Some(None) => {
                changes.insert("parent".to_owned(), Value::Null);
            }
            None => {}
        }
        if let Some(labels) = labels {
            changes.insert("labels".to_owned(), json!(labels));
        }
        if let Some(metadata) = metadata {
            changes.insert("metadata".to_owned(), json!(metadata));
        }
        let was_closed = ticket.status == Status::Closed;
        match fields.status {
            Some(Status::Closed) if !was_closed => {
                changes.insert("closed_at".to_owned(), self.at.clone());
                self.save(ticket, changes)?;
                let closed = self.close_ancestors(ticket.id.as_str(), None)?;
                return Ok(OperationResult {
                    id: ticket.id.clone(),
                    closed,
                    ready: None,
                });
            }
            Some(status) if was_closed && status != Status::Closed => {
                changes.insert("closed_at".to_owned(), Value::Null);
                changes.insert("close_reason".to_owned(), Value::Null);
            }
            _ => {}
        }
        self.save(ticket, changes)?;
        Ok(OperationResult {
            id: ticket.id.clone(),
            closed: Vec::new(),
            ready: None,
        })
    }

    /// The set fields of an update or create as column changes, checked as the ticket service
    /// checked them. `parent` is left for the caller, which must resolve it.
    fn field_changes(fields: &TicketFields) -> Step<Map<String, Value>> {
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
        let Ok(Value::Object(mut changes)) = serde_json::to_value(fields) else {
            unreachable!("ticket fields serialize to an object")
        };
        changes.remove("parent");
        if let Some(labels) = &fields.labels {
            changes.insert("labels".to_owned(), json!(dedupe(labels.clone())));
        }
        Ok(changes)
    }

    fn closing(&self, reason: Option<&str>) -> Map<String, Value> {
        object(&[
            ("status", json!(Status::Closed)),
            ("closed_at", self.at.clone()),
            ("close_reason", json!(reason)),
        ])
    }

    fn opening() -> Map<String, Value> {
        object(&[
            ("status", json!(Status::Open)),
            ("closed_at", Value::Null),
            ("close_reason", Value::Null),
        ])
    }

    /// Writes the fields of `changes` that differ from `before`, with a new version and a
    /// history row. Writes nothing when nothing differs.
    fn save(&mut self, before: &Ticket, changes: Map<String, Value>) -> Step<()> {
        let old = tickets::to_object(before);
        let mut patch: Map<String, Value> = changes
            .into_iter()
            .filter(|(key, value)| old.get(key).unwrap_or(&Value::Null) != value)
            .collect();
        if patch.is_empty() {
            return Ok(());
        }
        self.seq += 1;
        patch.insert("updated_at".to_owned(), self.at.clone());
        patch.insert("version".to_owned(), json!(self.seq));
        let assignments: Vec<String> = patch.keys().map(|k| format!("{} = ?", quoted(k))).collect();
        let mut values: Vec<_> = patch
            .iter()
            .map(|(k, v)| tickets::sql_value(k, v))
            .collect();
        values.push(rusqlite::types::Value::Text(before.id.to_string()));
        self.conn.execute(
            &format!("UPDATE tickets SET {} WHERE id = ?", assignments.join(", ")),
            params_from_iter(values),
        )?;
        let change: Map<String, Value> = patch
            .iter()
            .map(|(k, v)| (k.clone(), json!([old.get(k).unwrap_or(&Value::Null), v])))
            .collect();
        self.history(before.id.as_str(), &change)?;
        self.changed.insert(before.id.to_string());
        Ok(())
    }

    fn history(&self, id: &str, change: &Map<String, Value>) -> Step<()> {
        self.conn.execute(
            "INSERT INTO history (ticket, seq, actor, at, change) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                self.seq as i64,
                self.actor,
                self.now.to_string(),
                Value::Object(change.clone()).to_string()
            ],
        )?;
        Ok(())
    }

    /// Closes each ancestor of `id`, nearest first, until one that still has a live
    /// descendant. Returns the ancestors it closed.
    fn close_ancestors(&mut self, id: &str, reason: Option<&str>) -> Step<Vec<TicketId>> {
        let mut closed = Vec::new();
        for parent in self.ancestors(id)? {
            let live: Option<i64> = self
                .conn
                .query_row(
                    "WITH RECURSIVE below(id) AS ( \
                       SELECT id FROM tickets WHERE parent = ?1 \
                       UNION ALL SELECT tickets.id FROM below JOIN tickets ON tickets.parent = below.id) \
                     SELECT 1 FROM below JOIN tickets USING (id) WHERE status != 'closed' LIMIT 1",
                    [&parent],
                    |row| row.get(0),
                )
                .optional()?;
            if live.is_some() {
                break;
            }
            if let Some(ticket) = tickets::load(self.conn, &parent)?
                && ticket.status != Status::Closed
            {
                self.save(&ticket, self.closing(reason))?;
                closed.push(ticket.id);
            }
        }
        Ok(closed)
    }

    /// The ancestors of `id`, nearest first.
    fn ancestors(&self, id: &str) -> Step<Vec<String>> {
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

    /// Adds or removes `id` depends on `on`. Adding an edge that exists with another kind
    /// changes its kind; removing with a kind removes only an edge of that kind.
    fn dependency(&mut self, id: &str, on: &str, kind: Option<EdgeKind>, remove: bool) -> Step<()> {
        let dst = self.ticket(on)?.id.to_string();
        let existing = edges::kind(self.conn, id, &dst)?;
        if remove {
            if existing.is_none() || kind.is_some_and(|k| Some(k) != existing) {
                return Ok(());
            }
            edges::remove(self.conn, id, &dst)?;
        } else {
            let kind = kind.unwrap_or_default();
            if existing == Some(kind) {
                return Ok(());
            }
            if id == dst {
                return rejected(format!("{id} cannot depend on itself"));
            }
            if !matches!(kind, EdgeKind::Related | EdgeKind::DiscoveredFrom) {
                cycles::check(self.conn, id, &dst)?;
            }
            edges::add(self.conn, id, &dst, kind)?;
        }
        self.seq += 1;
        self.changed.insert(id.to_owned());
        self.changed.insert(dst);
        Ok(())
    }

    /// A new root ID: a letter and three letters or digits, used by no ticket or alias.
    fn allocate_root(&self) -> Step<String> {
        loop {
            let mut bytes = [0_u8; 4];
            getrandom::getrandom(&mut bytes)
                .map_err(|e| StoreError::Invalid(format!("no random numbers: {e}")))?;
            // Rejection sampling keeps all 22 letters equally likely as the first character.
            if bytes[0] >= 242 {
                continue;
            }
            let mut id = String::with_capacity(4);
            id.push(char::from(ALPHABET[10 + usize::from(bytes[0]) % 22]));
            for byte in &bytes[1..] {
                id.push(char::from(ALPHABET[usize::from(*byte) % 32]));
            }
            let taken: Option<i64> = self
                .conn
                .query_row(
                    "SELECT 1 FROM tickets WHERE id = ?1 UNION ALL \
                     SELECT 1 FROM aliases WHERE alias = ?1 OR alias LIKE '%-' || ?1 LIMIT 1",
                    [&id],
                    |row| row.get(0),
                )
                .optional()?;
            if taken.is_none() {
                return Ok(id);
            }
        }
    }
}

/// The labels and metadata an update leaves, when it adds or removes any: its `set` value or
/// the ticket's, with labels added and removed and metadata keys set and unset.
#[allow(clippy::type_complexity)]
fn edits(
    ticket: &Ticket,
    set: Option<&TicketFields>,
    (add_labels, remove_labels): (&[String], &[String]),
    (set_meta, unset_meta): (&BTreeMap<String, Value>, &[String]),
) -> (Option<Vec<String>>, Option<BTreeMap<String, Value>>) {
    let labels = (!add_labels.is_empty() || !remove_labels.is_empty()).then(|| {
        let base = set.and_then(|s| s.labels.clone());
        let mut next = base.unwrap_or_else(|| ticket.labels.clone());
        next.extend(add_labels.iter().cloned());
        let mut next = dedupe(next);
        next.retain(|label| !remove_labels.contains(label));
        next
    });
    let metadata = (!set_meta.is_empty() || !unset_meta.is_empty()).then(|| {
        let base = set.and_then(|s| s.metadata.clone());
        let mut next = base.unwrap_or_else(|| ticket.metadata.clone());
        next.extend(set_meta.clone());
        for key in unset_meta {
            next.remove(key);
        }
        next
    });
    (labels, metadata)
}

/// The result of an operation that closed nothing and asked for no ready list.
fn plain(id: TicketId) -> OperationResult {
    OperationResult {
        id,
        closed: Vec::new(),
        ready: None,
    }
}

fn object(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

/// Removes repeated labels, keeping the first of each.
fn dedupe(labels: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    labels
        .into_iter()
        .filter(|l| seen.insert(l.clone()))
        .collect()
}

/// A column name quoted for SQL, since `type` is a keyword.
fn quoted(column: &str) -> String {
    format!("\"{column}\"")
}

fn parse_id(id: &str) -> Step<TicketId> {
    id.parse()
        .map_err(|e| Stop::Store(StoreError::Invalid(format!("ticket ID: {e}"))))
}
