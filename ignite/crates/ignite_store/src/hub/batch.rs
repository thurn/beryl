//! `POST /batch`: preconditions and operations applied in one transaction that succeeds or fails
//! as a whole, ported from the ticket service.
//!
//! - The transaction starts `IMMEDIATE`, so two batches on two connections run one after the
//!   other: of two claims of one ticket, the second sees the first's assignee and conflicts.
//! - Every change to a ticket's fields takes the next position (`seq`) and makes it the ticket's
//!   `version`, with a history row of `{field: [old, new]}`. Comments and edges take a position
//!   too but leave the version alone, as in the ticket service.
//! - An operation's `id` may be `$<index>`, the ticket that the create at that index made.
//! - Closing a ticket closes each ancestor left with no live descendant, and marks the finding
//!   filed for any of them closed. An epic with an acceptance is not closed but gets a child
//!   that verifies it (`ancestors`).
//! - At the end, `meta.seq` moves to the last position used, and the feed gets one change per
//!   ticket touched, at that position.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{
    Connection, OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter,
};
use serde_json::{Map, Value, json};

use ignite_core::ids::{Seq, TicketId};
use ignite_core::protocol::tickets::{
    BatchConflict, BatchReply, BatchRequest, Dependency, FeedChange, Operation, OperationResult,
    Precondition, Status, Ticket, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;

use crate::hub::fix_outcomes::{self, LandingFix};
use crate::hub::{feed, findings, flaky_triage, fleet, placement_deferrals, tickets};
use crate::{Result, StoreError};

mod ancestors;
mod epic_dependencies;
mod fields;
mod moves;
pub(crate) mod roots;

use roots::Filer;

/// The outcome of a batch: applied, or refused at one precondition or operation.
#[derive(Debug)]
pub enum BatchOutcome {
    Applied(BatchReply),
    Conflict(Box<BatchConflict>),
}

/// How long a create's `request_key` makes a retry return the first create's ticket.
const REQUEST_KEY_SECONDS: f64 = 86_400.0;

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
    apply_as(conn, batch, actor, machine, now, Filer::Person)
}

/// [`apply`] for a batch that Ignite files on its own, such as a regression or a flaky check
/// found by upkeep. Justiciar roots get `J`, other roots get `A` ([`Filer`]), and children
/// inherit their parent's ID.
pub fn apply_automatic(
    conn: &mut Connection,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
) -> Result<BatchOutcome> {
    apply_as(conn, batch, actor, machine, now, Filer::Ignite)
}

fn apply_as(
    conn: &mut Connection,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
    filer: Filer,
) -> Result<BatchOutcome> {
    let mut transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let outcome = apply_in_as(&mut transaction, batch, actor, machine, now, filer, None)?;
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
    apply_in_as(transaction, batch, actor, machine, now, Filer::Person, None)
}

/// Applies the close batch of a landing and gives its primary ticket credit for the fix.
pub fn apply_landing_in(
    transaction: &mut Transaction,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
    landing: (&str, &LandingFix),
) -> Result<BatchOutcome> {
    apply_in_as(
        transaction,
        batch,
        actor,
        machine,
        now,
        Filer::Person,
        Some(landing),
    )
}

/// Applies an automatic filing inside the caller's transaction.
pub fn apply_automatic_in(
    transaction: &mut Transaction,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
) -> Result<BatchOutcome> {
    apply_in_as(transaction, batch, actor, machine, now, Filer::Ignite, None)
}

fn apply_in_as(
    transaction: &mut Transaction,
    batch: &BatchRequest,
    actor: &str,
    machine: &str,
    now: Timestamp,
    filer: Filer,
    landing: Option<(&str, &LandingFix)>,
) -> Result<BatchOutcome> {
    let mut savepoint = transaction.savepoint()?;
    let outcome = {
        let mut run = Run::new(&savepoint, actor, machine, now, filer)?;
        run.landing = landing;
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
    filer: Filer,
    landing: Option<(&'a str, &'a LandingFix)>,
    at: Value,
    start: Seq,
    seq: u64,
    /// The tickets made by creates, by the index of their operation.
    created: HashMap<usize, String>,
    /// Every ticket whose row, comments or edges changed, for the feed.
    changed: BTreeSet<String>,
}

impl<'a> Run<'a> {
    fn new(
        conn: &'a Connection,
        actor: &'a str,
        machine: &'a str,
        now: Timestamp,
        filer: Filer,
    ) -> Result<Self> {
        let start = feed::current(conn)?;
        Ok(Run {
            conn,
            actor,
            machine,
            now,
            filer,
            landing: None,
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
                let (closed, filed) = self.close_ancestors(ticket.id.as_str(), Some(reason))?;
                let ready = if *include_ready {
                    Some(tickets::ready(self.conn, self.now)?)
                } else {
                    None
                };
                Ok(OperationResult {
                    id: ticket.id.clone(),
                    closed,
                    filed,
                    ready,
                })
            }
            Operation::Reopen { .. } => self.reopen(&ticket, None),
            Operation::Defer { until, .. } => self.reopen(&ticket, Some(until)),
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
            Operation::DepAdd {
                on, kind, external, ..
            } => {
                self.dependency(ticket.id.as_str(), on, *kind, *external)?;
                Ok(plain(ticket.id.clone()))
            }
            Operation::DepRemove { on, kind, .. } => {
                self.remove_dependency(ticket.id.as_str(), on, *kind)?;
                Ok(plain(ticket.id.clone()))
            }
        }
    }

    fn reopen(
        &mut self,
        ticket: &Ticket,
        defer_until: Option<&Timestamp>,
    ) -> Step<OperationResult> {
        let mut changes = Self::opening();
        if let Some(until) = defer_until {
            changes.insert("defer_until".to_owned(), json!(until));
        }
        self.save(ticket, changes)?;
        if ticket.status == Status::Closed {
            self.wait_verify_children(ticket.id.as_str())?;
        }
        Ok(plain(ticket.id.clone()))
    }

    /// Assigns `ticket` to the actor unless someone holds it or has a live reservation.
    /// A claim by a new assignee, or a re-claim of a ticket that is not in progress (after a
    /// release or a close), records when the work started. A repeated claim of a ticket the actor
    /// already holds in progress keeps its start time.
    fn claim(&mut self, index: usize, ticket: &Ticket) -> Step<()> {
        let reserved = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM reservations WHERE ticket = ?1 AND expires_at > ?2)",
            params![ticket.id.as_str(), self.now.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        let blocked_triage = flaky_triage::is_batch(ticket)
            && ticket.assignee.as_deref() != Some(self.actor)
            && (ticket.status == Status::PendingApproval
                || flaky_triage::slot_taken(self.conn, self.now)?);
        if reserved || blocked_triage || ticket.assignee.as_deref().is_some_and(|a| a != self.actor)
        {
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
        let restarted = ticket.status != Status::InProgress;
        if ticket.assignee.as_deref() != Some(self.actor) || restarted {
            changes.insert("claimed_at".to_owned(), self.at.clone());
        }
        if ticket.assignee.as_deref() != Some(self.actor)
            && self.machine.parse::<ignite_core::ids::MachineId>().is_ok()
        {
            changes.insert("machine".to_owned(), json!(self.machine));
        }
        self.save(ticket, changes)?;
        if ticket.status == Status::Closed {
            self.wait_verify_children(ticket.id.as_str())?;
        }
        Ok(())
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
        let mut changes = Self::field_changes(fields, None, None)?;
        Self::allocation_changes(fields, fields.ticket_type.unwrap_or_default(), &mut changes);
        tickets::default_workers(fields, &mut changes);
        let parent_name = match parent {
            Some(parent) => Some(parent.to_owned()),
            None => fields.parent.clone().flatten().map(String::from),
        };
        let parent = parent_name.map(|name| self.ticket(&name)).transpose()?;
        let meta = fields.ticket_type == Some(TicketType::Meta);
        if meta && parent.is_some() {
            return rejected("A meta-ticket cannot have a parent");
        }
        if meta && fields.every_hours.flatten().is_none() {
            return rejected("A meta-ticket needs a schedule (every_hours)");
        }
        let id = match &parent {
            Some(parent) => self.take_child_id(parent)?,
            None => roots::allocate(self.conn, self.filer.letter(fields))?,
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
        if ticket.parent.is_some() && ticket.status != Status::Closed {
            self.wait_verify_children(&id)?;
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
        let mut changes = Self::field_changes(fields, labels, ticket.needs_os)?;
        let was_meta = ticket.ticket_type == TicketType::Meta;
        if fields
            .ticket_type
            .is_some_and(|t| (t == TicketType::Meta) != was_meta)
        {
            // A meta-ticket's ID starts with `M` and an ordinary ticket's with `T`, so neither
            // can become the other.
            return rejected("A ticket cannot become a meta-ticket or stop being one");
        }
        if was_meta && matches!(fields.parent, Some(Some(_))) {
            return rejected("A meta-ticket cannot have a parent");
        }
        if was_meta && matches!(fields.every_hours, Some(None)) {
            return rejected("A meta-ticket needs a schedule (every_hours)");
        }
        let changes_epic = fields.ticket_type.is_some_and(|kind| {
            (kind == TicketType::Epic) != (ticket.ticket_type == TicketType::Epic)
        });
        let original_id = &ticket.id;
        // A new parent, or `"parent": null` to detach, gives the ticket a new ID.
        let moved;
        let ticket = match &fields.parent {
            Some(parent) => {
                moved = self.move_ticket(ticket, parent.as_ref().map(TicketId::as_str))?;
                &moved
            }
            None => ticket,
        };
        if let Some(metadata) = metadata {
            changes.insert("metadata".to_owned(), json!(metadata));
        }
        let releasing = Self::update_allocation(ticket, fields, &mut changes);
        let releasing_epic = releasing && ticket.ticket_type == TicketType::Epic;
        let was_closed = ticket.status == Status::Closed;
        let closing = !was_closed && fields.status == Some(Status::Closed);
        let reopening = was_closed && fields.status.is_some_and(|s| s != Status::Closed);
        let reparented = &ticket.id != original_id;
        match fields.status {
            Some(Status::Closed) if closing => {
                changes.insert("closed_at".to_owned(), self.at.clone());
            }
            Some(status) if was_closed && status != Status::Closed => {
                changes.insert("closed_at".to_owned(), Value::Null);
                changes.insert("close_reason".to_owned(), Value::Null);
            }
            _ => {}
        }
        self.save(ticket, changes)?;
        if changes_epic || reparented {
            self.check_epic_dependencies(ticket.id.as_str(), false)?;
        } else if releasing_epic {
            self.check_epic_dependencies(ticket.id.as_str(), true)?;
        }
        if releasing {
            self.release_descendants(&ticket.id)?;
        }
        if reopening || (reparented && fields.status.unwrap_or(ticket.status) != Status::Closed) {
            self.wait_verify_children(ticket.id.as_str())?;
        }
        let (closed, filed) = if closing {
            self.close_ancestors(ticket.id.as_str(), None)?
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(OperationResult {
            id: ticket.id.clone(),
            closed,
            filed,
            ready: None,
        })
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
        // A dispatch wait belongs to one period when the ticket was open and unassigned.
        if patch.contains_key("status") || patch.contains_key("assignee") {
            placement_deferrals::clear(self.conn, &before.id)?;
            fleet::release_reservation(self.conn, before.id.as_str())?;
        }
        let change: Map<String, Value> = patch
            .iter()
            .map(|(k, v)| (k.clone(), json!([old.get(k).unwrap_or(&Value::Null), v])))
            .collect();
        self.history(before.id.as_str(), &change)?;
        self.changed.insert(before.id.to_string());
        if patch.get("status") == Some(&json!(Status::Closed)) {
            // Whoever closes the ticket, an improvement's finding for it is then closed too.
            let id = before.id.as_str();
            findings::close_ticket(self.conn, id, self.now)?;
            let landing = self
                .landing
                .filter(|(ticket, _)| *ticket == id)
                .map(|(_, fix)| fix);
            fix_outcomes::record_close(self.conn, id, self.seq as i64, self.now, landing)?;
        }
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
        filed: Vec::new(),
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
