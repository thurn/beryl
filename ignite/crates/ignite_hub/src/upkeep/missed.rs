//! Checks tip verification missed (see "Tip verification" in `docs/ignite.md`).
//!
//! A landing whose gate finds a check failing at the `staging` tip it was rebased onto reports
//! it as a regression ([`FoundBy::Landing`]). That repairs the landing, but tip verification
//! should have found the failure first, and each case points at a gap in it. So when such a
//! report opens or joins a regression ticket, the hub looks at what tip verification did at that
//! tip and at the tips it saw after it, and records why it missed the check, in this order:
//!
//! - [`Kind::StalePass`]: a tip verification round passed the check at one of those tips.
//! - [`Kind::NotRun`]: a round finished at one of them without running the check, left none of
//!   the check's family's checks unrun, and left none of them to other machines. It did not have
//!   the check to run: by the inputs the check declares nothing changed since the last verified
//!   commit, so an earlier pass stood.
//! - [`Kind::Elsewhere`]: a round finished at one of them having left some of the family's
//!   checks to other machines, which held their claims or had stored results. The check was
//!   probably among them, and no machine ran it.
//! - [`Kind::Skipped`]: every round that finished there left checks of the family unrun: a newer
//!   tip ended the round, or `master` did not move and the checks past the tip budget wait for a
//!   later round. The newest such round gives the reason.
//! - No round has finished at the tip or after it: tip verification has not reached the tip
//!   yet. That is no miss, and nothing is recorded.
//!
//! A miss adds a comment to the regression ticket, and the dashboard's tips page counts misses
//! per family. When one family is missed [`MISSES_TO_FILE`] times within [`WINDOW_SECONDS`], the
//! hub files one `hub`-labelled ticket for the family, matched by signature, so each later miss
//! adds a comment to it instead of a second ticket.

use std::fmt::Write as _;

use rusqlite::Connection;

use ignite_core::ids::CheckName;
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::protocol::tickets::{BatchRequest, Operation, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::rows::TipMiss;
use ignite_store::hub::tip_misses::{self, Evidence};

use super::filing::{self, Author, Filing};
use super::signature::Signature;

/// The signature category of a family's missed-checks ticket.
pub const CATEGORY: &str = "tip-miss";

/// How many misses of one family within [`WINDOW_SECONDS`] file its ticket.
pub const MISSES_TO_FILE: usize = 3;

/// The window misses are counted over: seven days.
pub const WINDOW_SECONDS: f64 = 7.0 * 86_400.0;

/// Why tip verification missed a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    StalePass,
    Skipped,
    NotRun,
    Elsewhere,
}

impl Kind {
    /// The name stored in `tip_misses.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::StalePass => "stale-pass",
            Kind::Skipped => "skipped",
            Kind::NotRun => "not-run",
            Kind::Elsewhere => "elsewhere",
        }
    }
}

/// One miss: its kind and the sentence that explains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Miss {
    pub kind: Kind,
    pub reason: String,
}

fn short(commit: &str) -> &str {
    commit.get(..9).unwrap_or(commit)
}

/// Why tip verification missed `check`, which a landing found failing at `tip`, or `None` when
/// no round has finished at that tip or a later one.
pub fn explain(evidence: &Evidence, check: &CheckName, tip: &str) -> Option<Miss> {
    let family = check.family();
    let at = |commit: &str| {
        if commit == tip {
            format!("at that tip ({})", short(commit))
        } else {
            format!("at the later tip {}", short(commit))
        }
    };
    if let Some(commit) = &evidence.passed_at {
        return Some(Miss {
            kind: Kind::StalePass,
            reason: format!(
                "Tip verification passed the check {}, so its pass is stale: the check's \
                 declared inputs do not cover something it reads, or it does not give the same \
                 result for the same inputs.",
                at(commit)
            ),
        });
    }
    let left = |budget: &Option<ignite_core::tipround::RoundBudget>| {
        budget
            .as_ref()
            .and_then(|b| b.unrun.get(family).copied())
            .filter(|n| *n > 0)
    };
    // A round whose record has no budget, from a daemon built before it, says nothing about what
    // it left out. A round that left some of the family to other machines did not decide for
    // itself that the check was unchanged.
    let elsewhere = |budget: &Option<ignite_core::tipround::RoundBudget>| {
        budget
            .as_ref()
            .and_then(|b| b.elsewhere.get(family).copied())
            .filter(|n| *n > 0)
    };
    let complete = |(_, b): &&(String, Option<ignite_core::tipround::RoundBudget>)| {
        b.is_some() && left(b).is_none() && elsewhere(b).is_none()
    };
    if let Some((commit, _)) = evidence.rounds.iter().find(complete) {
        return Some(Miss {
            kind: Kind::NotRun,
            reason: format!(
                "A tip verification round finished {} without running the check, and left none \
                 of the {family} checks unrun. It did not have the check to run: by the inputs \
                 the check declares, nothing it reads changed since the last verified commit, \
                 so an earlier pass stood.",
                at(commit)
            ),
        });
    }
    if let Some((commit, budget)) = evidence
        .rounds
        .iter()
        .find(|(_, b)| left(b).is_none() && elsewhere(b).is_some())
    {
        let n = elsewhere(budget).unwrap_or_default();
        return Some(Miss {
            kind: Kind::Elsewhere,
            reason: format!(
                "The tip verification round {} left {n} {family} checks to other machines, \
                 which held their claims or had stored results, and this check was likely \
                 among them: no machine ran it.",
                at(commit)
            ),
        });
    }
    let (commit, budget) = evidence.rounds.last()?;
    let budget = budget.as_ref()?;
    let unrun = left(&Some(budget.clone())).unwrap_or_default();
    let why = match &budget.ended_by {
        Some(newer) => format!(
            "the newer tip {} ended the round first",
            short(newer.as_str())
        ),
        None => "`master` did not move, so the checks past the tip budget wait for a later \
                 round"
            .to_owned(),
    };
    Some(Miss {
        kind: Kind::Skipped,
        reason: format!(
            "The tip verification round {} left {unrun} {family} checks unrun, this one \
             likely among them: {why}.",
            at(commit)
        ),
    })
}

/// When `report` came from a landing, records why tip verification missed its check, adds that
/// to the regression `ticket` as a comment, and files or comments on the family's ticket once
/// the family has been missed [`MISSES_TO_FILE`] times within [`WINDOW_SECONDS`]. Returns the
/// miss recorded, or `None` for a report from tip verification, a tip no round has finished at
/// yet, or a miss already recorded.
pub fn record(
    conn: &mut Connection,
    author: Author,
    report: &RegressionReport,
    ticket: &str,
    now: Timestamp,
) -> Result<Option<Miss>> {
    if report.found_by != FoundBy::Landing {
        return Ok(None);
    }
    let check = report.check.as_str();
    let tip = report
        .detected_at
        .as_ref()
        .unwrap_or(&report.first_bad)
        .as_str();
    let evidence = tip_misses::evidence(conn, check, tip)?;
    let Some(miss) = explain(&evidence, &report.check, tip) else {
        return Ok(None);
    };
    let row = TipMiss {
        check_name: check.to_owned(),
        family: report.check.family().to_owned(),
        tip: tip.to_owned(),
        kind: miss.kind.as_str().to_owned(),
        reason: miss.reason.clone(),
        ticket: ticket.to_owned(),
        at: now.to_string(),
    };
    if !tip_misses::record(conn, &row)? {
        return Ok(None);
    }
    let comment = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Comment {
            id: ticket.to_owned(),
            text: format!(
                "Tip verification missed {check}: a landing found it failing at the staging \
                 tip it was rebased onto ({}) before tip verification did. {}",
                short(tip),
                miss.reason
            ),
        }],
    };
    if let BatchOutcome::Conflict(conflict) =
        batch::apply(conn, &comment, author.actor, author.machine, now)?
    {
        eprintln!(
            "ignite-hub: the comment on {ticket} about a missed check conflicted at operation {}",
            conflict.failed
        );
    }
    file_family(conn, &row, now)?;
    Ok(Some(miss))
}

/// Files or comments on the family's ticket once its misses within the window reach
/// [`MISSES_TO_FILE`].
/// The hub files it on its own, as upkeep: no one report is the reason for it.
fn file_family(conn: &mut Connection, newest: &TipMiss, now: Timestamp) -> Result<()> {
    let misses = tip_misses::since(
        conn,
        Some(&newest.family),
        now.plus_seconds(-WINDOW_SECONDS),
    )?;
    if misses.len() < MISSES_TO_FILE {
        return Ok(());
    }
    let family = &newest.family;
    let mut body = format!(
        "Tip verification missed {count} {family} checks in the last seven days: each time, a \
         landing found the check failing at the staging tip it was rebased onto before tip \
         verification reported it. Find what keeps tip verification from running these checks \
         at the tips where they fail, and fix it (see \"Tip verification\" in docs/ignite.md).\n",
        count = misses.len()
    );
    for miss in &misses {
        let _ = write!(
            body,
            "\n- {} at {} ({}, regression {}): {}",
            miss.check_name,
            short(&miss.tip),
            miss.kind,
            miss.ticket,
            miss.reason
        );
    }
    let filing = Filing {
        signature: Signature::of_subject(family, CATEGORY),
        evidence: format!("{}@{}", newest.check_name, newest.tip),
        fields: TicketFields {
            title: Some(format!("Tip verification misses failing {family} checks")),
            body: Some(body),
            ticket_type: Some(TicketType::Task),
            priority: Some(2),
            labels: Some(vec![
                "improvement".to_owned(),
                format!("rule:{CATEGORY}"),
                format!("check:{family}"),
            ]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    filing::file(conn, filing::UPKEEP, &filing, now)?;
    Ok(())
}
