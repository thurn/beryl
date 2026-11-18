//! `rejected-command`: a verb or flag that agents keep typing and the `ignite` executable does
//! not have. Its ticket asks whether to add the command or an alias for it.
//!
//! - The executable sends each command line it refuses to parse to `POST /commands/rejected`.
//!   A command is the recognized subcommands followed by the rejected word, such as
//!   `ticket dep list`.
//! - Over the last week, a command counts only once at least three sessions have tried it, so
//!   one session's typo files nothing.
//! - The cost is an estimate in dollars: each try costs the model request that was refused and
//!   the requests the agent spent working around it. Adding the command saves all of it.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use super::{DOLLARS, Finding, Rule, WEEK_SECONDS};
use ignite_core::time::Timestamp;

pub const NAME: &str = "rejected-command";

/// A command counts once this many sessions have tried it.
const MIN_SESSIONS: usize = 3;
/// The estimated cost of one try: the refused request and the workaround after it.
const DOLLARS_PER_TRY: f64 = 0.25;
/// How many different argument lists a ticket quotes.
const SAMPLES: usize = 3;

pub struct RejectedCommand {
    pub report_only: bool,
}

/// One command's tries in the week.
#[derive(Default)]
struct Tries {
    count: u32,
    sessions: BTreeSet<String>,
    samples: Vec<String>,
    suggestion: Option<String>,
}

impl Rule for RejectedCommand {
    fn name(&self) -> &str {
        NAME
    }

    fn report_only(&self) -> bool {
        self.report_only
    }

    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>> {
        let mut found = Vec::new();
        for (verb, tries) in week(conn, now)? {
            if tries.sessions.len() < MIN_SESSIONS {
                continue;
            }
            let cost = f64::from(tries.count) * DOLLARS_PER_TRY;
            let sessions = tries.sessions.len();
            let samples = tries
                .samples
                .iter()
                .map(|args| format!("- `ignite {args}`"))
                .collect::<Vec<_>>()
                .join("\n");
            let suggestion = tries
                .suggestion
                .as_ref()
                .map(|suggestion| format!(" The parser suggested `{suggestion}`."))
                .unwrap_or_default();
            found.push(Finding {
                rule: NAME.to_owned(),
                subject: verb.clone(),
                unit: DOLLARS.to_owned(),
                cost,
                saving: cost,
                title: format!("Add or alias `ignite {verb}`"),
                body: format!(
                    "Agents tried `ignite {verb}` {count} times from {sessions} sessions in the \
                     last week, and the executable refused it each time.{suggestion} At an \
                     estimated ${DOLLARS_PER_TRY:.2} a try for the refused request and the \
                     workaround, that costs ${cost:.2} a week.\n\nWhat they typed:\n{samples}\n\n\
                     Add the command, or an alias to the command that does what they wanted, or \
                     make the error say which command to use.\n\
                     measure: {NAME} dollars a week for {verb}, now {cost:.2}",
                    count = tries.count,
                ),
            });
        }
        Ok(found)
    }

    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64> {
        Ok(week(conn, now)?
            .get(subject)
            .map_or(0.0, |tries| f64::from(tries.count) * DOLLARS_PER_TRY))
    }
}

/// The columns of `rejected_commands` the rule reads.
struct Row {
    id: i64,
    machine: String,
    session: Option<String>,
    verb: String,
    args: String,
    suggestion: Option<String>,
}

/// The week's rejected commands before `now`, by command.
fn week(conn: &Connection, now: Timestamp) -> rusqlite::Result<BTreeMap<String, Tries>> {
    let since = now.plus_seconds(-WEEK_SECONDS).to_string();
    let mut statement = conn.prepare(
        "SELECT id, machine, session, verb, args, suggestion FROM rejected_commands
         WHERE at >= ?1 AND at <= ?2 ORDER BY at, id",
    )?;
    let rows = statement
        .query_map(params![since, now.to_string()], |row| {
            Ok(Row {
                id: row.get(0)?,
                machine: row.get(1)?,
                session: row.get(2)?,
                verb: row.get(3)?,
                args: row.get(4)?,
                suggestion: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut week: BTreeMap<String, Tries> = BTreeMap::new();
    for row in rows {
        let tries = week.entry(row.verb).or_default();
        tries.count += 1;
        // A command line from no known session counts as its own session.
        tries.sessions.insert(
            row.session
                .unwrap_or_else(|| format!("{}#{}", row.machine, row.id)),
        );
        let args: Vec<String> = serde_json::from_str(&row.args).unwrap_or_default();
        let args = args.join(" ");
        if tries.samples.len() < SAMPLES && !tries.samples.contains(&args) {
            tries.samples.push(args);
        }
        if row.suggestion.is_some() {
            tries.suggestion = row.suggestion;
        }
    }
    Ok(week)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::filing;
    use ignite_core::config;
    use ignite_core::protocol::hub;
    use ignite_store::hub as store;
    use ignite_store::hub::rejected;

    fn now() -> Timestamp {
        "2026-11-17T12:00:00.000Z".parse().unwrap()
    }

    /// `tries` tries of `verb` from each of `sessions` sessions, a day ago.
    fn tried(conn: &Connection, verb: &str, sessions: u32, tries: u32) {
        let at = now().plus_seconds(-86_400.0);
        for session in 0..sessions {
            for _ in 0..tries {
                let command = hub::RejectedCommand {
                    verb: verb.to_owned(),
                    args: verb.split(' ').map(str::to_owned).collect(),
                    kind: "unknown subcommand".to_owned(),
                    suggestion: None,
                    session: Some(format!("s-{session}").parse().unwrap()),
                };
                rejected::insert(conn, "m-1", &command, at).unwrap();
            }
        }
    }

    fn evaluate(conn: &mut Connection) -> filing::Evaluation {
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(RejectedCommand { report_only: false })];
        filing::evaluate(conn, &rules, &config::Rules::default(), now()).unwrap()
    }

    #[test]
    fn a_command_three_sessions_keep_trying_files_one_ticket() {
        let mut conn = store::open_in_memory().unwrap();
        tried(&conn, "ticket dep list", 3, 40);
        let evaluation = evaluate(&mut conn);
        assert_eq!(evaluation.filed.len(), 1);
        let (title, body): (String, String) = conn
            .query_row("SELECT title, body FROM tickets", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(title, "Add or alias `ignite ticket dep list`");
        assert!(body.contains("120 times from 3 sessions"), "{body}");
        assert!(body.contains("- `ignite ticket dep list`"), "{body}");
        assert!(
            evaluate(&mut conn).filed.is_empty(),
            "one ticket per command"
        );
    }

    #[test]
    fn two_sessions_file_nothing_however_often_they_try() {
        let mut conn = store::open_in_memory().unwrap();
        tried(&conn, "ticket dep list", 2, 200);
        let evaluation = evaluate(&mut conn);
        assert!(evaluation.filed.is_empty() && evaluation.reported.is_empty());
    }

    #[test]
    fn a_few_tries_are_reported_without_a_ticket() {
        let mut conn = store::open_in_memory().unwrap();
        tried(&conn, "ticket children --open", 3, 2);
        let evaluation = evaluate(&mut conn);
        assert!(evaluation.filed.is_empty());
        assert_eq!(evaluation.reported.len(), 1);
        assert!((evaluation.reported[0].cost - 1.5).abs() < 1e-9);
    }
}
