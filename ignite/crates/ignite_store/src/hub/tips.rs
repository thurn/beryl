//! Tips: each new `staging` commit, its verification claims and its state, regressions, and
//! which ticket landed each commit a culprit search looks at.
//!
//! A claim names one batch of one family's checks at one tip. It expires, so a machine that
//! stops (or is shut down) does not hold a batch forever, and another machine can take it.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use ignite_core::config::Os;
use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::hub::{
    LandedCommit, RegressionReport, TipCheckFailures, TipClaim, TipClaimReply, TipState,
    TipVerified,
};
use ignite_core::time::Timestamp;
use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};

use crate::hub::{meta, results::parse};
use crate::{Result, StoreError};

const CANDIDATE_KEY: &str = "tip_candidate";

/// How long a claim holds its batch.
pub const CLAIM_SECONDS: f64 = 900.0;

/// A regression's states: `open` when filed, `fixed` once the check passes at a later tip, and
/// `reopened` when it fails again after that.
pub const OPEN: &str = "open";
pub const FIXED: &str = "fixed";
pub const REOPENED: &str = "reopened";

fn tip_state_text(state: TipState) -> &'static str {
    match state {
        TipState::Seen => "seen",
        TipState::Verifying => "verifying",
        TipState::Verified => "verified",
        TipState::Failed => "failed",
    }
}

/// Records a new tip as seen. A tip already known keeps its state and its first-seen time.
pub fn seen(conn: &Connection, commit: &str, now: Timestamp) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO tips (commit_sha, seen, verified, state) VALUES (?1, ?2, NULL, 'seen')",
        params![commit, now.to_string()],
    )?;
    Ok(())
}

pub fn candidate(conn: &Connection) -> Result<Option<TipCandidate>> {
    meta::get(conn, CANDIDATE_KEY)
}

/// One machine's selection wins even when two machines fetch different staging tips.
pub fn update_candidate(
    conn: &mut Connection,
    update: &TipCandidateUpdate,
    now: Timestamp,
) -> Result<bool> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if candidate(&transaction)? != update.expected {
        return Ok(false);
    }
    seen(&transaction, update.replacement.commit.as_str(), now)?;
    meta::put(&transaction, CANDIDATE_KEY, &update.replacement)?;
    transaction.commit()?;
    Ok(true)
}

/// Records failed required result keys. If this is the active candidate, it can give way to
/// newer staging. A batch that finishes after replacement still belongs to its original tip.
pub fn note_check_failures(conn: &mut Connection, report: &TipCheckFailures) -> Result<()> {
    if report.checks.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let checks = serde_json::to_string(&report.checks)
        .map_err(|error| StoreError::Invalid(format!("observed tip failures: {error}")))?;
    let recorded = tx.execute(
        "UPDATE tips SET observed_failures = (SELECT json_group_array(json(value)) FROM (
             SELECT value FROM json_each(observed_failures)
             UNION SELECT value FROM json_each(?2)
         ))
         WHERE commit_sha = ?1 AND state != 'verified'",
        params![report.commit.as_str(), checks],
    )?;
    if recorded != 0
        && let Some(mut selected) =
            candidate(&tx)?.filter(|tip| tip.commit == report.commit && !tip.retry_on_new_tip)
    {
        selected.retry_on_new_tip = true;
        meta::put(&tx, CANDIDATE_KEY, &selected)?;
    }
    tx.commit()?;
    Ok(())
}

/// Reports failures observed at the selected candidate without changing its verification state.
pub fn candidate_failures(conn: &Connection) -> Result<Option<TipCheckFailures>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT t.commit_sha, t.observed_failures
             FROM tips t JOIN meta m ON t.commit_sha = json_extract(m.value, '$.commit')
             WHERE m.key = ?1 AND t.observed_failures IS NOT NULL
               AND t.state IN ('seen', 'verifying', 'failed')",
            [CANDIDATE_KEY],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(commit, checks)| {
        let checks = serde_json::from_str(&checks)
            .map_err(|error| StoreError::Invalid(format!("observed tip failures: {error}")))?;
        Ok(TipCheckFailures {
            commit: parse("tip", &commit)?,
            checks,
        })
    })
    .transpose()
}

/// The latest failed required checks in this lag episode, for context while a newer
/// candidate collects its own results. The boolean says whether the round recorded a verdict.
pub fn latest_failure(
    conn: &Connection,
    since: Timestamp,
) -> Result<Option<(TipCheckFailures, bool)>> {
    let row: Option<(String, String, bool)> = conn
        .query_row(
            "SELECT commit_sha, coalesce(json_extract(failure, '$.checks'), observed_failures),
                    failure IS NOT NULL
             FROM tips
             WHERE coalesce(verified, seen) >= ?1
               AND (failure IS NOT NULL OR observed_failures IS NOT NULL)
             ORDER BY coalesce(verified, seen) DESC, commit_sha DESC LIMIT 1",
            [since.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(commit, checks, completed)| {
        let checks = serde_json::from_str(&checks)
            .map_err(|error| StoreError::Invalid(format!("stored tip failures: {error}")))?;
        Ok((
            TipCheckFailures {
                commit: parse("tip", &commit)?,
                checks,
            },
            completed,
        ))
    })
    .transpose()
}

/// Grants the claim if no unexpired claim by another machine holds the same batch. A machine
/// that claims its own batch again has it renewed. A claim also marks a tip that was only seen
/// as `verifying`.
pub fn claim(conn: &Connection, claim: &TipClaim, now: Timestamp) -> Result<TipClaimReply> {
    let held: Option<(String, String)> = conn
        .query_row(
            "SELECT machine, expires_at FROM tip_claims
             WHERE commit_sha = ?1 AND family = ?2 AND batch = ?3",
            params![claim.commit.as_str(), claim.family, claim.batch],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((machine, expires_at)) = held {
        let expires: Timestamp = parse("time", &expires_at)?;
        if expires > now && machine != claim.machine.as_str() {
            return Ok(TipClaimReply {
                granted: false,
                expires_at: Some(expires),
            });
        }
    }
    let expires = now.plus_seconds(CLAIM_SECONDS);
    conn.execute(
        "INSERT OR REPLACE INTO tip_claims (commit_sha, family, batch, machine, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            claim.commit.as_str(),
            claim.family,
            claim.batch,
            claim.machine.as_str(),
            expires.to_string()
        ],
    )?;
    seen(conn, claim.commit.as_str(), now)?;
    conn.execute(
        "UPDATE tips SET state = 'verifying' WHERE commit_sha = ?1 AND state = 'seen'",
        params![claim.commit.as_str()],
    )?;
    Ok(TipClaimReply {
        granted: true,
        expires_at: Some(expires),
    })
}

/// Sets a tip's state. `verified` and `failed` also record when verification finished, and
/// mark every older tip still `seen` or `verifying` as `superseded`: tips in between are never
/// verified one by one, and leaving them `verifying` showed stale work on the dashboard and
/// made the hub's upkeep blame silent machines for them.
pub fn verified(conn: &Connection, verified: &TipVerified, now: Timestamp) -> Result<()> {
    seen(conn, verified.commit.as_str(), now)?;
    let finished = matches!(verified.state, TipState::Verified | TipState::Failed);
    let passes = verified
        .regression_passes
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    let failure = verified
        .failure
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StoreError::Invalid(format!("a tip's failed checks: {error}")))?;
    conn.execute(
        "UPDATE tips SET state = ?2, verified = CASE WHEN ?3 THEN ?4 ELSE NULL END,
         regression_passes = CASE
           WHEN ?2 != 'verified' THEN NULL
           WHEN ?5 IS NULL THEN regression_passes
           WHEN ?5 = '[]' AND state = 'verified' AND regression_passes IS NULL THEN NULL
           ELSE (SELECT json_group_array(json(value)) FROM (
             SELECT value FROM json_each(regression_passes)
             UNION SELECT value FROM json_each(?5)
           ))
         END,
         failure = CASE WHEN ?2 = 'failed' THEN coalesce(?6, failure) END,
         observed_failures = CASE WHEN ?2 != 'verified' THEN observed_failures END
         WHERE commit_sha = ?1",
        params![
            verified.commit.as_str(),
            tip_state_text(verified.state),
            finished,
            now.to_string(),
            passes,
            failure,
        ],
    )?;
    if finished {
        conn.execute(
            "UPDATE tips SET state = 'superseded'
             WHERE state IN ('seen', 'verifying')
               AND seen < (SELECT seen FROM tips WHERE commit_sha = ?1)",
            params![verified.commit.as_str()],
        )?;
    }
    Ok(())
}

/// Records a regression against the ticket filed for it. A check with no regression gets an
/// open one. A fixed regression that fails again is reopened under its old ticket, with the new
/// first bad commit and last good commit. One already open or reopened is left as it is.
pub fn regression(conn: &Connection, report: &RegressionReport, ticket: &str) -> Result<()> {
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM regressions WHERE check_name = ?1",
            params![report.check.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    match state.as_deref() {
        None => {
            conn.execute(
                "INSERT INTO regressions
                    (check_name, ticket, first_bad, last_good, state, platform, culprit_ticket, failure_output)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    report.check.as_str(),
                    ticket,
                    report.first_bad.as_str(),
                    report.last_good.as_str(),
                    OPEN,
                    report.platform.map(Os::as_str).unwrap_or_default(),
                    report.caused_by.as_ref().map(TicketId::as_str),
                    report.output,
                ],
            )?;
        }
        Some(FIXED) => {
            conn.execute(
                "UPDATE regressions SET state = ?2, first_bad = ?3, last_good = ?4, platform = ?5,
                    culprit_ticket = ?6, failure_output = ?7
                 WHERE check_name = ?1",
                params![
                    report.check.as_str(),
                    REOPENED,
                    report.first_bad.as_str(),
                    report.last_good.as_str(),
                    report.platform.map(Os::as_str).unwrap_or_default(),
                    report.caused_by.as_ref().map(TicketId::as_str),
                    report.output,
                ],
            )?;
        }
        Some(OPEN | REOPENED) => {}
        Some(other) => {
            return Err(StoreError::Invalid(format!(
                "stored regression state {other:?}"
            )));
        }
    }
    Ok(())
}

/// Marks a check's regression fixed, after the check passes at a later tip.
pub fn regression_fixed(conn: &Connection, check: &str) -> Result<()> {
    conn.execute(
        "UPDATE regressions SET state = ?2 WHERE check_name = ?1 AND state != ?2",
        params![check, FIXED],
    )?;
    Ok(())
}

/// Each check whose regression is not fixed, with its ticket, for the tickets that are not
/// closed. A check the hub added to another check's ticket is listed with that ticket.
pub fn open_regressions(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut statement = conn.prepare(
        "SELECT r.check_name, r.ticket FROM regressions r JOIN tickets t ON t.id = r.ticket
         WHERE t.status != 'closed' AND r.state != ?1 ORDER BY r.check_name",
    )?;
    let rows = statement
        .query_map(params![FIXED], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The ticket whose landing recorded each of `commits`, in the order asked, leaving out commits
/// no landing recorded. A commit recorded by two landings belongs to the later one.
pub fn landed_commits(conn: &Connection, commits: &[Commit]) -> Result<Vec<LandedCommit>> {
    let wanted: Vec<&str> = commits.iter().map(Commit::as_str).collect();
    let wanted = serde_json::to_string(&wanted).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let mut statement = conn.prepare(
        "SELECT landed_commit.value, tickets.id
         FROM tickets, json_each(tickets.landed, '$.commits') AS landed_commit
         WHERE tickets.landed IS NOT NULL
           AND landed_commit.value IN (SELECT value FROM json_each(?1))
         ORDER BY tickets.landed_at, tickets.id",
    )?;
    let mut owner: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for row in statement.query_map(params![wanted], |row| Ok((row.get(0)?, row.get(1)?)))? {
        let (commit, ticket): (String, String) = row?;
        owner.insert(commit, ticket);
    }
    commits
        .iter()
        .filter_map(|commit| {
            owner.get(commit.as_str()).map(|ticket| {
                Ok(LandedCommit {
                    commit: commit.clone(),
                    ticket: parse("ticket", ticket)?,
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;
    use ignite_core::ids::{CheckName, Commit, MachineId, TicketId};

    fn at(seconds: f64) -> Timestamp {
        Timestamp::from_millis(0).plus_seconds(seconds)
    }

    fn claim_by(machine: &str, batch: u32) -> TipClaim {
        TipClaim {
            commit: Commit::new("abcd").unwrap(),
            family: "rust".to_owned(),
            batch,
            machine: MachineId::new(machine).unwrap(),
        }
    }

    fn tip_state(conn: &Connection) -> (String, Option<String>) {
        conn.query_row(
            "SELECT state, verified FROM tips WHERE commit_sha = 'abcd'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    fn report(first_bad: &str) -> RegressionReport {
        RegressionReport {
            check: CheckName::new("rust:a").unwrap(),
            first_bad: Commit::new(first_bad).unwrap(),
            last_good: Commit::new("0000").unwrap(),
            caused_by: Some(TicketId::new("t.1").unwrap()),
            output: "boom".to_owned(),
            suspects: Vec::new(),
            platform: None,
            search: None,
            found_by: ignite_core::protocol::hub::FoundBy::TipVerification,
            detected_at: None,
        }
    }

    fn regression_row(conn: &Connection) -> (String, String, String) {
        conn.query_row(
            "SELECT ticket, first_bad, state FROM regressions WHERE check_name = 'rust:a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    }

    fn land(conn: &Connection, ticket: &str, at: &str, commits: &[&str]) {
        conn.execute(
            "INSERT INTO tickets (id, title, status, created_at, created_by, updated_at, version,
                                  landed_at, landed)
             VALUES (?1, ?1, 'closed', ?2, 't', ?2, 0, ?2, ?3)",
            params![
                ticket,
                at,
                serde_json::json!({ "commits": commits, "files": [], "note": "" }).to_string()
            ],
        )
        .unwrap();
    }

    #[test]
    fn each_commit_is_named_by_the_ticket_that_landed_it() {
        let conn = hub::open_in_memory().unwrap();
        land(&conn, "t.1", "2026-11-01T00:00:00.000Z", &["aaaa", "bbbb"]);
        land(&conn, "t.2", "2026-11-02T00:00:00.000Z", &["cccc"]);
        // A commit landed again by a later ticket belongs to the later one.
        land(&conn, "t.3", "2026-11-03T00:00:00.000Z", &["bbbb"]);
        let asked: Vec<Commit> = ["cccc", "dddd", "aaaa", "bbbb"]
            .iter()
            .map(|c| Commit::new(*c).unwrap())
            .collect();

        let found: Vec<(String, String)> = landed_commits(&conn, &asked)
            .unwrap()
            .into_iter()
            .map(|l| (l.commit.to_string(), l.ticket.to_string()))
            .collect();

        let expected = [("cccc", "t.2"), ("aaaa", "t.1"), ("bbbb", "t.3")];
        assert_eq!(
            found,
            expected.map(|(c, t)| (c.to_owned(), t.to_owned())).to_vec()
        );
    }

    #[test]
    fn a_claim_excludes_other_machines_until_it_expires_and_then_is_taken() {
        let conn = hub::open_in_memory().unwrap();
        let first = claim(&conn, &claim_by("m-1", 0), at(0.0)).unwrap();
        assert!(first.granted);
        assert_eq!(first.expires_at, Some(at(CLAIM_SECONDS)));

        // Another machine is refused, and told when the claim ends; other batches and
        // families are separate claims.
        let refused = claim(&conn, &claim_by("m-2", 0), at(CLAIM_SECONDS - 1.0)).unwrap();
        assert!(!refused.granted);
        assert_eq!(refused.expires_at, Some(at(CLAIM_SECONDS)));
        assert!(claim(&conn, &claim_by("m-2", 1), at(1.0)).unwrap().granted);
        let mut other_family = claim_by("m-2", 0);
        other_family.family = "ui".to_owned();
        assert!(claim(&conn, &other_family, at(1.0)).unwrap().granted);

        // After the expiry the other machine takes it, and now the first is refused.
        let taken = claim(&conn, &claim_by("m-2", 0), at(CLAIM_SECONDS)).unwrap();
        assert!(taken.granted);
        assert_eq!(taken.expires_at, Some(at(2.0 * CLAIM_SECONDS)));
        assert!(
            !claim(&conn, &claim_by("m-1", 0), at(CLAIM_SECONDS + 1.0))
                .unwrap()
                .granted
        );
    }

    #[test]
    fn a_machine_renews_its_own_claim() {
        let conn = hub::open_in_memory().unwrap();
        claim(&conn, &claim_by("m-1", 0), at(0.0)).unwrap();
        let renewed = claim(&conn, &claim_by("m-1", 0), at(100.0)).unwrap();
        assert!(renewed.granted);
        assert_eq!(renewed.expires_at, Some(at(100.0 + CLAIM_SECONDS)));
    }

    #[test]
    fn a_stale_round_cannot_replace_the_current_candidate() {
        let mut conn = hub::open_in_memory().unwrap();
        let first = TipCandidate {
            commit: Commit::new("aaaa").unwrap(),
            retry_on_new_tip: false,
        };
        let newer = TipCandidate {
            commit: Commit::new("bbbb").unwrap(),
            retry_on_new_tip: false,
        };
        let select = |expected, replacement| TipCandidateUpdate {
            expected,
            replacement,
        };
        assert!(update_candidate(&mut conn, &select(None, first.clone()), at(1.0)).unwrap());
        assert!(!update_candidate(&mut conn, &select(None, newer.clone()), at(2.0)).unwrap());
        assert!(
            update_candidate(
                &mut conn,
                &select(Some(first.clone()), newer.clone()),
                at(3.0)
            )
            .unwrap()
        );
        assert!(
            !update_candidate(
                &mut conn,
                &select(
                    Some(first.clone()),
                    TipCandidate {
                        retry_on_new_tip: true,
                        ..first
                    }
                ),
                at(4.0),
            )
            .unwrap()
        );
        assert_eq!(candidate(&conn).unwrap(), Some(newer));
    }

    /// Tips 49a3e3b and 77fe5b6 stayed `verifying` for hours after a newer tip reached
    /// `master`. A finished tip now marks every older unfinished one `superseded`.
    #[test]
    fn a_finished_tip_supersedes_older_unfinished_ones() {
        let conn = hub::open_in_memory().unwrap();
        seen(&conn, "old1", at(10.0)).unwrap();
        seen(&conn, "old2", at(20.0)).unwrap();
        seen(&conn, "abcd", at(30.0)).unwrap();
        seen(&conn, "newer", at(40.0)).unwrap();
        let done = TipVerified {
            commit: Commit::new("abcd").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Verified,
            regression_passes: None,
            failure: None,
        };
        verified(&conn, &done, at(50.0)).unwrap();
        let state = |commit: &str| -> String {
            conn.query_row(
                "SELECT state FROM tips WHERE commit_sha = ?1",
                params![commit],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(state("old1"), "superseded");
        assert_eq!(state("old2"), "superseded");
        assert_eq!(state("abcd"), "verified");
        assert_eq!(state("newer"), "seen");
    }

    #[test]
    fn a_tip_goes_from_seen_to_verifying_to_verified() {
        let conn = hub::open_in_memory().unwrap();
        seen(&conn, "abcd", at(10.0)).unwrap();
        assert_eq!(tip_state(&conn), ("seen".to_owned(), None));
        claim(&conn, &claim_by("m-1", 0), at(20.0)).unwrap();
        assert_eq!(tip_state(&conn).0, "verifying");
        // Seeing it again changes nothing.
        seen(&conn, "abcd", at(30.0)).unwrap();
        assert_eq!(tip_state(&conn).0, "verifying");
        let done = TipVerified {
            commit: Commit::new("abcd").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Verified,
            regression_passes: None,
            failure: None,
        };
        verified(&conn, &done, at(40.0)).unwrap();
        assert_eq!(
            tip_state(&conn),
            ("verified".to_owned(), Some(at(40.0).to_string()))
        );
        let seen_at: String = conn
            .query_row("SELECT seen FROM tips", [], |row| row.get(0))
            .unwrap();
        assert_eq!(seen_at, at(10.0).to_string());
    }

    #[test]
    fn a_regression_opens_is_fixed_and_reopens_under_the_same_ticket() {
        let conn = hub::open_in_memory().unwrap();
        regression(&conn, &report("1111"), "t.9").unwrap();
        assert_eq!(
            regression_row(&conn),
            ("t.9".to_owned(), "1111".to_owned(), "open".to_owned())
        );

        // Reported again while open: nothing changes, and no second ticket takes over.
        regression(&conn, &report("2222"), "t.10").unwrap();
        assert_eq!(
            regression_row(&conn),
            ("t.9".to_owned(), "1111".to_owned(), "open".to_owned())
        );

        regression_fixed(&conn, "rust:a").unwrap();
        assert_eq!(regression_row(&conn).2, "fixed");

        regression(&conn, &report("3333"), "t.11").unwrap();
        assert_eq!(
            regression_row(&conn),
            ("t.9".to_owned(), "3333".to_owned(), "reopened".to_owned())
        );
    }
}
