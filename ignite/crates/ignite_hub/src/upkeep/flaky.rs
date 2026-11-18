//! Flaky checks: a check with a second flaky record within 7 days of the first, on a different
//! commit, is switched off on every machine, and one P3 ticket asks for it to be deleted or
//! replaced with a deterministic test.

use rusqlite::{Connection, params};

use ignite_core::protocol::hub::CheckState;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::checks::{self, FLAKY_WINDOW_DAYS};

use super::filing::{self, Filed, Filing, UPKEEP};
use super::parse_time;
use super::signature::Signature;

/// One flaky record: the check failed and then passed on the same inputs.
struct FlakyRecord {
    hash: String,
    commit: String,
    machine: String,
    at: Timestamp,
}

/// Switches off each check that is on and has two flaky records that count, and files its
/// ticket. Returns the checks switched off and the tickets.
pub fn switch_off_repeated(conn: &mut Connection, now: Timestamp) -> Result<Vec<(String, Filed)>> {
    let candidates: Vec<String> = {
        let mut select = conn.prepare(
            "SELECT DISTINCT f.check_name FROM flaky f JOIN checks c ON c.name = f.check_name
             WHERE c.state != 'off' ORDER BY f.check_name",
        )?;
        select
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut switched = Vec::new();
    for check in candidates {
        if !checks::flaky_repeated(conn, &check)? {
            continue;
        }
        let records = records(conn, &check)?;
        let Some((first, second)) = repeated_pair(&records) else {
            continue;
        };
        checks::set_state(conn, &check, CheckState::Off)?;
        let filed = filing::file(conn, UPKEEP, &filing(&check, first, second), now)?;
        switched.push((check, filed));
    }
    Ok(switched)
}

fn records(conn: &Connection, check: &str) -> Result<Vec<FlakyRecord>> {
    let mut select = conn.prepare_cached(
        "SELECT hash, commit_sha, machine, at FROM flaky WHERE check_name = ?1 ORDER BY at",
    )?;
    let rows = select
        .query_map(params![check], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(hash, commit, machine, at)| {
            Ok(FlakyRecord {
                hash,
                commit,
                machine,
                at: parse_time(&at)?,
            })
        })
        .collect()
}

/// The first two records on different commits within the window, as `flaky_repeated` counts
/// them.
fn repeated_pair(records: &[FlakyRecord]) -> Option<(&FlakyRecord, &FlakyRecord)> {
    let window = FLAKY_WINDOW_DAYS * 86_400.0;
    records.iter().enumerate().find_map(|(i, first)| {
        records[i + 1..]
            .iter()
            .find(|later| {
                later.commit != first.commit && later.at.seconds_since(first.at) <= window
            })
            .map(|later| (first, later))
    })
}

fn filing(check: &str, first: &FlakyRecord, second: &FlakyRecord) -> Filing {
    let quote = |record: &FlakyRecord| {
        format!(
            "- {}: failed, then passed on the same inputs, at commit {} on machine {} (input hash {})",
            record.at, record.commit, record.machine, record.hash
        )
    };
    let body = format!(
        "{check} failed and then passed at once on the same inputs twice within \
         {FLAKY_WINDOW_DAYS} days, on different commits. It is switched off on every machine.\n\n\
         {}\n{}\n\n\
         Delete the check, or replace it with a deterministic test and switch it back on.",
        quote(first),
        quote(second)
    );
    let family = check.split_once(':').map_or(check, |(family, _)| family);
    Filing {
        signature: Signature::of_subject(check, "flaky"),
        evidence: format!("{}+{}", first.commit, second.commit),
        fields: TicketFields {
            title: Some(format!("Flaky check switched off: {check}")),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(3),
            labels: Some(vec!["flaky".to_owned(), format!("check:{family}")]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
    }
}
