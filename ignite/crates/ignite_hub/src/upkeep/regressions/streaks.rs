//! Reports a check that failed at consecutive tips without waiting for a round's culprit
//! search (see `ignite_store::hub::tip_streaks` for why rounds can fail to reach one).
//!
//! The report names the range from the last verified tip to the oldest tip of the streak, and
//! says that no search ran. A round that later reaches its search sends its own report, which
//! adds the culprit to the same ticket.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::ids::Commit;
use ignite_core::protocol::hub::{FoundBy, RegressionReport, TipCheckFailures};
use ignite_core::protocol::tickets::LandedWork;
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_store::Result;
use ignite_store::hub::tip_streaks::{self, Streak};
use ignite_store::hub::tips;

use super::super::filing::Author;
use super::super::parse_time;
use super::file;

/// Files a regression for each check that `failures` shows failing at the last
/// [`tip_streaks::STREAK`] tips, unless its regression is already open. `author` is the
/// machine that sent the failures; a report is pinned to the machine that stored the failed
/// result when the cache still holds it.
pub fn file_streaks(
    conn: &mut Connection,
    author: Author,
    failures: &TipCheckFailures,
    now: Timestamp,
) -> Result<()> {
    let streaks = tip_streaks::streaks(conn, &failures.commit)?;
    if streaks.is_empty() {
        return Ok(());
    }
    let tip_seen = tip_seen(conn, &failures.commit)?;
    let open = tips::open_regressions(conn)?;
    for streak in streaks {
        if open
            .iter()
            .any(|(check, _)| check == streak.key.check.as_str())
        {
            continue;
        }
        if predates_closure(conn, streak.key.check.as_str(), &failures.commit, tip_seen)? {
            continue;
        }
        let machine = streak
            .output
            .as_ref()
            .map_or(author.machine, |(machine, _)| machine.as_str())
            .to_owned();
        let report = report(&failures.commit, &streak);
        file(
            conn,
            Author {
                actor: author.actor,
                machine: &machine,
            },
            &report,
            now,
        )?;
    }
    Ok(())
}

fn tip_seen(conn: &Connection, commit: &Commit) -> Result<Option<Timestamp>> {
    conn.query_row(
        "SELECT seen FROM tips WHERE commit_sha = ?1",
        params![commit.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|seen| parse_time(&seen))
    .transpose()
}

/// Whether `commit` (first seen at `tip_seen`) predates the closure of `check`'s existing
/// regression ticket. A round verifying an older tip can post its failures after a fix has
/// already closed the ticket; reopening on that older tip would falsely count the fix as failed.
fn predates_closure(
    conn: &Connection,
    check: &str,
    commit: &Commit,
    tip_seen: Option<Timestamp>,
) -> Result<bool> {
    let Some(tip_seen) = tip_seen else {
        return Ok(false);
    };
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT t.closed_at, t.landed
             FROM regressions r
             JOIN tickets t ON t.id = r.ticket
             WHERE r.check_name = ?1 AND t.status = 'closed' AND t.closed_at IS NOT NULL",
            params![check],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((closed_at, landed)) = row else {
        return Ok(false);
    };
    if tip_seen >= parse_time(&closed_at)? {
        return Ok(false);
    }
    // If the failing tip is one of the commits that landed as the fix itself, it does not
    // predate the fix even when the hub saw the pushed commit slightly before recording closure.
    if let Some(landed) = landed
        && let Ok(work) = serde_json::from_str::<LandedWork>(&landed)
        && work
            .commits
            .iter()
            .any(|landed_commit| landed_commit == commit)
    {
        return Ok(false);
    }
    Ok(true)
}

fn report(newest: &ignite_core::ids::Commit, streak: &Streak) -> RegressionReport {
    let output = match &streak.output {
        Some((machine, path)) => {
            format!("The failing run at {newest} saved its output on {machine} at {path}.")
        }
        None => String::new(),
    };
    RegressionReport {
        check: streak.key.check.clone(),
        first_bad: streak.first_bad.clone(),
        last_good: streak.last_good.clone(),
        caused_by: None,
        output,
        suspects: Vec::new(),
        platform: streak.key.platform,
        search: Some(CulpritSearch::NotRun {
            why: format!(
                "the check failed at {} tips in a row, through {newest}, and newer landings \
                 replaced each tip before its round could search",
                streak.tips
            ),
        }),
        found_by: FoundBy::TipVerification,
        detected_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::config::Os;
    use ignite_core::ids::{CheckName, InputHash, MachineId};
    use ignite_core::protocol::hub::{ResultKey, TipState, TipVerified};
    use ignite_core::protocol::tickets::{BatchRequest, Operation, Status};
    use ignite_store::hub::{self, batch, tickets};

    const AUTHOR: Author<'_> = Author {
        actor: "m-1",
        machine: "m-1",
    };

    fn at(seconds: f64) -> Timestamp {
        Timestamp::EPOCH.plus_seconds(seconds)
    }

    fn key(check: &str, hash: char) -> ResultKey {
        ResultKey {
            check: CheckName::new(check).unwrap(),
            hash: InputHash::new(hash.to_string().repeat(64)).unwrap(),
            platform: Some(Os::Macos),
        }
    }

    fn verified_tip(conn: &mut Connection, sha: &str, seconds: f64) {
        tips::seen(conn, sha, at(seconds)).unwrap();
        let verified = TipVerified {
            commit: Commit::new(sha).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            state: TipState::Verified,
            regression_passes: Some(Vec::new()),
            failure: None,
            failed_descendant: None,
        };
        tips::verified(conn, &verified, at(seconds)).unwrap();
    }

    fn fail_tip(
        conn: &mut Connection,
        sha: &str,
        seconds: f64,
        check: &str,
        hash: char,
    ) -> TipCheckFailures {
        tips::seen(conn, sha, at(seconds)).unwrap();
        let failures = TipCheckFailures {
            commit: Commit::new(sha).unwrap(),
            checks: vec![key(check, hash)],
        };
        tips::note_check_failures(conn, &failures).unwrap();
        failures
    }

    fn close_ticket(conn: &mut Connection, id: &str, seconds: f64, landed: Option<LandedWork>) {
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops: vec![Operation::Close {
                id: id.to_owned(),
                reason: "fixed".to_owned(),
                include_ready: false,
            }],
        };
        batch::apply(conn, &request, "worker", "m-1", at(seconds)).unwrap();
        if let Some(work) = landed {
            let json = serde_json::to_string(&work).unwrap();
            conn.execute(
                "UPDATE tickets SET landed = ?1 WHERE id = ?2",
                params![json, id],
            )
            .unwrap();
        }
    }

    #[test]
    fn stale_tip_failure_does_not_reopen_closed_regression() {
        let mut conn = hub::open_in_memory().unwrap();
        verified_tip(&mut conn, "aaaa", 1.0);
        fail_tip(&mut conn, "bbbb", 2.0, "x:a", 'b');
        let tip_c = fail_tip(&mut conn, "cccc", 3.0, "x:a", 'c');
        file_streaks(&mut conn, AUTHOR, &tip_c, at(4.0)).unwrap();

        let ticket_id: String = conn
            .query_row(
                "SELECT ticket FROM regressions WHERE check_name = 'x:a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        close_ticket(
            &mut conn,
            &ticket_id,
            10.0,
            Some(LandedWork {
                commits: vec![Commit::new("dddd").unwrap()],
                files: vec!["a.rs".to_owned()],
                note: "fix".to_owned(),
            }),
        );

        // Another machine reports the older tip `cccc` (seen at 3.0 < closed_at 10.0) after the fix.
        file_streaks(&mut conn, AUTHOR, &tip_c, at(12.0)).unwrap();
        let loaded = tickets::load(&conn, &ticket_id).unwrap().unwrap();
        assert_eq!(loaded.status, Status::Closed);

        // A failure at a tip seen after closure reopens the regression.
        let tip_e = fail_tip(&mut conn, "eeee", 15.0, "x:a", 'e');
        file_streaks(&mut conn, AUTHOR, &tip_e, at(16.0)).unwrap();
        let loaded = tickets::load(&conn, &ticket_id).unwrap().unwrap();
        assert_eq!(loaded.status, Status::Open);
    }

    #[test]
    fn failure_on_the_landed_fix_commit_reopens_even_if_seen_before_close() {
        let mut conn = hub::open_in_memory().unwrap();
        verified_tip(&mut conn, "aaaa", 1.0);
        fail_tip(&mut conn, "bbbb", 2.0, "x:a", 'b');
        let tip_c = fail_tip(&mut conn, "cccc", 3.0, "x:a", 'c');
        file_streaks(&mut conn, AUTHOR, &tip_c, at(4.0)).unwrap();

        let ticket_id: String = conn
            .query_row(
                "SELECT ticket FROM regressions WHERE check_name = 'x:a'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // The fix commit `dddd` is seen slightly before closure is recorded.
        let tip_d = fail_tip(&mut conn, "dddd", 9.0, "x:a", 'd');
        close_ticket(
            &mut conn,
            &ticket_id,
            10.0,
            Some(LandedWork {
                commits: vec![Commit::new("dddd").unwrap()],
                files: vec!["a.rs".to_owned()],
                note: "fix".to_owned(),
            }),
        );

        file_streaks(&mut conn, AUTHOR, &tip_d, at(12.0)).unwrap();
        let loaded = tickets::load(&conn, &ticket_id).unwrap().unwrap();
        assert_eq!(loaded.status, Status::Open);
    }
}
