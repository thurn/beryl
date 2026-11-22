//! Repair tickets for a check that flaked or whose regression fix changed only test files.
//! The filing, check state and acknowledgement of the evidence commit together.

use rusqlite::{Connection, params};

use ignite_core::config::Rules;
use ignite_core::protocol::hub::CheckState;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{check_state, checks, tickets};

use super::filing::{self, Filed, Filing};
use super::signature::Signature;
use super::{regressions::OUTPUT_LINES, tail};

#[derive(Debug, Default)]
pub(super) struct RemedyOutcome {
    pub switched_off: Vec<String>,
    pub filed: Vec<Filed>,
}

pub(super) struct Remedy<'a> {
    pub check: &'a str,
    pub category: &'a str,
    pub evidence: String,
    pub failure: String,
    pub keep_on: bool,
}

pub(super) fn file(
    conn: &mut Connection,
    remedy: &Remedy<'_>,
    outcome: &mut RemedyOutcome,
    now: Timestamp,
    record: impl Fn(&Connection, &str) -> Result<()>,
) -> Result<()> {
    let check = remedy.check;
    let family = check.split_once(':').map_or(check, |(family, _)| family);
    let always = remedy.category == "flaky-always";
    let (title, status, instruction) = if always {
        (
            format!("Always-set checks flake in {family}"),
            "It stays on because it is in the always set.",
            format!("Fix the cause so {check} gives the same result every time."),
        )
    } else {
        (
            format!("Delete or redesign invalid checks in {family}"),
            if remedy.keep_on {
                "It stays on because it is protected."
            } else {
                "It is switched off on every machine."
            },
            format!(
                "Delete {check} or redesign it so it cannot fail this way again. Record which \
                 remedy you chose and why. See `ignite guide flaky-checks`."
            ),
        )
    };
    let label = if remedy.category == "test-only-regression" {
        "test-only-regression"
    } else {
        "flaky"
    };
    let mut switched_off = false;
    let filed = filing::file_then(
        conn,
        &Filing {
            signature: Signature::of_subject(family, remedy.category),
            evidence: remedy.evidence.clone(),
            fields: TicketFields {
                title: Some(title),
                body: Some(format!(
                    "{check} had an invalid failure. {status}\n\n{}\n\n{instruction}",
                    remedy.failure
                )),
                ticket_type: Some(TicketType::Bug),
                priority: Some(if always { 1 } else { 3 }),
                labels: Some(vec![label.to_owned(), format!("check:{family}")]),
                metadata: Some(
                    [(filing::CHECKS_KEY.to_owned(), serde_json::json!([check]))].into(),
                ),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: false,
        },
        now,
        |tx, ticket| {
            record(tx, ticket)?;
            switched_off = !remedy.keep_on && quarantine(tx, check)?;
            Ok(())
        },
    )?;
    if switched_off {
        outcome.switched_off.push(check.to_owned());
    }
    outcome.filed.push(filed);
    Ok(())
}

/// Quarantines a check once. The caller records its evidence and chooses whether it can be off.
pub(super) fn quarantine(conn: &Connection, check: &str) -> Result<bool> {
    if check_state::state(conn, check)?.is_none_or(|state| state == CheckState::Off) {
        return Ok(false);
    }
    checks::set_state(conn, check, CheckState::Off)?;
    Ok(true)
}

#[derive(Debug)]
struct RegressionClose {
    seq: i64,
    check: String,
    ticket: String,
    first_bad: String,
    fix: String,
    output: Option<String>,
}

/// Each newly recorded test-only close gets one decision, even if the check is switched back on
/// or the original regression is reopened before the next pass.
pub(super) fn regressions(
    conn: &mut Connection,
    rules: &Rules,
    now: Timestamp,
) -> Result<RemedyOutcome> {
    let pending = {
        let mut query = conn.prepare(
            "SELECT close_seq, check_name, ticket, first_bad, fixing_commit, failure_output
             FROM check_fix_outcomes WHERE remedy_ticket = '' ORDER BY close_seq, check_name",
        )?;
        query
            .query_map([], |row| {
                Ok(RegressionClose {
                    seq: row.get(0)?,
                    check: row.get(1)?,
                    ticket: row.get(2)?,
                    first_bad: row.get(3)?,
                    fix: row.get(4)?,
                    output: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut outcome = RemedyOutcome::default();
    for close in pending {
        let acknowledge = |tx: &Connection, ticket: Option<&str>| -> Result<()> {
            tx.execute(
                "UPDATE check_fix_outcomes SET remedy_ticket = ?3
                 WHERE close_seq = ?1 AND check_name = ?2 AND source = 'regression'",
                params![close.seq, close.check, ticket],
            )?;
            Ok(())
        };
        if check_state::state(conn, &close.check)?.is_none() {
            acknowledge(conn, None)?;
            continue;
        }
        let output = failure_output(conn, &close)?;
        let failure = format!(
            "Regression {} first failed at {}. Fixing commit {} changed only test files.\n\n{output}",
            close.ticket, close.first_bad, close.fix
        );
        let remedy = Remedy {
            check: &close.check,
            category: "test-only-regression",
            evidence: format!("{}@close-{}", close.check, close.seq),
            failure,
            keep_on: rules.is_protected(&close.check),
        };
        file(conn, &remedy, &mut outcome, now, |tx, ticket| {
            acknowledge(tx, Some(ticket))
        })?;
    }
    Ok(outcome)
}

fn failure_output(conn: &Connection, close: &RegressionClose) -> Result<String> {
    match &close.output {
        Some(output) if !output.trim().is_empty() => Ok(format!(
            "Output of the failed run (last {OUTPUT_LINES} lines):\n{}",
            tail(output, OUTPUT_LINES)
        )),
        // Regressions that were open before the evidence columns were added still have their
        // original report in the ticket body or a comment.
        None => {
            let check = format!("rerun: ignite test {}", close.check);
            let commit = format!("first bad {},", close.first_bad);
            let matching = |text: &str| {
                text.lines().any(|line| line == check)
                    && text.lines().any(|line| line.starts_with(&commit))
            };
            let comments = tickets::comments(conn, &close.ticket)?;
            let entry = comments
                .into_iter()
                .rev()
                .find(|comment| matching(&comment.text));
            if let Some(comment) = entry {
                return Ok(comment.text);
            }
            if let Some(ticket) = tickets::load(conn, &close.ticket)?
                && matching(&ticket.body)
            {
                return Ok(ticket.body);
            }
            Ok("No output was reported for this failure.".to_owned())
        }
        Some(_) => Ok("No output was reported for this failure.".to_owned()),
    }
}
