//! Filing a ticket the hub writes by itself, matched against earlier ones by signature.
//!
//! - An open ticket with the same signature gets a comment instead of a second ticket.
//!   Exploration reports share a small set of tickets while they wait for approval.
//! - Each report carries an `evidence` string that names the sighting, such as a commit or an
//!   input hash. A report whose evidence the ticket already holds changes nothing, so a pass of
//!   upkeep that sees the same fault again, or a daemon that sends a report twice, adds nothing.
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
//! Each new ticket is owned by the machine whose report filed it (see `ignite_core::owner`),
//! unless its filing names another owner; one that upkeep files on its own follows the default
//! owner. A released ticket also falls back to another machine when its owner goes silent.
//!
//! A ticket is filed `pending_approval`, so the pool never takes it before a person releases it,
//! unless its filing says it is [`Filing::released`]: a fault that holds `master` back cannot
//! wait for a person. A later released sighting releases a ticket still waiting for a person.
//!
//! The lookup and the write are fenced by the feed position: if anything else wrote between
//! them, the write is refused and the lookup is done again.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::Value;

use ignite_core::owner::Owner;

use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, Operation, Precondition, Status, TicketFields,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{feed, fleet};
use ignite_store::{Result, StoreError};

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
    /// Whether the fault holds `master` back, so that its ticket is filed `open` for the pool
    /// to take at once instead of waiting for a person to release it. A sighting with this set
    /// also releases a matching ticket that is still `pending_approval`.
    pub released: bool,
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
                return Ok(Filed {
                    ticket: seen.id.clone(),
                    change: Change::Unchanged,
                });
            }
            pending_exploration(&matches, label)
        } else {
            matches = matching(conn, &key)?;
            matches.iter().find(|m| !m.closed)
        };
        let (ops, change) = match live {
            Some(open) if open.sightings.contains(&filing.evidence) => {
                return Ok(Filed {
                    ticket: open.id.clone(),
                    change: Change::Unchanged,
                });
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
                        origin.as_ref(),
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
        match batch::apply_automatic(conn, &request, author.actor, author.machine, now)? {
            BatchOutcome::Applied(reply) => {
                return Ok(Filed {
                    ticket: reply.results[0].id.to_string(),
                    change,
                });
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

/// Releases each ticket the hub filed that still waits for a person (`pending_approval`) and
/// holds back one of `checks`, which a tip now waits on, so that `master` does not wait for a
/// person to release it. Returns the tickets released.
///
/// The release is written by [`UPKEEP`] and changes nothing but the status, so
/// [`super::clearing`] counts it as it counts a person's release and still closes the ticket
/// once its fault clears.
pub fn release_holding(
    conn: &mut Connection,
    checks: &[&str],
    now: Timestamp,
) -> Result<Vec<String>> {
    let wanted = serde_json::to_string(checks)
        .map_err(|error| StoreError::Invalid(format!("checks {checks:?}: {error}")))?;
    let ids = {
        let mut select = conn.prepare(
            "SELECT t.id FROM tickets t
             WHERE t.status = 'pending_approval'
               AND EXISTS (SELECT 1 FROM json_each(t.labels) l WHERE l.value = ?1)
               AND EXISTS (
                 SELECT 1 FROM json_each(t.metadata, '$.ignite_checks') held
                 WHERE held.value IN (SELECT value FROM json_each(?2)))
             ORDER BY t.id",
        )?;
        select
            .query_map(params![HUB_LABEL, wanted], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in &ids {
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops: vec![Operation::Update {
                id: id.clone(),
                set: Some(TicketFields {
                    status: Some(Status::Open),
                    ..TicketFields::default()
                }),
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: BTreeMap::new(),
                unset_meta: Vec::new(),
            }],
        };
        batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?;
    }
    Ok(ids)
}

/// The owner of a ticket that the machine `machine` (its ID) files: that machine with its home
/// app. `None` for upkeep itself and for a machine the hub has no registration of.
pub fn origin(conn: &Connection, machine: &str) -> Result<Option<Owner>> {
    if machine == UPKEEP.machine {
        return Ok(None);
    }
    Ok(fleet::name_of(conn, machine)?.map(|name| Owner::new(&name, None)))
}

fn create(
    filing: &Filing,
    key: &str,
    author: Author,
    origin: Option<&Owner>,
    previous: Option<&str>,
    now: Timestamp,
) -> Operation {
    let mut fields = filing.fields.clone();
    if fields.owner.is_none() {
        fields.owner = origin.map(|owner| Some(owner.clone()));
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
