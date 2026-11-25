//! Reports a check that failed at consecutive tips without waiting for a round's culprit
//! search (see `ignite_store::hub::tip_streaks` for why rounds can fail to reach one).
//!
//! The report names the range from the last verified tip to the oldest tip of the streak, and
//! says that no search ran. A round that later reaches its search sends its own report, which
//! adds the culprit to the same ticket.

use rusqlite::Connection;

use ignite_core::protocol::hub::{FoundBy, RegressionReport, TipCheckFailures};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_store::Result;
use ignite_store::hub::tip_streaks::{self, Streak};
use ignite_store::hub::tips;

use super::super::filing::Author;
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
    let open = tips::open_regressions(conn)?;
    for streak in streaks {
        if open
            .iter()
            .any(|(check, _)| check == streak.key.check.as_str())
        {
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
