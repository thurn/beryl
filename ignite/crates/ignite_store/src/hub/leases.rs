//! Named, expiring leases with a queue of waiters, ported from the ticket service.
//!
//! - A lease belongs to a holder on a machine until `expires_at`. The holder renews it to keep
//!   it, with a new `ttl_s`, and releases it when done.
//! - An `acquire` of a lease someone else holds joins the queue and answers 409 with the
//!   caller's position, unless it says `no_queue`. A waiter stays in the queue for
//!   [`WAITER_SECONDS`] after its last attempt, so a session that stops retrying drops out.
//! - When a lease is free, only the head of the queue may take it.
//! - `if_revision` on a renew or release succeeds only if the lease's payload has that
//!   `revision`, so a holder can tell its own state from a newer one.
//! - Every change to a lease or to who is queued takes the next feed position and leaves a
//!   change naming the lease, so a waiter long-polling the feed wakes when it is its turn.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use ignite_core::protocol::tickets::{
    FeedChange, Lease, LeaseBusy, LeaseOp, LeaseReply, LeaseWaiter,
};
use ignite_core::time::Timestamp;

use crate::hub::feed;
use crate::{Result, StoreError};

/// How long a waiter keeps its place in the queue after its last attempt.
pub const WAITER_SECONDS: f64 = 60.0;

/// Applies one operation to the lease `name` for a holder on `machine`.
pub fn apply(
    conn: &mut Connection,
    name: &str,
    op: &LeaseOp,
    machine: &str,
    now: Timestamp,
) -> Result<LeaseReply> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let reply = update(&transaction, name, op, machine, now)?;
    transaction.commit()?;
    Ok(reply)
}

fn update(
    conn: &Connection,
    name: &str,
    op: &LeaseOp,
    machine: &str,
    now: Timestamp,
) -> Result<LeaseReply> {
    let (holder, ttl_s, payload, if_revision, no_queue) = parts(op);
    let releasing = matches!(op, LeaseOp::Release { .. });
    if holder.is_empty() {
        return Err(StoreError::Rejected("holder must have a value".to_owned()));
    }
    if !releasing && ttl_s == 0 {
        return Err(StoreError::Rejected("ttl_s must be positive".to_owned()));
    }
    if payload.is_some_and(|p| !p.is_object()) {
        return Err(StoreError::Rejected("payload must be an object".to_owned()));
    }

    let current = load(conn, name)?;
    let active = current.as_ref().is_some_and(|c| c.expires_at > now);
    let held = active
        && current
            .as_ref()
            .is_some_and(|c| c.holder == *holder && c.machine == machine);
    let waiters = current
        .as_ref()
        .map(|c| live_waiters(&c.waiters, now))
        .unwrap_or_default();
    let caller = |waiter: &LeaseWaiter| {
        waiter.holder == *holder && waiter.machine.as_deref().unwrap_or(machine) == machine
    };

    if !matches!(op, LeaseOp::Acquire { .. }) {
        let revision_differs = if_revision.is_some_and(|expected| {
            current
                .as_ref()
                .is_none_or(|c| c.payload.get("revision").and_then(Value::as_str) != Some(expected))
        });
        if !held || revision_differs {
            let current = current.filter(|_| active);
            return Ok(LeaseReply::Busy(LeaseBusy {
                holder: current.as_ref().map(|c| c.holder.clone()),
                expires_at: current.map(|c| c.expires_at),
                position: None,
                seq: None,
            }));
        }
    }

    if let (LeaseOp::Acquire { .. }, Some(current)) = (op, &current)
        && !held
        && (active || waiters.first().is_some_and(|head| !caller(head)))
    {
        let caller = Caller { holder, machine };
        return wait(conn, current, waiters, &caller, no_queue, now);
    }

    if releasing && waiters.is_empty() {
        conn.execute("DELETE FROM leases WHERE name = ?1", [name])?;
        advance(conn, name)?;
        let current = current.expect("a held lease exists");
        return Ok(LeaseReply::Held(Lease {
            expires_at: now,
            waiters,
            ..current
        }));
    }

    let payload = if releasing {
        current
            .as_ref()
            .map_or_else(|| json!({}), |c| c.payload.clone())
    } else {
        match (payload, &current) {
            (Some(payload), _) => payload.clone(),
            (None, Some(current)) if held => current.payload.clone(),
            (None, _) => json!({}),
        }
    };
    let next = Lease {
        name: name.to_owned(),
        holder: holder.clone(),
        machine: machine.to_owned(),
        expires_at: if releasing {
            now
        } else {
            now.plus_seconds(ttl_s as f64)
        },
        payload,
        waiters: waiters.into_iter().filter(|w| !caller(w)).collect(),
    };
    conn.execute(
        "INSERT INTO leases (name, holder, machine, expires_at, payload, waiters) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(name) DO UPDATE SET holder = excluded.holder, machine = excluded.machine, \
           expires_at = excluded.expires_at, payload = excluded.payload, waiters = excluded.waiters",
        params![
            next.name,
            next.holder,
            next.machine,
            next.expires_at.to_string(),
            next.payload.to_string(),
            json!(next.waiters).to_string()
        ],
    )?;
    advance(conn, name)?;
    Ok(LeaseReply::Held(next))
}

/// An operation's holder, `ttl_s` (0 for a release), payload, expected revision and
/// `no_queue`.
#[allow(clippy::type_complexity)]
fn parts(op: &LeaseOp) -> (&String, u64, Option<&Value>, Option<&str>, bool) {
    match op {
        LeaseOp::Acquire {
            holder,
            ttl_s,
            payload,
            no_queue,
        } => (holder, *ttl_s, payload.as_ref(), None, *no_queue),
        LeaseOp::Renew {
            holder,
            ttl_s,
            payload,
            if_revision,
        } => (
            holder,
            *ttl_s,
            payload.as_ref(),
            if_revision.as_deref(),
            false,
        ),
        LeaseOp::Release {
            holder,
            if_revision,
        } => (holder, 0, None, if_revision.as_deref(), false),
    }
}

/// Who is asking for a lease.
struct Caller<'a> {
    holder: &'a str,
    machine: &'a str,
}

impl Caller<'_> {
    fn is(&self, waiter: &LeaseWaiter) -> bool {
        waiter.holder == self.holder
            && waiter.machine.as_deref().unwrap_or(self.machine) == self.machine
    }
}

/// Answers an acquire of a lease someone else holds, or that is free but promised to the head
/// of the queue: busy, after putting the caller in the queue unless `no_queue`. A new waiter
/// takes a feed position; a waiter retrying only refreshes its time.
fn wait(
    conn: &Connection,
    current: &Lease,
    mut waiters: Vec<LeaseWaiter>,
    caller: &Caller<'_>,
    no_queue: bool,
    now: Timestamp,
) -> Result<LeaseReply> {
    if no_queue {
        return Ok(LeaseReply::Busy(LeaseBusy {
            holder: Some(current.holder.clone()),
            expires_at: Some(current.expires_at),
            position: None,
            seq: Some(feed::current(conn)?),
        }));
    }
    let index = waiters.iter().position(|w| caller.is(w));
    let membership_changed = index.is_none() || waiters.len() != current.waiters.len();
    let waiter = LeaseWaiter {
        holder: caller.holder.to_owned(),
        machine: Some(caller.machine.to_owned()),
        since: now,
    };
    let position = if let Some(index) = index {
        waiters[index] = waiter;
        index + 1
    } else {
        waiters.push(waiter);
        waiters.len()
    };
    if waiters != current.waiters {
        conn.execute(
            "UPDATE leases SET waiters = ?1 WHERE name = ?2",
            params![json!(waiters).to_string(), current.name],
        )?;
    }
    if membership_changed {
        advance(conn, &current.name)?;
    }
    Ok(LeaseReply::Busy(LeaseBusy {
        holder: Some(current.holder.clone()),
        expires_at: Some(current.expires_at),
        position: Some(position as u32),
        seq: Some(feed::current(conn)?),
    }))
}

/// Every lease that has not expired or still has someone waiting, sorted by name, with only the
/// waiters still in the queue.
pub fn list(conn: &Connection, now: Timestamp) -> Result<Vec<Lease>> {
    let mut statement = conn.prepare("SELECT name FROM leases ORDER BY name")?;
    let names: Vec<String> = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut leases = Vec::new();
    for name in names {
        if let Some(lease) = load(conn, &name)? {
            let waiters = live_waiters(&lease.waiters, now);
            if lease.expires_at > now || !waiters.is_empty() {
                leases.push(Lease { waiters, ..lease });
            }
        }
    }
    Ok(leases)
}

fn load(conn: &Connection, name: &str) -> Result<Option<Lease>> {
    let row: Option<(String, String, String, String, String, String)> = conn
        .query_row(
            "SELECT name, holder, machine, expires_at, payload, waiters FROM leases WHERE name = ?1",
            [name],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((name, holder, machine, expires_at, payload, waiters)) = row else {
        return Ok(None);
    };
    let invalid = |what: &str, error: &dyn std::fmt::Display| {
        StoreError::Invalid(format!("lease {name:?} {what}: {error}"))
    };
    Ok(Some(Lease {
        expires_at: Timestamp::parse(&expires_at).map_err(|e| invalid("expires_at", &e))?,
        payload: serde_json::from_str(&payload).map_err(|e| invalid("payload", &e))?,
        waiters: serde_json::from_str(&waiters).map_err(|e| invalid("waiters", &e))?,
        name: name.clone(),
        holder,
        machine,
    }))
}

fn live_waiters(waiters: &[LeaseWaiter], now: Timestamp) -> Vec<LeaseWaiter> {
    waiters
        .iter()
        .filter(|w| w.since.plus_seconds(WAITER_SECONDS) > now)
        .cloned()
        .collect()
}

/// Takes the next feed position for a change to the lease `name`.
fn advance(conn: &Connection, name: &str) -> Result<()> {
    let seq = feed::next_seq(conn)?;
    feed::record(
        conn,
        &FeedChange {
            seq,
            kind: "lease".to_owned(),
            ticket: None,
            name: Some(name.to_owned()),
        },
    )
}

#[cfg(test)]
#[path = "leases_tests.rs"]
mod tests;
