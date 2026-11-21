//! Regression tickets use the newest verified tip to determine whether every check has passed.
//! Unlike faults that can appear and disappear between sightings, a verified fix needs no
//! waiting period. The same decision governs upkeep, dispatch and releases.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

use ignite_core::config::Os;
use ignite_core::ids::Commit;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::protocol::tickets::{BatchRequest, Operation, Status, Ticket};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{fleet, tickets, tip_coverage, tips};
use ignite_store::{Result, StoreError};

use super::super::clearing::{CLOSE_PREFIX, NOTED_KEY, meta};
use super::super::filing::{CLEARED_SINCE_KEY, UPKEEP};
use super::ADDED_KEY;

/// Which claim and set of failing checks have already been told the fix passed.
const TOLD_KEY: &str = "ignite_regression_clear_noted";

struct Check {
    name: String,
    platform: Option<String>,
    off: bool,
}

/// Proof that the active checks on a regression ticket have passed.
pub struct Passing {
    tip: String,
    verified: String,
    since: Timestamp,
    checks: Vec<Check>,
    passes: Option<Vec<ResultKey>>,
}

fn since(conn: &Connection, ticket: &Ticket) -> Result<Timestamp> {
    let since = ticket
        .metadata
        .get(ADDED_KEY)
        .and_then(Value::as_str)
        .and_then(|at| Timestamp::parse(at).ok())
        .map_or(ticket.created_at, |at| at.max(ticket.created_at));
    Ok(tickets::history(conn, ticket.id.as_str())?
        .iter()
        .filter(|row| row.change.get("status") == Some(&json!([Status::Closed, Status::Open])))
        .map(|row| row.at)
        .fold(since, Timestamp::max))
}

fn matching_pass<'a>(check: &Check, passes: &'a [ResultKey]) -> Option<&'a ResultKey> {
    passes.iter().find(|key| {
        key.check.as_str() == check.name
            && check
                .platform
                .as_deref()
                .is_none_or(|platform| platform == key.platform.map(Os::as_str).unwrap_or_default())
    })
}

/// A check switched off by the hub needs no pass. For older tips whose daemon did not send
/// individual results, retain the verified-tip inference, except for platforms with deferred
/// verification.
pub fn passing(conn: &Connection, ticket: &Ticket) -> Result<Option<Passing>> {
    let mut rows = conn.prepare(
        "SELECT r.check_name, r.platform, coalesce(c.state = 'off', false)
         FROM regressions r LEFT JOIN checks c ON c.name = r.check_name
         WHERE r.ticket = ?1 AND r.state != ?2 ORDER BY r.check_name",
    )?;
    let mut checks = rows
        .query_map(params![ticket.id.as_str(), tips::FIXED], |row| {
            Ok(Check {
                name: row.get(0)?,
                platform: row.get(1)?,
                off: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if checks.is_empty() {
        return Ok(None);
    }
    let since = since(conn, ticket)?;
    let tip: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT commit_sha, verified, regression_passes FROM tips
         WHERE state = 'verified' AND seen > ?1 ORDER BY seen DESC, commit_sha DESC LIMIT 1",
            [since.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((tip, verified, passes)) = tip else {
        return Ok(None);
    };
    let passes = passes
        .map(|raw| serde_json::from_str::<Vec<ResultKey>>(&raw))
        .transpose()
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    // Before per-check platforms were recorded, the ticket retained the reporting platform.
    if let Some(os) = ticket.needs_os {
        for check in &mut checks {
            check.platform.get_or_insert_with(|| os.as_str().to_owned());
        }
    }
    let all_pass = if let Some(passes) = &passes {
        checks
            .iter()
            .all(|check| check.off || matching_pass(check, passes).is_some())
    } else {
        let coverage = tip_coverage::get(conn)?;
        checks.iter().all(|check| {
            check.off
                || !coverage.platforms.iter().any(|(os, status)| {
                    status.deferred.is_some()
                        && check
                            .platform
                            .as_deref()
                            .is_none_or(|platform| platform == os.as_str())
                })
        })
    };
    Ok(all_pass.then_some(Passing {
        tip,
        verified,
        since,
        checks,
        passes,
    }))
}

impl Passing {
    fn reason(&self, conn: &Connection) -> Result<String> {
        let names: Vec<_> = self
            .checks
            .iter()
            .map(|check| check.name.as_str())
            .collect();
        let mut reason = format!(
            "all regression checks passed at verified tip {} ({}).",
            self.tip,
            names.join(", ")
        );
        for check in &self.checks {
            if check.off {
                let _ = write!(
                    reason,
                    " {} counts as passing because it is switched off.",
                    check.name
                );
            } else if let Some(key) = self
                .passes
                .as_deref()
                .and_then(|passes| matching_pass(check, passes))
                && let Some(landing) = self.landing(conn, key)?
            {
                let _ = write!(
                    reason,
                    " {} has a passing result from landing {landing}.",
                    check.name
                );
            }
        }
        Ok(reason)
    }

    fn landing(&self, conn: &Connection, key: &ResultKey) -> Result<Option<String>> {
        let commit: Option<String> = conn
            .query_row(
                "SELECT r.commit_sha FROM results p JOIN records r ON r.id = p.record
             WHERE p.check_name = ?1 AND p.hash = ?2 AND p.platform = ?3
               AND p.outcome = 'pass' AND p.at >= ?4 AND p.at <= ?5
               AND r.commit_sha IS NOT NULL",
                params![
                    key.check.as_str(),
                    key.hash.as_str(),
                    key.platform.map(Os::as_str).unwrap_or_default(),
                    self.since.to_string(),
                    self.verified
                ],
                |row| row.get(0),
            )
            .optional()?;
        let Some(commit) = commit else {
            return Ok(None);
        };
        let commit = commit
            .parse::<Commit>()
            .map_err(|error| StoreError::Invalid(format!("commit {commit}: {error}")))?;
        Ok(tips::landed_commits(conn, &[commit])?
            .first()
            .map(|landing| format!("{} ({})", landing.ticket, landing.commit)))
    }

    /// Close an unheld ticket (including one just released) and mark its checks fixed in the
    /// same transaction. A queued reservation is dropped with the ticket.
    pub fn close(self, tx: &mut Transaction<'_>, ticket: &Ticket, now: Timestamp) -> Result<()> {
        let reason = self.reason(tx)?;
        let id = ticket.id.as_str();
        apply(
            tx,
            vec![
                meta(
                    id,
                    BTreeMap::new(),
                    &[CLEARED_SINCE_KEY, NOTED_KEY, TOLD_KEY],
                ),
                Operation::Close {
                    id: id.to_owned(),
                    reason: format!("{CLOSE_PREFIX} {reason}"),
                    include_ready: false,
                },
            ],
            now,
        )?;
        self.mark_fixed(tx)?;
        fleet::release_reservation(tx, id)?;
        Ok(())
    }

    /// Records each check as fixed when its holder closes the ticket after verification.
    pub fn mark_fixed(self, conn: &Connection) -> Result<()> {
        for check in self.checks {
            tips::regression_fixed(conn, &check.name)?;
        }
        Ok(())
    }

    fn tell(&self, tx: &mut Transaction<'_>, ticket: &Ticket, now: Timestamp) -> Result<()> {
        let claim = json!([ticket.assignee, ticket.claimed_at, self.since]);
        if ticket.metadata.get(TOLD_KEY) == Some(&claim) {
            return Ok(());
        }
        let id = ticket.id.as_str();
        let last: u32 = tx.query_row(
            "SELECT coalesce(max(n), 0) FROM comments WHERE ticket = ?1",
            [id],
            |row| row.get(0),
        )?;
        let reason = self.reason(tx)?;
        apply(
            tx,
            vec![
                Operation::Comment {
                    id: id.to_owned(),
                    text: format!(
                        "Ignite verified that {reason} Land your work if it is still needed, \
                              or run `ignite close`."
                    ),
                },
                meta(
                    id,
                    BTreeMap::from([(TOLD_KEY, claim), ("tell", json!(last + 1))]),
                    &[],
                ),
            ],
            now,
        )
    }
}

fn apply(tx: &mut Transaction<'_>, ops: Vec<Operation>, now: Timestamp) -> Result<()> {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply_in(tx, &request, UPKEEP.actor, UPKEEP.machine, now)? {
        BatchOutcome::Applied(_) => Ok(()),
        BatchOutcome::Conflict(conflict) => Err(StoreError::Rejected(format!(
            "settling a regression conflicted at operation {}",
            conflict.failed
        ))),
    }
}

/// Tells holders of verified regressions and closes those that nobody holds. Rechecks the
/// verdict under the same write lock as the changes to avoid racing a claim or another check
/// being added to the ticket.
pub fn settle(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    let ids: BTreeSet<_> = tips::open_regressions(conn)?
        .into_iter()
        .map(|(_, id)| id)
        .collect();
    let mut closed = Vec::new();
    for id in ids {
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(ticket) =
            tickets::load(&tx, &id)?.filter(|ticket| ticket.status != Status::Closed)
        else {
            continue;
        };
        let Some(proof) = passing(&tx, &ticket)? else {
            continue;
        };
        if ticket.assignee.is_some() {
            proof.tell(&mut tx, &ticket, now)?;
        } else {
            proof.close(&mut tx, &ticket, now)?;
            closed.push(id);
        }
        tx.commit()?;
    }
    Ok(closed)
}
