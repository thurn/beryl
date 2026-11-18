//! Filing a ticket the hub writes by itself, matched against earlier ones by signature.
//!
//! - An open ticket with the same signature gets a comment instead of a second ticket.
//! - Each report carries an `evidence` string that names the sighting, such as a commit or an
//!   input hash. A report whose evidence the ticket already holds changes nothing, so a pass of
//!   upkeep that sees the same fault again, or a daemon that sends a report twice, adds nothing.
//! - A fault that returns after its ticket closed gets a new ticket that names the old one.
//!
//! The lookup and the write are fenced by the feed position: if anything else wrote between
//! them, the write is refused and the lookup is done again.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::Value;

use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, Operation, Precondition, Status, TicketFields,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::feed;
use ignite_store::{Result, StoreError};

use super::signature::Signature;

/// The ticket metadata key that holds a signature's key.
pub const SIGNATURE_KEY: &str = "ignite_signature";

/// The ticket metadata key that holds the evidence of each sighting already recorded.
pub const SIGHTINGS_KEY: &str = "ignite_sightings";

/// The label on every ticket the hub files by itself. A ticket with it is offered to workers
/// whether or not an epic is active, so a fault report is never left with nobody to take it.
pub const HUB_LABEL: &str = "hub";

/// How many sightings a ticket remembers.
const SIGHTINGS_KEPT: usize = 50;

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
}

struct Match {
    id: String,
    closed: bool,
    sightings: Vec<String>,
}

/// Files `filing`: a comment on the open ticket with its signature, or a new ticket.
pub fn file(
    conn: &mut Connection,
    author: Author,
    filing: &Filing,
    now: Timestamp,
) -> Result<Filed> {
    let key = filing.signature.key();
    let body = filing.fields.body.clone().unwrap_or_default();
    for _ in 0..ATTEMPTS {
        let seq = feed::current(conn)?;
        let matches = matching(conn, &key)?;
        let live = matches.iter().find(|m| !m.closed);
        let (ops, change) = match live {
            Some(open) if open.sightings.contains(&filing.evidence) => {
                return Ok(Filed {
                    ticket: open.id.clone(),
                    change: Change::Unchanged,
                });
            }
            Some(open) => {
                let mut sightings = open.sightings.clone();
                sightings.push(filing.evidence.clone());
                let ops = vec![
                    Operation::Comment {
                        id: open.id.clone(),
                        text: body.clone(),
                    },
                    Operation::Update {
                        id: open.id.clone(),
                        set: None,
                        add_labels: Vec::new(),
                        remove_labels: Vec::new(),
                        set_meta: sightings_meta(sightings),
                        unset_meta: Vec::new(),
                    },
                ];
                (ops, Change::Commented)
            }
            None => {
                let previous = matches.first().map(|m| m.id.as_str());
                (vec![create(filing, &key, previous)], Change::Created)
            }
        };
        let request = BatchRequest {
            preconditions: vec![Precondition::Seq { seq }],
            ops,
        };
        match batch::apply(conn, &request, author.actor, author.machine, now)? {
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

fn create(filing: &Filing, key: &str, previous: Option<&str>) -> Operation {
    let mut fields = filing.fields.clone();
    let mut metadata = fields.metadata.take().unwrap_or_default();
    metadata.insert(SIGNATURE_KEY.to_owned(), Value::from(key));
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
    // Nothing is scheduled until a person releases it, the hub's own tickets included.
    fields.status = Some(Status::PendingApproval);
    Operation::Create {
        fields,
        parent: None,
        deps: filing.deps.clone(),
        request_key: None,
    }
}

fn sightings_meta(mut sightings: Vec<String>) -> BTreeMap<String, Value> {
    if sightings.len() > SIGHTINGS_KEPT {
        sightings.drain(..sightings.len() - SIGHTINGS_KEPT);
    }
    BTreeMap::from([(SIGHTINGS_KEY.to_owned(), Value::from(sightings))])
}

/// Every ticket with this signature, newest first.
fn matching(conn: &Connection, key: &str) -> Result<Vec<Match>> {
    let mut select = conn.prepare_cached(
        "SELECT id, status, json_extract(metadata, '$.ignite_sightings') FROM tickets
         WHERE json_extract(metadata, '$.ignite_signature') = ?1
         ORDER BY created_at DESC, id DESC",
    )?;
    let rows = select
        .query_map(params![key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, status, sightings)| {
            let sightings = match sightings {
                Some(json) => serde_json::from_str(&json)
                    .map_err(|error| StoreError::Invalid(format!("sightings of {id}: {error}")))?,
                None => Vec::new(),
            };
            Ok(Match {
                closed: status == "closed",
                id,
                sightings,
            })
        })
        .collect()
}
