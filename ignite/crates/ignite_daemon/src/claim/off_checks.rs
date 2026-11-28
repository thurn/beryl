//! The refusal of `ignite land` and `ignite close` for a regression ticket that still holds a
//! check the hub has switched off.
//!
//! Upkeep switches off a check that holds the main branch back for an hour with no record of
//! catching a real defect. A switched-off check runs in no gate and no tip round, so a landing
//! that left it broken passed its gate, closed the ticket, and the check's next failure, once
//! someone switched it on, reopened that ticket while another worker was already fixing the
//! same check under a follow-up ticket. A landing or close is therefore refused while the
//! ticket holds such a check, until the worker switches it on (so the gate runs it) or moves it
//! to the ticket that holds the rest of the work. A justiciar can pass `--allow-off-checks`.

use ignite_core::ids::{CheckName, TicketId};
use ignite_core::protocol::client::{Exit, Reply};

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

/// The refusal of `verb` (`land` or `close`) on `ticket` while it holds `checks`, which are
/// switched off; `None` when it holds none.
pub fn refusal(verb: &str, ticket: &TicketId, checks: &[CheckName]) -> Option<Reply> {
    if checks.is_empty() {
        return None;
    }
    let names: Vec<&str> = checks.iter().map(CheckName::as_str).collect();
    let lines = [
        (
            "refused".to_owned(),
            format!(
                "{ticket} holds checks that are switched off and have not passed since they \
                 failed: {}. No gate runs them, so closing {ticket} now would leave them \
                 failing, and their next failure would reopen it.",
                names.join(", ")
            ),
        ),
        (
            "fixed".to_owned(),
            "for a check this change fixes, run `ignite flaky on <check> -m \"<what you \
             changed>\"`, so the gate runs it"
                .to_owned(),
        ),
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
