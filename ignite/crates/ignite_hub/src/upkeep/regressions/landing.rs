//! A landing files its confirmed staging failure before the historical search, and tip
//! verification on a slow machine files its failure without searching. The pending search
//! stays with that check's open regression so a retry or a restart can finish it.
//!
//! The machine that reported the failure runs the search, unless a much faster machine could
//! (`super::searcher`). Then the search goes to a pool, and the fastest free machine claims it.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::protocol::hub::{
    FoundBy, LandingAttribution, LandingRegression, RegressionAttribution,
};
use ignite_core::protocol::tickets::{EdgeKind, Operation};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_store::hub::{edges, tickets, tips};
use ignite_store::{Result, StoreError};

use super::{Author, Outcome, apply, details, file_report, searcher, update_open};

#[derive(Deserialize, Serialize)]
struct Pending {
    /// The machine that reported the failure.
    machine: String,
    baseline: LandingRegression,
    /// Set when a faster machine runs the search. A search queued before pools existed has
    /// none and stays with its reporter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pool: Option<Pool>,
}

/// A search that the fastest free machine with `os` claims.
#[derive(Deserialize, Serialize)]
struct Pool {
    os: Os,
    queued_at: Timestamp,
    claim: Option<SearchClaim>,
}

#[derive(Deserialize, Serialize)]
struct SearchClaim {
    machine: String,
    until: Timestamp,
}

impl Pending {
    /// The machine whose report finishes the search: the claimant of a pooled search, else
    /// the reporter.
    fn searcher(&self) -> Option<&str> {
        match &self.pool {
            None => Some(&self.machine),
            Some(pool) => pool.claim.as_ref().map(|claim| claim.machine.as_str()),
        }
    }
}

/// How a failure's culprit search is queued.
#[derive(Clone, Copy)]
pub(super) struct Queued<'a> {
    pub baseline: &'a LandingRegression,
    /// The reporter already found a faster machine and did not search.
    pub handed_off: bool,
}

pub(crate) fn file(
    conn: &mut Connection,
    author: Author,
    baseline: &LandingRegression,
    now: Timestamp,
) -> Result<String> {
    let report = &baseline.report;
    if report.found_by != FoundBy::Landing
        || report.detected_at.as_ref() != Some(&report.first_bad)
        || report.search.is_some()
        || report.caused_by.is_some()
        || !report.suspects.is_empty()
    {
        return Err(StoreError::Rejected(
            "expected a confirmed baseline failure before historical attribution".to_owned(),
        ));
    }
    let queued = Queued {
        baseline,
        handed_off: false,
    };
    file_report(conn, author, report, Some(queued), now).map(|(ticket, _)| ticket)
}

/// Files a failure tip verification found on a machine too slow to search for its culprit.
/// The report says why the search did not run.
pub(crate) fn file_handoff(
    conn: &mut Connection,
    author: Author,
    baseline: &LandingRegression,
    now: Timestamp,
) -> Result<String> {
    let report = &baseline.report;
    if report.found_by != FoundBy::TipVerification
        || !matches!(report.search, Some(CulpritSearch::NotRun { .. }))
        || report.caused_by.is_some()
        || !report.suspects.is_empty()
    {
        return Err(StoreError::Rejected(
            "expected a tip failure whose culprit search did not run".to_owned(),
        ));
    }
    let queued = Queued {
        baseline,
        handed_off: true,
    };
    file_report(conn, author, report, Some(queued), now).map(|(ticket, _)| ticket)
}

/// Queues the search with the reporter `machine`, or in the pool when the reporter handed it
/// off or a much faster machine could run it.
pub(super) fn queue(
    conn: &Connection,
    ticket: &str,
    machine: &str,
    queued: Queued<'_>,
    now: Timestamp,
) -> Result<()> {
    let baseline = queued.baseline;
    let family = baseline.report.check.family();
    let pooled = queued.handed_off || searcher::placement(conn, machine, family, now)?.is_some();
    let pool = match searcher::os_of(conn, machine)? {
        Some(os) if pooled => Some(Pool {
            os,
            queued_at: now,
            claim: None,
        }),
        _ => None,
    };
    let pending = Pending {
        machine: machine.to_owned(),
        baseline: baseline.clone(),
        pool,
    };
    store(conn, baseline.report.check.as_str(), ticket, &pending)
}

fn store(conn: &Connection, check: &str, ticket: &str, pending: &Pending) -> Result<()> {
    let pending =
        serde_json::to_string(pending).map_err(|error| StoreError::Invalid(error.to_string()))?;
    conn.execute(
        "UPDATE regressions SET landing_attribution = ?3 WHERE check_name = ?1 AND ticket = ?2",
        params![check, ticket, pending],
    )?;
    Ok(())
}

/// The next search `machine` runs: one it reported and kept, one it claimed before, or a pooled
/// search it now claims (`searcher::takes`).
pub(crate) fn next(
    conn: &Connection,
    machine: &str,
    now: Timestamp,
) -> Result<Option<LandingAttribution>> {
    let mut statement = conn.prepare(
        "SELECT r.check_name, r.ticket, r.landing_attribution FROM regressions r
         JOIN tickets t ON t.id = r.ticket
         WHERE t.status != 'closed' AND r.state != ?1
           AND r.landing_attribution IS NOT NULL
         ORDER BY r.check_name",
    )?;
    let rows = statement
        .query_map(params![tips::FIXED], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<std::result::Result<Vec<(String, String, String)>, _>>()?;
    for (check, ticket, text) in rows {
        let mut pending = read_pending(&text)?;
        let runs_here = match &pending.pool {
            None => pending.machine == machine,
            Some(Pool {
                claim: Some(claim), ..
            }) if now < claim.until => claim.machine == machine,
            Some(pool) => searcher::takes(
                conn,
                machine,
                pool.os,
                pending.baseline.report.check.family(),
                pool.queued_at,
                now,
            )?,
        };
        if !runs_here {
            continue;
        }
        if let Some(pool) = &mut pending.pool {
            // Asking again renews the claim, as a search a full landing tree kept waiting does.
            pool.claim = Some(SearchClaim {
                machine: machine.to_owned(),
                until: now.plus_seconds(searcher::CLAIM_S),
            });
            store(conn, &check, &ticket, &pending)?;
        }
        return Ok(Some(LandingAttribution {
            ticket: ticket
                .parse()
                .map_err(|error| StoreError::Invalid(format!("ticket ID {ticket:?}: {error}")))?,
            baseline: pending.baseline,
        }));
    }
    Ok(None)
}

/// Enriches the original open regression. A stale search can neither reopen a fixed check nor
/// replace the attribution of a later failure.
pub(crate) fn finish(
    conn: &mut Connection,
    author: Author,
    attribution: &RegressionAttribution,
    now: Timestamp,
) -> Result<()> {
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let report = &attribution.report;
    let check = report.check.as_str();
    let id = attribution.ticket.as_str();
    let pending: Option<String> = tx
        .query_row(
            "SELECT r.landing_attribution FROM regressions r JOIN tickets t ON t.id = r.ticket
             WHERE r.check_name = ?1 AND r.ticket = ?2 AND r.state != ?3
               AND t.status != 'closed' AND r.landing_attribution IS NOT NULL",
            params![check, id, tips::FIXED],
            |row| row.get(0),
        )
        .optional()?;
    let Some(pending) = pending else {
        return Ok(());
    };
    let pending = read_pending(&pending)?;
    if pending.searcher() != Some(author.machine)
        || pending.baseline.report.detected_at != report.detected_at
        || pending.baseline.report.platform != report.platform
    {
        return Ok(());
    }
    if report.search.is_none() {
        return Err(StoreError::Rejected(
            "expected a historical search result".to_owned(),
        ));
    }
    let ticket = tickets::load(&tx, id)?.expect("the pending search has an open ticket");
    if update_open(&mut tx, &ticket, author, report, now)? == Outcome::Unchanged {
        // A range or a grouped check may have diagnostics without changing the ticket's
        // confirmed culprit or its dispatch affinity.
        let mut ops = vec![Operation::Comment {
            id: id.to_owned(),
            text: format!(
                "The culprit search for {check} finished.\n\n{}",
                details(report)
            ),
        }];
        let linked: std::collections::BTreeSet<_> =
            report.suspects.iter().chain(&report.caused_by).collect();
        for suspect in linked {
            if *suspect != attribution.ticket && edges::kind(&tx, id, suspect.as_str())?.is_none() {
                ops.push(Operation::DepAdd {
                    id: id.to_owned(),
                    on: suspect.to_string(),
                    kind: Some(EdgeKind::DiscoveredFrom),
                    external: false,
                });
            }
        }
        let now = ticket.claimed_at.map_or(now, |claimed| now.max(claimed));
        apply(&mut tx, author, check, ops, now)?;
    }
    tx.execute(
        "UPDATE regressions SET first_bad = ?2, last_good = ?3, landing_attribution = NULL
         WHERE check_name = ?1",
        params![check, report.first_bad.as_str(), report.last_good.as_str()],
    )?;
    tx.commit()?;
    Ok(())
}

fn read_pending(text: &str) -> Result<Pending> {
    serde_json::from_str(text).map_err(|error| StoreError::Invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use ignite_core::ids::Commit;
    use ignite_core::protocol::hub::RegressionReport;
    use ignite_core::protocol::tickets::Status;
    use ignite_core::tipround::CulpritSearch;
    use ignite_core::traits::Clock;

    use super::*;
    use crate::upkeep::filing::UPKEEP;
    use crate::upkeep::tests::{apply, start};

    fn baseline(tip: &str) -> LandingRegression {
        let tip: Commit = tip.parse().unwrap();
        LandingRegression {
            report: RegressionReport {
                check: "demo:check".parse().unwrap(),
                first_bad: tip.clone(),
                last_good: "0000".parse().unwrap(),
                caused_by: None,
                output: "failed".to_owned(),
                suspects: Vec::new(),
                platform: None,
                search: None,
                found_by: FoundBy::Landing,
                detected_at: Some(tip),
            },
            inputs: vec!["inputs.txt".to_owned()],
        }
    }

    fn result_for(pending: LandingAttribution) -> RegressionAttribution {
        let mut report = pending.baseline.report;
        report.search = Some(CulpritSearch::OneCandidate);
        RegressionAttribution {
            ticket: pending.ticket,
            report,
        }
    }

    #[test]
    fn a_search_updates_only_its_open_baseline_and_is_idempotent() {
        let now = start().now();
        let mut conn = ignite_store::hub::open_in_memory().unwrap();
        let id = file(&mut conn, UPKEEP, &baseline("1111"), now).unwrap();
        let old = result_for(next(&conn, UPKEEP.machine, now).unwrap().unwrap());
        assert!(next(&conn, "another-machine", now).unwrap().is_none());
        apply(
            &mut conn,
            vec![Operation::Close {
                id: id.clone(),
                reason: "fixed".to_owned(),
                include_ready: false,
            }],
            now.plus_seconds(1.0),
        );
        finish(&mut conn, UPKEEP, &old, now.plus_seconds(2.0)).unwrap();
        assert_eq!(
            tickets::load(&conn, &id).unwrap().unwrap().status,
            Status::Closed
        );

        let later = baseline("2222");
        let reopened = file(&mut conn, UPKEEP, &later, now.plus_seconds(3.0)).unwrap();
        assert_eq!(reopened, id);
        finish(&mut conn, UPKEEP, &old, now.plus_seconds(4.0)).unwrap();
        let pending = next(&conn, UPKEEP.machine, now).unwrap().unwrap();
        assert_eq!(pending.baseline, later);
        let current = result_for(pending);
        let before = tickets::comments(&conn, &id).unwrap().len();
        for seconds in [5.0, 6.0] {
            finish(&mut conn, UPKEEP, &current, now.plus_seconds(seconds)).unwrap();
        }
        let comments = tickets::comments(&conn, &id).unwrap();
        assert_eq!(comments.len(), before + 1);
        assert!(
            comments
                .iter()
                .any(|comment| comment.text.contains("first bad 2222"))
        );
        assert!(next(&conn, UPKEEP.machine, now).unwrap().is_none());
    }
}
