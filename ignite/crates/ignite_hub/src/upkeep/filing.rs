//! Filing a ticket the hub writes by itself, matched against earlier ones by signature.
//!
//! - An open ticket with the same signature gets a comment instead of a second ticket.
//!   Exploration reports share a small set of tickets while they wait for approval.
//! - Each report carries an `evidence` string that names the sighting, such as a commit or an
//!   input hash. A report whose evidence the ticket already holds changes nothing unless it
//!   names a tip the ticket was not known to block.
//! - A fault that returns within [`REOPEN_SECONDS`] of upkeep closing its ticket because the
//!   fault had cleared ([`super::clearing`]) reopens that ticket in the status it had. A fault
//!   that returns after a person closed its ticket, or later, gets a new ticket that names the
//!   old one.
//!
//! Each ticket also records what [`super::clearing`] needs to tell when its fault has cleared:
//! the parts of its signature ([`FAULT_KEY`]), who filed its sightings ([`FILERS_KEY`]), when the
//! newest sighting came ([`SIGHTED_KEY`]) and, for a fault that holds checks back, which checks
//! ([`CHECKS_KEY`]).
//!
//! Reports filed by a machine pin the new ticket to that machine unless the filing includes
//! its own machine pin. Tickets that upkeep files on its own can go to any machine.
//!
//! A ticket starts at `pending_approval` unless its filing marks it as [`Filing::released`].
//! A later released sighting releases a ticket still waiting for approval.
//!
//! The lookup and write are fenced by the feed position. If another write changes the feed
//! between them, the lookup is retried.

use std::collections::BTreeMap;

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use serde_json::Value;

use ignite_core::config::Os;
use ignite_core::pins::{from_labels, machine_label};

use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, Operation, Precondition, Status, TicketFields,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{feed, flaky_triage, fleet, tickets};
use ignite_store::{Result, StoreError};

use super::platform_wait::{PlatformWait, WAIT_SCOPE_KEY};
use super::signature::Signature;

/// The ticket metadata key that holds a signature's key.
pub const SIGNATURE_KEY: &str = "ignite_signature";

/// The ticket metadata key that holds the evidence of each sighting already recorded.
pub const SIGHTINGS_KEY: &str = "ignite_sightings";

/// The ticket metadata key that holds the signature's subject, category and platform.
pub const FAULT_KEY: &str = "ignite_fault";

/// The ticket metadata key that holds every actor that filed a sighting on the ticket.
pub const FILERS_KEY: &str = "ignite_filers";

/// The ticket metadata key that holds when the newest sighting was recorded.
pub const SIGHTED_KEY: &str = "ignite_sighted_at";

/// The ticket metadata key that holds the checks a fault keeps from a result. A filing sets it
/// in its fields' metadata; each later sighting adds its checks to the ticket's list.
pub const CHECKS_KEY: &str = "ignite_checks";

/// The tips a report says this repair holds back. Dispatch ignores tips that have finished.
pub(crate) const BLOCKED_TIPS_KEY: &str = "ignite_blocked_tips";

/// The ticket metadata key that holds when upkeep closed the ticket because its fault cleared.
pub const AUTO_CLOSED_KEY: &str = "ignite_auto_closed";

/// The ticket metadata key that holds the status a ticket had when upkeep closed it.
pub const STATUS_BEFORE_KEY: &str = "ignite_status_before";

/// The ticket metadata key that counts how often the fault returned after an automatic close.
pub const RETURNS_KEY: &str = "ignite_returns";

/// The ticket metadata key that holds since when upkeep has seen the fault clear.
pub const CLEARED_SINCE_KEY: &str = "ignite_cleared_since";

/// How long after an automatic close a returning fault reopens the ticket instead of filing a
/// new one: a week, so a fault that comes and goes keeps one ticket and its history.
pub const REOPEN_SECONDS: f64 = 7.0 * 86_400.0;

/// The label on every ticket the hub files by itself. A ticket with it is offered to workers
/// whether or not an epic is active, so a fault report is never left with nobody to take it.
pub const HUB_LABEL: &str = "hub";

/// The ticket columns that identify the pool it may draw workers from.
pub(crate) const SOURCE_FIELDS: &str = "
    metadata -> 'ignite_fault' ->> 'category',
    EXISTS (SELECT 1 FROM json_each(labels) WHERE value IN ('flaky', 'flaky-check')),
    saving IS NOT NULL OR EXISTS (SELECT 1 FROM json_each(labels) WHERE value = 'improvement'),
    EXISTS (SELECT 1 FROM json_each(labels) WHERE value = 'hub')";

/// Find the source of both new automatic filings and reports that predate source controls.
pub(crate) fn allocation_source(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<ignite_core::shares::Source>> {
    use ignite_core::shares::Source;

    let category: Option<String> = row.get(index)?;
    let flaky: bool = row.get(index + 1)?;
    let improvement: bool = row.get(index + 2)?;
    let upkeep: bool = row.get(index + 3)?;
    Ok(
        if flaky
            || matches!(
                category.as_deref(),
                Some("flaky" | "flaky-always" | "machine-flaky" | flaky_triage::CATEGORY)
            )
        {
            Some(Source::Flaky)
        } else if category.as_deref() == Some("found") {
            Some(Source::Exploration)
        } else if improvement {
            Some(Source::Improvement)
        } else {
            (category.is_some() || upkeep).then_some(Source::Upkeep)
        },
    )
}

/// How many sightings a ticket remembers.
const SIGHTINGS_KEPT: usize = 50;

/// How many exploration tickets may wait for approval across all check families.
const PENDING_EXPLORATION_LIMIT: usize = 3;

/// How often a lookup and write is tried when other writes keep coming between them.
const ATTEMPTS: usize = 8;

/// Who the hub's own writes are from.
#[derive(Clone, Copy, Debug)]
pub struct Author<'a> {
    pub actor: &'a str,
    pub machine: &'a str,
}

/// The hub writing on its own, from an upkeep pass.
pub const UPKEEP: Author<'static> = Author {
    actor: "ignite-upkeep",
    machine: "hub",
};

/// One report of a fault.
#[derive(Clone, Debug)]
pub struct Filing {
    pub signature: Signature,
    /// Names this sighting; a sighting already recorded on the ticket is not added again.
    pub evidence: String,
    /// The new ticket's fields. Its body is also the comment on a matching open ticket.
    pub fields: TicketFields,
    pub deps: Vec<Dependency>,
    /// File the ticket `open` so the pool can take it. A sighting with this set also releases a
    /// matching ticket that is still `pending_approval`.
    pub released: bool,
}

impl Filing {
    /// Records that the fault holds this tip back and releases its ticket for dispatch.
    pub(super) fn holding_tip(mut self, tip: &str) -> Self {
        self.released = true;
        self.fields
            .metadata
            .get_or_insert_with(BTreeMap::new)
            .insert(BLOCKED_TIPS_KEY.to_owned(), Value::from(vec![tip]));
        self
    }

    /// Records both the tip this fault holds back and the work it is waiting for.
    pub(super) fn holding_platform_wait(self, scope: &PlatformWait) -> Self {
        let mut filing = self.holding_tip(scope.commit.as_str());
        filing
            .fields
            .metadata
            .get_or_insert_with(BTreeMap::new)
            .insert(WAIT_SCOPE_KEY.to_owned(), serde_json::json!(scope));
        filing
    }
}

/// What filing did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filed {
    pub ticket: String,
    pub change: Change,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Created,
    Commented,
    Updated,
    /// The ticket already held this sighting.
    Unchanged,
    /// Upkeep had closed the ticket because the fault cleared, and the fault came back.
    Reopened,
}

struct Match {
    id: String,
    closed: bool,
    pending: bool,
    labels: Vec<String>,
    sightings: Vec<String>,
    metadata: BTreeMap<String, Value>,
}

/// Files `filing` on a matching live ticket, or starts a new one. Exploration findings are
/// grouped by check family before approval and can share tickets across families.
pub fn file(
    conn: &mut Connection,
    author: Author,
    filing: &Filing,
    now: Timestamp,
) -> Result<Filed> {
    file_if(conn, author, filing, now, |_| Ok(true))?
        .ok_or_else(|| StoreError::Invalid("an unconditional filing was skipped".to_owned()))
}

/// Files only if `needed` still holds when the write transaction starts.
pub(super) fn file_if(
    conn: &mut Connection,
    author: Author,
    filing: &Filing,
    now: Timestamp,
    needed: impl Fn(&Connection) -> Result<bool>,
) -> Result<Option<Filed>> {
    file_with(conn, author, filing, now, needed, |_, _| Ok(()))
}

/// Records the effect of a new sighting in the transaction that files its ticket.
pub(super) fn file_then(
    conn: &mut Connection,
    filing: &Filing,
    now: Timestamp,
    record: impl FnOnce(&Connection, &str) -> Result<()>,
) -> Result<Filed> {
    file_with(conn, UPKEEP, filing, now, |_| Ok(true), record)?
        .ok_or_else(|| StoreError::Invalid("an unconditional filing was skipped".to_owned()))
}

/// Replaces the summary of the pending batch, or creates it. The caller holds the write
/// transaction from evidence selection through acknowledgement, so approval freezes the batch.
pub(super) fn file_pending_batch(
    tx: &mut Transaction<'_>,
    mut filing: Filing,
    now: Timestamp,
    summary: impl FnOnce(&Connection, Option<&str>) -> Result<String>,
) -> Result<Filed> {
    let key = filing.signature.key();
    let pending = load_matches(
        tx,
        "json_extract(metadata, '$.ignite_signature') = ?1 AND status = 'pending_approval'
         AND NOT EXISTS (SELECT 1 FROM history h WHERE h.ticket = tickets.id
                         AND json_extract(h.change, '$.status[1]') != 'pending_approval')",
        &key,
    )?;
    let pending = pending.first();
    filing.fields.body = Some(summary(tx, pending.map(|ticket| ticket.id.as_str()))?);
    let (op, change) = match pending {
        Some(ticket) => (
            Operation::Update {
                id: ticket.id.clone(),
                set: Some(TicketFields {
                    body: filing.fields.body.clone(),
                    ..TicketFields::default()
                }),
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: sighting_meta(ticket, &filing, UPKEEP, now),
                unset_meta: Vec::new(),
            },
            Change::Updated,
        ),
        None => (
            create(&filing, &key, UPKEEP, None, None, now),
            Change::Created,
        ),
    };
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![op],
    };
    let BatchOutcome::Applied(reply) =
        batch::apply_automatic_in(tx, &request, UPKEEP.actor, UPKEEP.machine, now)?
    else {
        return Err(StoreError::Invalid(
            "the pending batch filing conflicted".to_owned(),
        ));
    };
    Ok(Filed {
        ticket: reply.results[0].id.to_string(),
        change,
    })
}

fn file_with(
    conn: &mut Connection,
    author: Author,
    filing: &Filing,
    now: Timestamp,
    needed: impl Fn(&Connection) -> Result<bool>,
    record: impl FnOnce(&Connection, &str) -> Result<()>,
) -> Result<Option<Filed>> {
    let key = filing.signature.key();
    let body = filing.fields.body.clone().unwrap_or_default();
    let origin = origin(conn, author.machine)?;
    let family_label = (filing.signature.category == "found")
        .then(|| format!("found-by:{}", filing.signature.subject));
    for _ in 0..ATTEMPTS {
        let seq = feed::current(conn)?;
        let matches;
        let live = if let Some(label) = &family_label {
            matches = exploration_matches(conn)?;
            if let Some(seen) = matches
                .iter()
                .find(|m| saw_exploration(m, &filing.evidence, &key))
            {
                return Ok(Some(Filed {
                    ticket: seen.id.clone(),
                    change: Change::Unchanged,
                }));
            }
            pending_exploration(&matches, label)
        } else {
            matches = matching(conn, &key)?;
            matches.iter().find(|m| !m.closed)
        };
        let (ops, change) = match live {
            Some(open)
                if open.sightings.contains(&filing.evidence)
                    && new_blocked_tips(open, filing).is_none()
                    && new_wait_scope(open, filing).is_none() =>
            {
                return Ok(Some(Filed {
                    ticket: open.id.clone(),
                    change: Change::Unchanged,
                }));
            }
            Some(open) => {
                let release = filing.released && open.pending;
                let added_family = family_label
                    .as_ref()
                    .filter(|label| !open.labels.contains(label));
                let ops = vec![
                    Operation::Comment {
                        id: open.id.clone(),
                        text: body.clone(),
                    },
                    Operation::Update {
                        id: open.id.clone(),
                        set: (release || added_family.is_some()).then(|| TicketFields {
                            status: release.then_some(Status::Open),
                            title: added_family
                                .map(|_| "Investigate exploration findings".to_owned()),
                            ..TicketFields::default()
                        }),
                        add_labels: added_family.into_iter().cloned().collect(),
                        remove_labels: Vec::new(),
                        set_meta: sighting_meta(open, filing, author, now),
                        // The fault is seen again, so any clear spell upkeep was timing ends.
                        unset_meta: vec![CLEARED_SINCE_KEY.to_owned()],
                    },
                ];
                (ops, Change::Commented)
            }
            None => match matches.first().filter(|m| m.closed) {
                Some(newest) if auto_closed_within(newest, now) => {
                    (reopen(newest, filing, author, now), Change::Reopened)
                }
                previous => (
                    vec![create(
                        filing,
                        &key,
                        author,
                        origin.as_deref(),
                        previous.map(|m| m.id.as_str()),
                        now,
                    )],
                    Change::Created,
                ),
            },
        };
        let request = BatchRequest {
            preconditions: vec![Precondition::Seq { seq }],
            ops,
        };
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !needed(&tx)? {
            return Ok(None);
        }
        match batch::apply_automatic_in(&mut tx, &request, author.actor, author.machine, now)? {
            BatchOutcome::Applied(reply) => {
                let ticket = reply.results[0].id.to_string();
                record(&tx, &ticket)?;
                tx.commit()?;
                return Ok(Some(Filed { ticket, change }));
            }
            BatchOutcome::Conflict(_) => {}
        }
    }
    Err(StoreError::Invalid(format!(
        "filing {} did not commit after {ATTEMPTS} attempts",
        filing.signature.subject
    )))
}

/// The open ticket with this signature, if there is one.
pub fn open_ticket(conn: &Connection, signature: &Signature) -> Result<Option<String>> {
    Ok(matching(conn, &signature.key())?
        .into_iter()
        .find(|m| !m.closed)
        .map(|m| m.id))
}

/// How many tickets that are not closed carry every label of `labels` and hold the sighting
/// `evidence`.
pub fn sighted(conn: &Connection, labels: &[&str], evidence: &str) -> Result<usize> {
    let labels = serde_json::to_string(labels)
        .map_err(|error| StoreError::Invalid(format!("labels {labels:?}: {error}")))?;
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM tickets t
         WHERE t.status != 'closed'
           AND NOT EXISTS (
             SELECT 1 FROM json_each(?1) wanted
             WHERE NOT EXISTS (SELECT 1 FROM json_each(t.labels) had WHERE had.value = wanted.value))
           AND EXISTS (
             SELECT 1 FROM json_each(t.metadata, '$.ignite_sightings') seen
             WHERE seen.value = ?2)",
        params![labels, evidence],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Records that `tip` waits on repairs for these checks on `platform`, including portable
/// checks. Releases any of those tickets still waiting for approval in the same transaction.
pub fn release_holding(
    conn: &mut Connection,
    checks: &[&str],
    platform: Os,
    tip: &str,
    now: Timestamp,
) -> Result<()> {
    let wanted = serde_json::to_string(checks)
        .map_err(|error| StoreError::Invalid(format!("checks {checks:?}: {error}")))?;
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids = {
        let mut select = tx.prepare(
            "SELECT t.id FROM tickets t
             WHERE t.status IN ('pending_approval', 'open', 'in_progress')
               AND (t.needs_os IS NULL OR t.needs_os = ?3)
               AND EXISTS (SELECT 1 FROM json_each(t.labels) l WHERE l.value = ?1)
               AND EXISTS (
                 SELECT 1 FROM json_each(t.metadata, '$.ignite_checks') held
                 WHERE held.value IN (SELECT value FROM json_each(?2)))
             ORDER BY t.id",
        )?;
        select
            .query_map(params![HUB_LABEL, wanted, platform.as_str()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut ops = Vec::new();
    for id in ids {
        let Some(ticket) = tickets::load(&tx, &id)? else {
            continue;
        };
        let mut tips = strings(&ticket.metadata, BLOCKED_TIPS_KEY);
        let release = ticket.status == Status::PendingApproval;
        if !release && tips.iter().any(|held| held == tip) {
            continue;
        }
        tips = union(tips, vec![tip.to_owned()]);
        ops.push(Operation::Update {
            id,
            set: release.then(|| TicketFields {
                status: Some(Status::Open),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::from([(BLOCKED_TIPS_KEY.to_owned(), Value::from(tips))]),
            unset_meta: Vec::new(),
        });
    }
    if !ops.is_empty() {
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops,
        };
        if let BatchOutcome::Conflict(conflict) =
            batch::apply_automatic_in(&mut tx, &request, UPKEEP.actor, UPKEEP.machine, now)?
        {
            return Err(StoreError::Invalid(format!(
                "recording a tip wait: {conflict:?}"
            )));
        }
    }
    Ok(tx.commit()?)
}

/// The pin for the machine that filed a report, if it is registered.
pub fn origin(conn: &Connection, machine: &str) -> Result<Option<String>> {
    if machine == UPKEEP.machine {
        return Ok(None);
    }
    Ok(fleet::name_of(conn, machine)?.map(|name| machine_label(&name)))
}

fn create(
    filing: &Filing,
    key: &str,
    author: Author,
    origin: Option<&str>,
    previous: Option<&str>,
    now: Timestamp,
) -> Operation {
    let mut fields = filing.fields.clone();
    if let Some(pin) = origin {
        let labels = fields.labels.get_or_insert_with(Vec::new);
        if from_labels(labels).machine.is_none() {
            labels.push(pin.to_owned());
        }
    }
    if filing.released {
        fields.fallback = Some(true);
    }
    let mut metadata = fields.metadata.take().unwrap_or_default();
    metadata.insert(SIGNATURE_KEY.to_owned(), Value::from(key));
    metadata.insert(FAULT_KEY.to_owned(), fault_value(&filing.signature));
    metadata.insert(FILERS_KEY.to_owned(), Value::from(vec![author.actor]));
    metadata.insert(SIGHTED_KEY.to_owned(), Value::from(now.to_string()));
    metadata.extend(sightings_meta(vec![filing.evidence.clone()]));
    if let Some(previous) = previous {
        let body = fields.body.take().unwrap_or_default();
        fields.body = Some(format!(
            "{body}\n\nThe same fault was filed before as {previous}, which is closed."
        ));
    }
    fields.metadata = Some(metadata);
    let mut labels = fields.labels.take().unwrap_or_default();
    if !labels.iter().any(|label| label == HUB_LABEL) {
        labels.push(HUB_LABEL.to_owned());
    }
    fields.labels = Some(labels);
    // Nothing is scheduled until a person releases it, the hub's own tickets included, unless
    // the fault holds `master` back.
    fields.status = Some(if filing.released {
        Status::Open
    } else {
        Status::PendingApproval
    });
    Operation::Create {
        fields,
        parent: None,
        deps: filing.deps.clone(),
        request_key: None,
    }
}

/// The parts of a signature that its key hides, so upkeep can tell what the ticket is about.
pub fn fault_value(signature: &Signature) -> Value {
    serde_json::json!({
        "subject": signature.subject,
        "category": signature.category,
        "platform": signature.platform,
    })
}

/// The strings in a ticket's metadata list `key`.
pub fn strings(metadata: &BTreeMap<String, Value>, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `list` with each of `more` it does not already hold added at the end.
fn union(mut list: Vec<String>, more: Vec<String>) -> Vec<String> {
    for item in more {
        if !list.contains(&item) {
            list.push(item);
        }
    }
    list
}

/// The merged tips if this report adds a tip the ticket did not already block.
fn new_blocked_tips(ticket: &Match, filing: &Filing) -> Option<Vec<String>> {
    let incoming = strings(filing.fields.metadata.as_ref()?, BLOCKED_TIPS_KEY);
    if incoming.is_empty() {
        return None;
    }
    let previous = strings(&ticket.metadata, BLOCKED_TIPS_KEY);
    let count = previous.len();
    let tips = union(previous, incoming);
    (tips.len() > count).then_some(tips)
}

/// A platform wait report can add scope to a silent machine's ticket or update an earlier wait.
fn new_wait_scope<'a>(ticket: &Match, filing: &'a Filing) -> Option<&'a Value> {
    let scope = filing.fields.metadata.as_ref()?.get(WAIT_SCOPE_KEY)?;
    (Some(scope) != ticket.metadata.get(WAIT_SCOPE_KEY)).then_some(scope)
}

/// The metadata a new sighting writes on a ticket that already exists: the sighting, its filer,
/// its time, its checks and, on a ticket filed before tickets recorded it, the fault.
fn sighting_meta(
    ticket: &Match,
    filing: &Filing,
    author: Author,
    now: Timestamp,
) -> BTreeMap<String, Value> {
    let mut sightings = ticket.sightings.clone();
    sightings.push(filing.evidence.clone());
    let mut meta = sightings_meta(sightings);
    let filers = union(
        strings(&ticket.metadata, FILERS_KEY),
        vec![author.actor.to_owned()],
    );
    meta.insert(FILERS_KEY.to_owned(), Value::from(filers));
    meta.insert(SIGHTED_KEY.to_owned(), Value::from(now.to_string()));
    let checks = filing
        .fields
        .metadata
        .as_ref()
        .map(|meta| strings(meta, CHECKS_KEY))
        .unwrap_or_default();
    if !checks.is_empty() || ticket.metadata.contains_key(CHECKS_KEY) {
        let checks = union(strings(&ticket.metadata, CHECKS_KEY), checks);
        meta.insert(CHECKS_KEY.to_owned(), Value::from(checks));
    }
    if !ticket.metadata.contains_key(FAULT_KEY) {
        meta.insert(FAULT_KEY.to_owned(), fault_value(&filing.signature));
    }
    if let Some(tips) = new_blocked_tips(ticket, filing) {
        meta.insert(BLOCKED_TIPS_KEY.to_owned(), Value::from(tips));
    }
    if let Some(scope) = new_wait_scope(ticket, filing) {
        meta.insert(WAIT_SCOPE_KEY.to_owned(), scope.clone());
    }
    meta
}

/// Whether upkeep closed `ticket` because its fault cleared, within [`REOPEN_SECONDS`] of `now`.
fn auto_closed_within(ticket: &Match, now: Timestamp) -> bool {
    ticket.closed
        && ticket
            .metadata
            .get(AUTO_CLOSED_KEY)
            .and_then(Value::as_str)
            .and_then(|at| Timestamp::parse(at).ok())
            .is_some_and(|at| now.seconds_since(at) < REOPEN_SECONDS)
}

/// Reopens a ticket that upkeep closed when its fault cleared, back in the status it had, with
/// the new sighting as a comment.
fn reopen(ticket: &Match, filing: &Filing, author: Author, now: Timestamp) -> Vec<Operation> {
    let body = filing.fields.body.clone().unwrap_or_default();
    let mut meta = sighting_meta(ticket, filing, author, now);
    let returns = ticket
        .metadata
        .get(RETURNS_KEY)
        .and_then(Value::as_u64)
        .unwrap_or(0);
    meta.insert(RETURNS_KEY.to_owned(), Value::from(returns + 1));
    // Reopening makes a ticket `open`; one closed before a person released it goes back to
    // waiting for that release, unless the fault now holds `master` back.
    let status = ticket
        .metadata
        .get(STATUS_BEFORE_KEY)
        .and_then(|value| serde_json::from_value::<Status>(value.clone()).ok())
        .filter(|status| *status != Status::Open)
        .filter(|status| !(filing.released && *status == Status::PendingApproval));
    vec![
        Operation::Reopen {
            id: ticket.id.clone(),
        },
        Operation::Update {
            id: ticket.id.clone(),
            set: status.map(|status| TicketFields {
                status: Some(status),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: meta,
            unset_meta: vec![
                AUTO_CLOSED_KEY.to_owned(),
                STATUS_BEFORE_KEY.to_owned(),
                CLEARED_SINCE_KEY.to_owned(),
            ],
        },
        Operation::Comment {
            id: ticket.id.clone(),
            text: format!(
                "The fault came back after Ignite closed this ticket as cleared, so Ignite \
                 reopened it.\n\n{body}"
            ),
        },
    ]
}

fn sightings_meta(mut sightings: Vec<String>) -> BTreeMap<String, Value> {
    if sightings.len() > SIGHTINGS_KEPT {
        sightings.drain(..sightings.len() - SIGHTINGS_KEPT);
    }
    BTreeMap::from([(SIGHTINGS_KEY.to_owned(), Value::from(sightings))])
}

/// Every ticket with this signature, newest first.
fn matching(conn: &Connection, key: &str) -> Result<Vec<Match>> {
    load_matches(
        conn,
        "json_extract(metadata, '$.ignite_signature') = ?1",
        key,
    )
}

/// Released tickets only participate in retry detection. Their scope stays fixed after approval.
fn exploration_matches(conn: &Connection) -> Result<Vec<Match>> {
    load_matches(
        conn,
        "status != 'closed' AND json_extract(metadata, '$.ignite_fault.category') = ?1",
        "found",
    )
}

/// Older exploration tickets stored the replay and commit without the diagnostic signature.
fn saw_exploration(ticket: &Match, evidence: &str, key: &str) -> bool {
    let legacy = evidence.split_once(':').map(|(_, replay)| replay);
    let same_fault = ticket.metadata.get(SIGNATURE_KEY).and_then(Value::as_str) == Some(key);
    ticket
        .sightings
        .iter()
        .any(|seen| seen == evidence || (same_fault && Some(seen.as_str()) == legacy))
}

/// Keep a family together while a pending ticket covers it. When all slots are occupied,
/// put a new family on the ticket with the fewest recorded sightings.
fn pending_exploration<'a>(matches: &'a [Match], family_label: &str) -> Option<&'a Match> {
    let pending = matches.iter().filter(|ticket| ticket.pending);
    pending
        .clone()
        .find(|ticket| ticket.labels.iter().any(|label| label == family_label))
        .or_else(|| {
            (pending.clone().count() >= PENDING_EXPLORATION_LIMIT)
                .then(|| pending.min_by_key(|ticket| ticket.sightings.len()))
                .flatten()
        })
}

fn load_matches(conn: &Connection, condition: &str, value: &str) -> Result<Vec<Match>> {
    let mut select = conn.prepare_cached(&format!(
        "SELECT id, status, labels, metadata FROM tickets
         WHERE {condition}
         ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = select
        .query_map(params![value], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, status, labels, metadata)| {
            let labels = serde_json::from_str(&labels)
                .map_err(|error| StoreError::Invalid(format!("labels of {id}: {error}")))?;
            let metadata: BTreeMap<String, Value> = match metadata {
                Some(json) => serde_json::from_str(&json)
                    .map_err(|error| StoreError::Invalid(format!("metadata of {id}: {error}")))?,
                None => BTreeMap::new(),
            };
            Ok(Match {
                closed: status == "closed",
                pending: status == "pending_approval",
                id,
                labels,
                sightings: strings(&metadata, SIGHTINGS_KEY),
                metadata,
            })
        })
        .collect()
}
