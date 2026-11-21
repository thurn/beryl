//! Flaky checks.
//!
//! - A check with a second flaky record within 7 days of the first, on a different commit, is
//!   switched off on every machine, and a P3 ticket asks for it to be deleted or replaced with a
//!   deterministic test. The ticket quotes both failures' output.
//! - A check in the always set is never switched off. Its second flaky record files a P1 ticket
//!   to fix the step instead, because a formatter or lint that flakes is almost always the
//!   machine.
//! - Both kinds of ticket are shared by a check family (the part of a check's name before the
//!   colon). While a family's ticket is not closed, the next check of that family to be switched
//!   off is added to it as a comment instead of filing another ticket. Flakes in one family
//!   often share a cause, such as a loaded machine, and one session investigating them together
//!   costs much less than one session per check.
//! - When three or more checks flaked in one command run, the machine is at fault (a full disk,
//!   a stuck browser page). None of those records counts toward switching a check off, and the
//!   command gets one P1 ticket for the machine.
//!
//! Each record that a ticket was filed for is marked with that ticket, and no longer counts, so
//! a later pass files nothing more for it.

use rusqlite::{Connection, params};

use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::CheckState;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::checks::{self, FLAKY_WINDOW_DAYS, MACHINE_FAULT_CHECKS};

use super::filing::{self, Filed, Filing, UPKEEP};
use super::regressions::OUTPUT_LINES;
use super::signature::Signature;
use super::{machines, parse_time, tail};

/// One flaky record: the check failed and then passed on the same inputs.
struct FlakyRecord {
    check: String,
    hash: String,
    commit: String,
    machine: String,
    at: Timestamp,
    always: bool,
    output: String,
}

/// What one pass over the flaky records did.
#[derive(Debug, Default)]
pub struct FlakyOutcome {
    /// Each check switched off, with its P3 ticket.
    pub switched_off: Vec<(String, Filed)>,
    /// The P1 tickets for always-set checks and for machine faults.
    pub filed: Vec<Filed>,
}

/// Files one machine ticket for each command in which many checks flaked, then switches off
/// each check that is on and has two flaky records that count, or files a P1 ticket for it if
/// it is in the always set.
pub fn switch_off_repeated(conn: &mut Connection, now: Timestamp) -> Result<FlakyOutcome> {
    let mut outcome = FlakyOutcome {
        filed: machine_faults(conn, now)?,
        ..FlakyOutcome::default()
    };
    let candidates: Vec<String> = {
        let mut select = conn.prepare(&format!(
            "SELECT DISTINCT f.check_name FROM flaky f JOIN checks c ON c.name = f.check_name
             WHERE c.state != 'off' AND {} ORDER BY f.check_name",
            checks::counting_flaky()
        ))?;
        select
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for check in candidates {
        if !checks::flaky_repeated(conn, &check)? {
            continue;
        }
        let records = records(
            conn,
            &format!("check_name = ?1 AND {}", checks::counting_flaky()),
            &check,
        )?;
        let Some((first, second)) = repeated_pair(&records) else {
            continue;
        };
        if first.always || second.always {
            let filed = filing::file(conn, UPKEEP, &always_filing(&check, first, second), now)?;
            mark_filed(conn, &[first, second], &filed.ticket)?;
            outcome.filed.push(filed);
        } else {
            // The ticket comes first. If filing fails the check stays on and the next pass tries
            // again; if the process stops between the two, the next pass finds the same records
            // unmarked, files nothing new (the ticket already holds the evidence) and then
            // switches the check off.
            let filed = filing::file(conn, UPKEEP, &off_filing(&check, first, second), now)?;
            checks::set_state(conn, &check, CheckState::Off)?;
            mark_filed(conn, &[first, second], &filed.ticket)?;
            outcome.switched_off.push((check, filed));
        }
    }
    Ok(outcome)
}

/// One P1 ticket per command in which [`MACHINE_FAULT_CHECKS`] or more checks flaked and whose
/// records have no ticket yet. Repeats on the same machine comment on its open ticket.
fn machine_faults(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let commands: Vec<String> = {
        let mut select = conn.prepare(
            "SELECT command FROM flaky WHERE command != ''
             GROUP BY command
             HAVING COUNT(DISTINCT check_name) >= ?1 AND COUNT(*) > COUNT(filed)
             ORDER BY MIN(at)",
        )?;
        select
            .query_map(params![MACHINE_FAULT_CHECKS], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut filed = Vec::new();
    for command in commands {
        let records = records(conn, "command = ?1", &command)?;
        let Some(first) = records.first() else {
            continue;
        };
        let name = machines::name(conn, &first.machine)?;
        let mut names: Vec<&str> = records.iter().map(|r| r.check.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        let body = format!(
            "{} checks failed and then passed at once on the same inputs in one command on \
             {name} ({}), at commit {}. That many at once is a fault of the machine, such as a \
             full disk or a stuck browser page, so none of these records counts toward \
             switching a check off.\n\nchecks: {}\n\noutput (last {OUTPUT_LINES} lines):\n{}",
            names.len(),
            first.machine,
            first.commit,
            names.join(", "),
            tail(&first.output, OUTPUT_LINES)
        );
        let filing = Filing {
            signature: Signature::of_subject(&first.machine, "machine-flaky"),
            evidence: command.clone(),
            fields: TicketFields {
                title: Some(format!("Many checks flaked at once on {name}")),
                body: Some(body),
                ticket_type: Some(TicketType::Bug),
                priority: Some(1),
                labels: Some(vec!["machine".to_owned(), machine_label(&name)]),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: false,
        };
        let one = filing::file(conn, UPKEEP, &filing, now)?;
        conn.execute(
            "UPDATE flaky SET filed = ?2 WHERE command = ?1 AND filed IS NULL",
            params![command, one.ticket],
        )?;
        filed.push(one);
    }
    Ok(filed)
}

/// The records matching `condition`, whose one parameter is `value`, oldest first.
fn records(conn: &Connection, condition: &str, value: &str) -> Result<Vec<FlakyRecord>> {
    let mut select = conn.prepare_cached(&format!(
        "SELECT check_name, hash, commit_sha, machine, at, always, output FROM flaky
         WHERE {condition} ORDER BY at, check_name"
    ))?;
    let rows = select
        .query_map(params![value], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(check, hash, commit, machine, at, always, output)| {
            Ok(FlakyRecord {
                check,
                hash,
                commit,
                machine,
                at: parse_time(&at)?,
                always,
                output,
            })
        })
        .collect()
}

fn mark_filed(conn: &Connection, records: &[&FlakyRecord], ticket: &str) -> Result<()> {
    let mut update = conn.prepare_cached(
        "UPDATE flaky SET filed = ?4 WHERE check_name = ?1 AND hash = ?2 AND commit_sha = ?3",
    )?;
    for record in records {
        update.execute(params![record.check, record.hash, record.commit, ticket])?;
    }
    Ok(())
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

/// Both failures, each with where and when it happened and its output.
fn quote_both(first: &FlakyRecord, second: &FlakyRecord) -> String {
    let quote = |record: &FlakyRecord| {
        let output = if record.output.is_empty() {
            "(no output was reported)".to_owned()
        } else {
            tail(&record.output, OUTPUT_LINES)
        };
        format!(
            "- {}: failed, then passed on the same inputs, at commit {} on machine {} (input \
             hash {}). Output of the failed run (last {OUTPUT_LINES} lines):\n{output}",
            record.at, record.commit, record.machine, record.hash
        )
    };
    format!("{}\n\n{}", quote(first), quote(second))
}

fn family(check: &str) -> &str {
    check.split_once(':').map_or(check, |(family, _)| family)
}

/// Names one check's pair of flaky records, so that the family's shared ticket records each
/// check's pair once.
fn evidence(check: &str, first: &FlakyRecord, second: &FlakyRecord) -> String {
    format!("{check}@{}+{}", first.commit, second.commit)
}

fn off_filing(check: &str, first: &FlakyRecord, second: &FlakyRecord) -> Filing {
    let family = family(check);
    let body = format!(
        "{check} failed and then passed at once on the same inputs twice within \
         {FLAKY_WINDOW_DAYS} days, on different commits. It is switched off on every machine.\n\n\
         {}\n\n\
         Delete {check}, or replace it with a deterministic test and switch it back on.",
        quote_both(first, second)
    );
    Filing {
        signature: Signature::of_subject(family, "flaky"),
        evidence: evidence(check, first, second),
        fields: TicketFields {
            title: Some(format!("Flaky checks switched off in {family}")),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(3),
            labels: Some(vec!["flaky".to_owned(), format!("check:{family}")]),
            // The filed records are linked afterward. A close can already see the check here.
            metadata: Some([(filing::CHECKS_KEY.to_owned(), serde_json::json!([check]))].into()),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    }
}

fn always_filing(check: &str, first: &FlakyRecord, second: &FlakyRecord) -> Filing {
    let family = family(check);
    let body = format!(
        "{check} is in the always set and failed and then passed at once on the same inputs \
         twice within {FLAKY_WINDOW_DAYS} days, on different commits. A check in the always set \
         is never switched off, so it still runs. A formatter or lint that flakes is almost \
         always the machine: a file another process changed, a shared temporary path or a \
         download. Fix the step so that it gives the same result every time.\n\n{}",
        quote_both(first, second)
    );
    Filing {
        signature: Signature::of_subject(family, "flaky-always"),
        evidence: evidence(check, first, second),
        fields: TicketFields {
            title: Some(format!("Always-set checks flake in {family}")),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(1),
            labels: Some(vec!["flaky".to_owned(), format!("check:{family}")]),
            // The filed records are linked afterward. A close can already see the check here.
            metadata: Some([(filing::CHECKS_KEY.to_owned(), serde_json::json!([check]))].into()),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    }
}
