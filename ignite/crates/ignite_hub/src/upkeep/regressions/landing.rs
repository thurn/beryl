//! A landing files its confirmed staging failure before the historical search. The pending
//! search stays with that check's open regression so a retry or a restart can finish it.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use ignite_core::protocol::hub::{
    FoundBy, LandingAttribution, LandingRegression, RegressionAttribution,
};
use ignite_core::protocol::tickets::{EdgeKind, Operation};
use ignite_core::time::Timestamp;
use ignite_store::hub::{edges, tickets, tips};
use ignite_store::{Result, StoreError};

use super::{Author, Outcome, apply, details, file_report, update_open};

#[derive(Deserialize, Serialize)]
struct Pending {
    machine: String,
    baseline: LandingRegression,
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
    file_report(conn, author, report, Some(baseline), now).map(|(ticket, _)| ticket)
}

pub(super) fn queue(
    conn: &Connection,
    ticket: &str,
    machine: &str,
    baseline: &LandingRegression,
) -> Result<()> {
    let pending = serde_json::to_string(&Pending {
        machine: machine.to_owned(),
        baseline: baseline.clone(),
    })
    .map_err(|error| StoreError::Invalid(error.to_string()))?;
    conn.execute(
        "UPDATE regressions SET landing_attribution = ?3 WHERE check_name = ?1 AND ticket = ?2",
        params![baseline.report.check.as_str(), ticket, pending],
    )?;
    Ok(())
}

pub(crate) fn next(conn: &Connection, machine: &str) -> Result<Option<LandingAttribution>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT r.ticket, r.landing_attribution FROM regressions r
             JOIN tickets t ON t.id = r.ticket
             WHERE t.status != 'closed' AND r.state != ?1
               AND r.landing_attribution IS NOT NULL
               AND json_extract(r.landing_attribution, '$.machine') = ?2
             ORDER BY r.check_name LIMIT 1",
            params![tips::FIXED, machine],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(ticket, pending)| {
        Ok(LandingAttribution {
            ticket: ticket
                .parse()
                .map_err(|error| StoreError::Invalid(format!("ticket ID {ticket:?}: {error}")))?,
            baseline: read_pending(&pending)?.baseline,
        })
    })
    .transpose()
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
    if pending.machine != author.machine
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
        let old = result_for(next(&conn, UPKEEP.machine).unwrap().unwrap());
        assert!(next(&conn, "another-machine").unwrap().is_none());
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
        let pending = next(&conn, UPKEEP.machine).unwrap().unwrap();
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
        assert!(next(&conn, UPKEEP.machine).unwrap().is_none());
    }
}
