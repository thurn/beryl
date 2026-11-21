//! One flake on the same inputs is enough to file a remedy and switch off an ordinary check.
//! When three or more checks flake in one command, file a machine ticket first and leave
//! every check in that command running.

use rusqlite::{Connection, params};

use ignite_core::config::Rules;
use ignite_core::pins::machine_label;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::checks::{self, MACHINE_FAULT_CHECKS};

use super::filing::{self, Filed, Filing, UPKEEP};
use super::invalid_checks::{self, Remedy, RemedyOutcome};
use super::regressions::OUTPUT_LINES;
use super::signature::Signature;
use super::{machines, parse_time, tail};

/// One flaky record: the check failed and then passed on the same inputs.
struct FlakyRecord {
    name: String,
    hash: String,
    commit: String,
    machine: String,
    at: Timestamp,
    always: bool,
    output: String,
}

/// Files machine faults first, then handles the first unfiled flake of each check. One ticket
/// per family holds ordinary flakes. Always-set checks get a P1 step repair and stay on.
pub(super) fn handle(
    conn: &mut Connection,
    rules: &Rules,
    now: Timestamp,
) -> Result<RemedyOutcome> {
    let mut outcome = RemedyOutcome {
        filed: machine_faults(conn, now)?,
        ..RemedyOutcome::default()
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
    let condition = format!("check_name = ?1 AND {}", checks::counting_flaky());
    for check in candidates {
        let records = records(conn, &condition, &check)?;
        let Some(first) = records
            .iter()
            .find(|record| record.always)
            .or(records.first())
        else {
            continue;
        };
        let remedy = Remedy {
            check: &check,
            category: if first.always {
                "flaky-always"
            } else {
                "flaky"
            },
            evidence: format!("{check}@{}:{}", first.commit, first.hash),
            failure: quote(first),
            keep_on: first.always || rules.is_protected(&check),
        };
        invalid_checks::file(conn, &remedy, &mut outcome, now, |tx, ticket| {
            let mut update = tx.prepare_cached(
                "UPDATE flaky SET filed = ?4
                 WHERE check_name = ?1 AND hash = ?2 AND commit_sha = ?3 AND filed IS NULL",
            )?;
            for record in &records {
                update.execute(params![check, record.hash, record.commit, ticket])?;
            }
            Ok(())
        })?;
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
        let mut names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
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
        .map(|(name, hash, commit, machine, at, always, output)| {
            Ok(FlakyRecord {
                name,
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

/// Where and when the check flaked, followed by the failed command's output.
fn quote(record: &FlakyRecord) -> String {
    let output = if record.output.is_empty() {
        "(no output was reported)".to_owned()
    } else {
        tail(&record.output, OUTPUT_LINES)
    };
    format!(
        "The check failed and then passed on the same inputs on {} at commit {} on machine {} \
         (input hash {}). Output of the failed run (last {OUTPUT_LINES} lines):\n{output}",
        record.at, record.commit, record.machine, record.hash
    )
}
