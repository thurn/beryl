//! P1 tickets for checks left `incomplete`: one ticket per cause, never one per check.
//!
//! - A build that failed or was refused: one ticket per product, "Build fails: <product>".
//! - A manifest entry that is invalid: one ticket per family and message.
//! - A command that ended without reporting some of its checks: one ticket per family and what
//!   its output says.
//! - A hung command, and a command that says it cannot run on a machine: one ticket per family
//!   and output.
//!
//! The signature is computed from the cause, never from a check's name, so a later sighting of
//! the same cause, or a report of another check it holds back, adds to the open ticket. An
//! older daemon sends one report per check, and every check's report of one cause has the same
//! signature, so it still files one ticket.
//!
//! A report says whether the tip being verified waits on the checks it names
//! (`IncompleteReport::holds_tip`). Such a cause keeps `master` from moving, so its ticket is filed
//! released; a later sighting that holds the tip releases a ticket still waiting for a person.
//!
//! As a second guard, one run (one family's command, on one machine, at one commit) files at
//! most [`ROUND_LIMIT`] tickets. Each cause past that is a comment on one more ticket, which says
//! how many causes were left out and lists them.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde_json::Value;

use ignite_core::ids::CheckName;
use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::{IncompleteReason, IncompleteReport};
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::tips;

use super::super::filing::{self, Author, Filed, Filing};
use super::super::regressions::OUTPUT_LINES;
use super::super::signature::Signature;
use super::super::tail;

/// How many tickets one run may file for its incomplete checks before the rest of its causes go
/// on a single ticket.
pub const ROUND_LIMIT: usize = 5;

/// How many of a cause's checks its ticket names.
const LISTED: usize = ignite_core::protocol::hub::INCOMPLETE_LISTED;

/// The label on every ticket for incomplete checks.
const LABEL: &str = "incomplete";

/// The text a daemon puts on a check that a failed build kept from running.
const BUILD_FAILED: &str = "a build it needs failed";

/// The words before the message of a check whose inputs cannot be resolved.
const UNRESOLVED: &str = " cannot be resolved: ";

/// One cause of incomplete checks, as its ticket states it.
struct Cause {
    title: String,
    /// The ticket's first sentence: what left the checks without a result.
    why: String,
    signature: Signature,
}

/// Files the ticket for one cause of incomplete checks, or adds the sighting to its open ticket.
pub fn incomplete(
    conn: &mut Connection,
    author: Author,
    report: &IncompleteReport,
    now: Timestamp,
) -> Result<Filed> {
    let family = report.check.family();
    let checks = checks_of(report);
    let cause = cause(report, &checks);
    let labels = [LABEL.to_owned(), format!("check:{family}")];
    let evidence = format!("{}@{}", report.commit, report.machine);
    // The checks this cause holds back: once each has a result again, the cause has cleared
    // (see `clearing`).
    let held: Vec<&str> = checks.iter().map(|check| check.as_str()).collect();
    let metadata = BTreeMap::from([(filing::CHECKS_KEY.to_owned(), Value::from(held))]);
    let fields = |title: String, body: String| TicketFields {
        title: Some(title),
        body: Some(body),
        ticket_type: Some(TicketType::Bug),
        priority: Some(1),
        labels: Some(
            [
                labels[0].clone(),
                labels[1].clone(),
                machine_label(&report.host),
            ]
            .into(),
        ),
        metadata: Some(metadata.clone()),
        needs_os: Some(report.platform),
        ..TicketFields::default()
    };
    let round_labels = [LABEL, labels[1].as_str()];
    let full = filing::open_ticket(conn, &cause.signature)?.is_none()
        && filing::sighted(conn, &round_labels, &evidence)? >= ROUND_LIMIT;
    let filing = if full {
        overflow(report, &cause, &checks, fields)
    } else {
        Filing {
            evidence,
            fields: fields(cause.title.clone(), body(report, &cause, &checks)),
            signature: cause.signature,
            deps: Vec::new(),
            released: false,
        }
    };
    let filing = if report.holds_tip {
        let tip = report.commit.as_str();
        tips::seen(conn, tip, now)?;
        filing.holding_tip(tip)
    } else {
        filing
    };
    filing::file(conn, author, &filing, now)
}

/// The filing of a cause past [`ROUND_LIMIT`]: a comment on the run's one ticket for the causes
/// left out, whose sightings are those causes.
fn overflow(
    report: &IncompleteReport,
    cause: &Cause,
    checks: &[&CheckName],
    fields: impl Fn(String, String) -> TicketFields,
) -> Filing {
    let family = report.check.family();
    let run = format!("{family}@{}@{}", report.commit, report.machine);
    let body = format!(
        "One run of {family} on machine {} ({}) at commit {} found more causes of incomplete \
         checks than the hub files tickets for ({ROUND_LIMIT}). It filed those and leaves the \
         rest to this ticket, whose comments list each cause left out.\n\n\
         left out: {}\n\n{}",
        report.host,
        report.machine,
        report.commit,
        cause.title,
        body(report, cause, checks)
    );
    Filing {
        signature: Signature::of_subject(&run, "incomplete-overflow"),
        evidence: cause.signature.key(),
        fields: fields(
            format!("Too many causes of incomplete checks: {family}"),
            body,
        ),
        deps: Vec::new(),
        released: false,
    }
}

/// The checks a report holds back: its list, or its one check.
fn checks_of(report: &IncompleteReport) -> Vec<&CheckName> {
    if report.checks.is_empty() {
        vec![&report.check]
    } else {
        report.checks.iter().collect()
    }
}

/// How many checks the cause holds back.
fn count(report: &IncompleteReport, checks: &[&CheckName]) -> usize {
    usize::try_from(report.count)
        .unwrap_or(usize::MAX)
        .max(checks.len())
}

/// The reason a report gives, or for an older daemon's report the one its output shows: it sent
/// a failed build and an invalid entry as `Unreported`.
fn reason(report: &IncompleteReport) -> IncompleteReason {
    match report.reason {
        IncompleteReason::Unreported
            if report.output.contains(BUILD_FAILED)
                || report.check.as_str().ends_with(":build") =>
        {
            IncompleteReason::BuildFailed
        }
        IncompleteReason::Unreported if report.output.contains(UNRESOLVED) => {
            IncompleteReason::InvalidEntry
        }
        reason => reason,
    }
}

/// `text` with each check's name replaced, so that reports of two checks with one cause read
/// the same.
fn without_names(text: &str, checks: &[&CheckName]) -> String {
    checks.iter().fold(text.to_owned(), |text, check| {
        text.replace(check.as_str(), "<check>")
    })
}

fn cause(report: &IncompleteReport, checks: &[&CheckName]) -> Cause {
    let family = report.check.family();
    let platform = report.platform.map_or("", |os| os.as_str());
    let count = count(report, checks);
    let these = if count == 1 {
        "1 check".to_owned()
    } else {
        format!("{count} checks")
    };
    let output = without_names(&report.output, checks);
    let host = report.host.as_str();
    match reason(report) {
        IncompleteReason::BuildFailed => {
            let product = report
                .product
                .clone()
                .or_else(|| named_product(&report.output))
                .unwrap_or_else(|| format!("a product of {family}"));
            Cause {
                title: format!("Build fails: {product}"),
                why: format!(
                    "The build of {product} failed or was refused, so {these} that need it did \
                     not run."
                ),
                signature: Signature {
                    platform: platform.to_owned(),
                    ..Signature::of_subject(&product, "build-failed")
                },
            }
        }
        IncompleteReason::InvalidEntry => {
            let message = output
                .split_once(UNRESOLVED)
                .map_or(output.as_str(), |(_, message)| message)
                .trim()
                .to_owned();
            Cause {
                title: format!("Manifest entry is invalid: {family}"),
                why: format!(
                    "The manifest entry of {family} is invalid, so {these} of it cannot be keyed \
                     or run: {message}"
                ),
                signature: Signature::of_output(family, "invalid-entry", platform, &message, host),
            }
        }
        IncompleteReason::Unreported => Cause {
            title: format!("Checks stay incomplete: {family}"),
            why: if report.attempts > 1 {
                format!(
                    "The command of {family} ended without reporting {these} in each of {} \
                     attempts.",
                    report.attempts
                )
            } else {
                format!(
                    "Tip verification has no result for {these} of {family}; the output says why."
                )
            },
            signature: Signature::of_output(family, "incomplete", platform, &output, host),
        },
        IncompleteReason::CannotRun => Cause {
            title: format!("Checks cannot run on {host}: {family}"),
            why: format!(
                "The command of {family} said it cannot run on {host}, so {these} are not run \
                 there again."
            ),
            signature: Signature::of_output(family, "cannot-run", platform, &output, host),
        },
        IncompleteReason::Hung => Cause {
            title: format!("Command hangs: {family}"),
            why: format!(
                "The command of {family} ran past its hang limit and was ended with everything \
                 it started, leaving {these} without a result. It is not run again, because a \
                 hung command hangs again; the output lists the processes it left."
            ),
            signature: Signature::of_output(family, "hung", platform, &output, host),
        },
    }
}

/// The product a report's output names, as tip verification words it: "The build of <product>
/// failed".
fn named_product(output: &str) -> Option<String> {
    let rest = output.split_once("The build of ")?.1;
    let product = rest.split_once(" failed")?.0.trim();
    (!product.is_empty()).then(|| product.to_owned())
}

fn body(report: &IncompleteReport, cause: &Cause, checks: &[&CheckName]) -> String {
    let count = count(report, checks);
    let mut listed: Vec<&str> = checks.iter().take(LISTED).map(|c| c.as_str()).collect();
    let more = count.saturating_sub(listed.len());
    let more_line = format!("and {more} more");
    if more > 0 {
        listed.push(&more_line);
    }
    format!(
        "{}\nmachine {} ({}), commit {}\n\
         A landing or a tip that needs these checks waits for them.\n\n\
         checks ({count}):\n- {}\n\n\
         output (last {OUTPUT_LINES} lines):\n{}",
        cause.why,
        report.host,
        report.machine,
        report.commit,
        listed.join("\n- "),
        tail(&report.output, OUTPUT_LINES)
    )
}

#[cfg(test)]
#[path = "stuck_incomplete_tests.rs"]
mod tests;
