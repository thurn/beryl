//! An ordinary flake quarantines its check and joins the pending investigation batch.
//! Always-set checks stay on. When several checks flake in one command, the machine gets a
//! separate ticket and every check in that command stays on.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use rusqlite::{Connection, TransactionBehavior, params};

use ignite_core::config::Rules;
use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::FlakyEvidence;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::checks::{self, MACHINE_FAULT_CHECKS};
use ignite_store::hub::flaky_triage;

use super::filing::{self, Filed, Filing, UPKEEP};
use super::invalid_checks::{self, Remedy, RemedyOutcome};
use super::regressions::OUTPUT_LINES;
use super::signature::Signature;
use super::{machines, tail};

/// Files machine faults and always-set flakes first, then adds ordinary flakes to the batch.
pub(super) fn handle(
    conn: &mut Connection,
    rules: &Rules,
    now: Timestamp,
) -> Result<RemedyOutcome> {
    let mut outcome = RemedyOutcome {
        filed: machine_faults(conn, now)?,
        ..RemedyOutcome::default()
    };
    let mut by_check: BTreeMap<&str, Vec<&FlakyEvidence>> = BTreeMap::new();
    let pending = checks::pending_flakes(conn)?;
    for evidence in &pending {
        by_check
            .entry(evidence.check.as_str())
            .or_default()
            .push(evidence);
    }
    for (check, records) in by_check {
        if let Some(first) = records.iter().find(|record| record.always) {
            let remedy = Remedy {
                check,
                category: "flaky-always",
                evidence: reference(first),
                failure: quote(first),
                keep_on: true,
            };
            invalid_checks::file(conn, &remedy, &mut outcome, now, |tx, ticket| {
                acknowledge(tx, ticket, records.iter().copied())
            })?;
        }
    }
    ordinary_batch(conn, rules, &mut outcome, now)?;
    Ok(outcome)
}

fn ordinary_batch(
    conn: &mut Connection,
    rules: &Rules,
    outcome: &mut RemedyOutcome,
    now: Timestamp,
) -> Result<()> {
    // Select, summarize, acknowledge and quarantine under one write lock. Approval and other
    // report writers then see either the entire new scope or the previous one.
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let pending = checks::pending_flakes(&tx)?;
    let always: BTreeSet<_> = pending
        .iter()
        .filter(|e| e.always)
        .map(|e| &e.check)
        .collect();
    let pending: Vec<_> = pending
        .iter()
        .filter(|e| !always.contains(&e.check))
        .collect();
    let Some(first) = pending.first() else {
        return Ok(());
    };
    let names: BTreeSet<_> = pending.iter().map(|e| e.check.as_str()).collect();
    let filed = filing::file_pending_batch(
        &mut tx,
        Filing {
            signature: Signature::of_subject("ordinary checks", flaky_triage::CATEGORY),
            evidence: reference(first),
            fields: TicketFields {
                title: Some("Investigate flaky checks".to_owned()),
                ticket_type: Some(TicketType::Bug),
                priority: Some(3),
                labels: Some(vec!["flaky".to_owned()]),
                metadata: Some([(filing::CHECKS_KEY.to_owned(), serde_json::json!(names))].into()),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: false,
        },
        now,
        |tx, ticket| {
            let previous = ticket
                .map(|id| checks::filed_flakes(tx, id))
                .transpose()?
                .unwrap_or_default();
            Ok(batch_summary(
                previous.iter().chain(pending.iter().copied()),
            ))
        },
    )?;
    acknowledge(&tx, &filed.ticket, pending.iter().copied())?;
    let mut switched_off = Vec::new();
    for check in names {
        if !rules.is_protected(check) && invalid_checks::quarantine(&tx, check)? {
            switched_off.push(check.to_owned());
        }
    }
    tx.commit()?;
    outcome.switched_off.extend(switched_off);
    outcome.filed.push(filed);
    Ok(())
}

fn acknowledge<'a>(
    conn: &Connection,
    ticket: &str,
    records: impl IntoIterator<Item = &'a FlakyEvidence>,
) -> Result<()> {
    let mut update = conn.prepare_cached(
        "UPDATE flaky SET filed = ?4
         WHERE check_name = ?1 AND hash = ?2 AND commit_sha = ?3 AND filed IS NULL",
    )?;
    for report in records {
        update.execute(params![
            report.check.as_str(),
            report.hash.as_str(),
            report.commit.as_str(),
            ticket
        ])?;
    }
    Ok(())
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Cluster<'a> {
    Command(&'a str),
    Legacy {
        machine: &'a str,
        commit: &'a str,
        at: Timestamp,
        hash: &'a str,
    },
}

fn batch_summary<'a>(records: impl Iterator<Item = &'a FlakyEvidence>) -> String {
    let mut clusters: BTreeMap<Cluster<'_>, Vec<&FlakyEvidence>> = BTreeMap::new();
    for report in records {
        let cluster = if report.command.is_empty() {
            Cluster::Legacy {
                machine: report.machine.as_str(),
                commit: report.commit.as_str(),
                at: report.at,
                hash: report.hash.as_str(),
            }
        } else {
            Cluster::Command(&report.command)
        };
        clusters.entry(cluster).or_default().push(report);
    }
    let mut body = String::from(
        "These checks failed and then passed on the same inputs. Run `ignite flaky evidence <this ticket>` \
         for every saved report and its output. Use `ignite flaky list` to see each check's current state.\n\n\
         Compare each cluster with current staging. Matching legacy records without a command ID \
         are leads to investigate. Record the disposition of every cluster on this ticket, \
         including evidence that remains unresolved. File one repair ticket per distinct actionable \
         cause and link it here. In `ignite ticket batch`, label each repair `flaky` and set its \
         `ignite_checks` metadata to the checks it will fix so its close records their outcomes. \
         Follow `ignite guide flaky-checks` for the repair and re-enable checks individually after the fix.",
    );
    for (cluster, reports) in clusters {
        let names: BTreeSet<_> = reports.iter().map(|e| e.check.as_str()).collect();
        let names = names.into_iter().collect::<Vec<_>>().join(", ");
        let first = reports[0];
        let _ = match cluster {
            Cluster::Command(command) => write!(
                body,
                "\n\n- Command `{command}` on {} at {} (commit {}). Checks: {names}.",
                first.machine, first.at, first.commit
            ),
            Cluster::Legacy {
                machine,
                commit,
                at,
                hash,
            } => write!(
                body,
                "\n\n- Legacy evidence on {machine} at {at} (commit {commit}, input {hash}). Checks: {names}."
            ),
        };
        if let Some(evidence) = reports.iter().find(|e| !e.output.trim().is_empty()) {
            let _ = write!(
                body,
                "\n\n  Output from {} (last {OUTPUT_LINES} lines):\n\n",
                evidence.check
            );
            for line in tail(&evidence.output, OUTPUT_LINES).lines() {
                let _ = writeln!(body, "      {line}");
            }
        }
    }
    body
}

fn reference(report: &FlakyEvidence) -> String {
    format!("{}@{}:{}", report.check, report.commit, report.hash)
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
        let records = checks::command_flakes(conn, &command)?;
        let Some(first) = records.first() else {
            continue;
        };
        let name = machines::name(conn, first.machine.as_str())?;
        let mut names: Vec<&str> = records.iter().map(|e| e.check.as_str()).collect();
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
            signature: Signature::of_subject(first.machine.as_str(), "machine-flaky"),
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

/// Where and when the check flaked, followed by the failed command's output.
fn quote(report: &FlakyEvidence) -> String {
    let output = if report.output.is_empty() {
        "(no output was reported)".to_owned()
    } else {
        tail(&report.output, OUTPUT_LINES)
    };
    format!(
        "The check failed and then passed on the same inputs on {} at commit {} on machine {} \
         (input hash {}). Output of the failed run (last {OUTPUT_LINES} lines):\n{output}",
        report.at, report.commit, report.machine, report.hash
    )
}
