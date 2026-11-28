//! The refusal of `ignite land` and `ignite close` for a regression ticket that still holds a
//! check the hub has switched off.
//!
//! Upkeep switches off a check that holds the main branch back for an hour with no record of
//! catching a real defect. A switched-off check runs in no gate and no tip round, so a landing
//! that left it broken passed its gate, closed the ticket, and the check's next failure, once
//! someone switched it on, reopened that ticket while another worker was already fixing the
//! same check under a follow-up ticket. A landing or close is therefore refused while the
//! ticket holds such a check, until the worker moves it to the ticket that holds the rest of the
//! work. A justiciar can pass `--allow-off-checks`.
//!
//! A landing's gate runs its ticket's own switched-off checks (see `crate::checks::plan`), and
//! the hub switches each one the gate passed back on when the landing is pushed. Workers once
//! had to switch such a check on themselves before landing, and on 2026-11-28 tip rounds at tips
//! that did not have the fix yet failed it again and held the main branch back for up to an
//! hour. So `ignite land` refuses only for a check its gate did not run and pass, such as one
//! that is not in the tree's listing.

use std::collections::BTreeSet;

use ignite_core::ids::{CheckName, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::client::{Exit, Reply};

use crate::checks::CheckRun;
use crate::checks::verbs::render;
use crate::server::Context;

/// The flag that lets a landing or close go ahead with switched-off checks on its ticket.
pub const ALLOW: &str = "--allow-off-checks";

/// The checks whose regression `ticket` holds, still unfixed, that the hub has switched off.
/// Empty when the hub cannot be reached: the landing's own hub calls report that.
pub fn on_ticket(ctx: &Context, ticket: &TicketId) -> Vec<CheckName> {
    ctx.hub
        .open_regressions()
        .unwrap_or_default()
        .into_iter()
        .filter(|open| open.off && &open.ticket == ticket)
        .map(|open| open.check)
        .collect()
}

/// The switched-off checks that a landing's ticket holds, and those of them that the
/// landing's gate ran and passed.
#[derive(Debug, Default)]
pub struct Held {
    checks: Vec<CheckName>,
    passed: BTreeSet<CheckName>,
}

impl Held {
    pub fn new(checks: Vec<CheckName>) -> Self {
        Self {
            checks,
            passed: BTreeSet::new(),
        }
    }

    /// Notes each held check that `run`, a gate that passed, ran and passed. A landing notes
    /// every pass, because a later pass does not run again a family it carries over.
    pub fn note(&mut self, run: &CheckRun) {
        for (check, outcome) in &run.outcomes {
            if matches!(outcome, Outcome::Pass | Outcome::Flaky) && self.checks.contains(check) {
                self.passed.insert(check.clone());
            }
        }
    }

    /// Whether the ticket holds any switched-off check.
    pub fn any(&self) -> bool {
        !self.checks.is_empty()
    }

    /// The held checks for the hub to switch on once the landing of `ticket` is pushed, or the
    /// refusal when the gate did not run and pass one of them.
    pub fn switch_on(&self, ticket: &TicketId) -> Result<Vec<CheckName>, Box<Reply>> {
        let unpassed: Vec<CheckName> = self
            .checks
            .iter()
            .filter(|check| !self.passed.contains(*check))
            .cloned()
            .collect();
        match refusal("land", ticket, &unpassed) {
            Some(refused) => Err(Box::new(refused)),
            None => Ok(self.checks.clone()),
        }
    }
}

/// The refusal of `verb` (`land` or `close`) on `ticket` while it holds `checks`, which are
/// switched off and which, for `land`, its gate did not run and pass; `None` when it holds none.
pub fn refusal(verb: &str, ticket: &TicketId, checks: &[CheckName]) -> Option<Reply> {
    if checks.is_empty() {
        return None;
    }
    let names: Vec<&str> = checks.iter().map(CheckName::as_str).collect();
    let (why, fixed) = if verb == "land" {
        (
            "The gate did not run and pass them, so",
            "the gate runs a switched-off check this ticket holds, and the landing switches it \
             on; a check the gate did not run is not in this tree's listing",
        )
    } else {
        (
            "No gate runs them, so",
            "for a check this change fixes, run `ignite flaky on <check> -m \"<what you \
             changed>\"`",
        )
    };
    let lines = [
        (
            "refused".to_owned(),
            format!(
                "{ticket} holds checks that are switched off and have not passed since they \
                 failed: {}. {why} closing {ticket} now would leave them failing, and their \
                 next failure would reopen it.",
                names.join(", ")
            ),
        ),
        ("fixed".to_owned(), fixed.to_owned()),
        (
            "unfixed".to_owned(),
            "move each check you leave failing to the ticket that holds the rest of its work \
             with `ignite ticket regression move <check> <ticket>`"
                .to_owned(),
        ),
        (
            "next".to_owned(),
            format!("then run ignite {verb} again; a justiciar may pass {ALLOW}"),
        ),
    ];
    Some(Reply::text(Exit::CheckFailed, render(&lines)))
}

/// Takes [`ALLOW`] out of `args`, saying whether it was there.
pub fn take_allow(args: &mut Vec<String>) -> bool {
    let before = args.len();
    args.retain(|arg| arg != ALLOW);
    args.len() != before
}
