//! What follows the end of a CLI worker's turn: the rules of "How a turn ends" in the design
//! document (`docs/ignite.md`, "CLI mode"), as one pure function.
//!
//! The daemon reads a turn's events, asks the app's [`CliApp::classify`](super::CliApp) how
//! the turn ended, gathers the [`Facts`] around it and carries out the [`Action`] that
//! [`decide`] returns. The rules, in the order they are tried:
//!
//! 1. A turn can only end while its slot is starting or running. An end in any other state is
//!    a combination the rules do not name and parks the slot.
//! 2. An [`TurnEnd::Unknown`] end parks the slot. An end the daemon cannot place proves nothing,
//!    so it never releases a ticket and never starts a turn.
//! 3. A [`TurnEnd::Hold`] end is not the session's fault. A single API error first backs off for
//!    [`BACK_OFF_SECONDS`]; a second one in a row, an API error while the app is already on a
//!    hold, and every other reason put the app on a CLI hold with the slot parked.
//! 4. A clean, interrupted or plan-limit end after which the session no longer holds its ticket
//!    frees the slot: the ticket was landed, released or closed.
//! 5. A clean end with the ticket held waits for the session's running `ignite` verb, else
//!    resumes with the waiting messages, else resumes once with [`block_text`]. A second clean
//!    end after the block text releases the ticket, with the worker's last message as the note.
//! 6. An interrupted end resumes at once with [`INTERRUPTED_TEXT`], until the
//!    [`INTERRUPTED_LIMIT`]th in a row with no `ignite` command between them, which releases the
//!    ticket. The daemon sends the justiciar report that goes with that release.
//! 7. A plan-limit end releases the ticket. The daemon records the window's 100% sample.
//!
//! Two guards then apply to what the rules chose:
//!
//! - A release never runs while a `land` of the session has begun its push, since releasing
//!   resets the tree under the push. The slot waits for the land instead, or parks when no land
//!   is listed as running.
//! - No turn starts while the app is on a CLI hold, so a resume or a back-off parks the slot.
//!   When the hold ends, the daemon resumes every parked slot.

use super::{Action, Facts, HoldReason, PreviousAnswer, Slot, SlotState, TurnEnd, Verb};
use crate::ids::TicketId;

/// How long a single API error waits before the turn is tried again.
pub const BACK_OFF_SECONDS: f64 = 30.0;

/// The interrupted end, counted in a row with no `ignite` command between them, that releases
/// the ticket instead of resuming.
pub const INTERRUPTED_LIMIT: u32 = 5;

/// The longest part of the worker's last message kept in a release note.
pub const NOTE_CHARS: usize = 2000;

/// The prompt of the turn that follows an interrupted one.
pub const INTERRUPTED_TEXT: &str = "Your process was interrupted. Your last tool call may not \
                                    have finished. Run `ignite status` and continue.";

/// The prompt of the turn that follows a single API error's back-off.
pub const API_ERROR_TEXT: &str = "Your last turn ended on an API error and was started again \
                                  after a pause. Run `ignite status` and continue.";

/// What follows the end of `slot`'s latest turn, by the rules of "How a turn ends" in the
/// design document. No I/O and no clock but `facts.now`. An end or a combination the rules do
/// not name returns [`Action::Park`], never a release or a start.
pub fn decide(slot: &Slot, end: &TurnEnd, facts: &Facts) -> Action {
    let chosen = by_the_rules(slot, end, facts);
    let chosen = guard_push(chosen, facts);
    guard_hold(chosen, facts)
}

/// The rules before the guards.
fn by_the_rules(slot: &Slot, end: &TurnEnd, facts: &Facts) -> Action {
    if !matches!(slot.state, SlotState::Starting | SlotState::Running) {
        return Action::Park {
            report: format!(
                "{}: a turn ended ({}) while the slot was {}, where no turn runs",
                slot_name(slot),
                end_name(end),
                slot.state
            ),
        };
    }
    match end {
        TurnEnd::Unknown { why } => Action::Park {
            report: format!(
                "{}: the turn's end could not be placed: {why}",
                slot_name(slot)
            ),
        },
        TurnEnd::Hold { reason, message } => hold(*reason, message, facts),
        _ if !facts.ticket_held => Action::Free,
        TurnEnd::Clean => clean(slot, facts),
        TurnEnd::Interrupted => interrupted(facts),
        TurnEnd::PlanLimit { window } => Action::Release {
            note: quoting(
                &match window {
                    Some(window) => format!("The plan's limit was reached in `{window}`."),
                    None => "The plan's limit was reached.".to_owned(),
                },
                facts.last_text.as_deref(),
            ),
        },
    }
}

/// A failure that is not the session's: a back-off for a single API error, otherwise a hold.
fn hold(reason: HoldReason, message: &str, facts: &Facts) -> Action {
    let first_api_error = reason == HoldReason::ApiError
        && facts.previous != PreviousAnswer::BackedOff
        && facts.hold.is_none();
    if first_api_error {
        Action::BackOff {
            at: facts.now.plus_seconds(BACK_OFF_SECONDS),
            prompt: with_messages(API_ERROR_TEXT, &facts.messages),
        }
    } else {
        Action::Hold {
            reason,
            message: message.to_owned(),
        }
    }
}

/// A clean end with the ticket held.
fn clean(slot: &Slot, facts: &Facts) -> Action {
    if let Some(verb) = facts.running_verbs.first() {
        return wait_for(verb);
    }
    if !facts.messages.is_empty() {
        return Action::Resume {
            prompt: facts.messages.join("\n\n"),
        };
    }
    if facts.previous == PreviousAnswer::BlockSent {
        return Action::Release {
            note: release_reason(facts.last_text.as_deref()),
        };
    }
    match &slot.ticket {
        Some(ticket) => Action::Resume {
            prompt: block_text(ticket),
        },
        None => Action::Park {
            report: format!(
                "{}: the hub says the session holds a ticket, but the slot names none",
                slot_name(slot)
            ),
        },
    }
}

/// An interrupted end with the ticket held.
fn interrupted(facts: &Facts) -> Action {
    if facts.interrupted_in_row >= INTERRUPTED_LIMIT {
        return Action::Release {
            note: quoting(
                &format!(
                    "The worker's turn was interrupted {} times in a row with no `ignite` \
                     command between them.",
                    facts.interrupted_in_row
                ),
                facts.last_text.as_deref(),
            ),
        };
    }
    Action::Resume {
        prompt: with_messages(INTERRUPTED_TEXT, &facts.messages),
    }
}

/// Waiting for `verb`, and the prompt that tells the worker it finished.
fn wait_for(verb: &Verb) -> Action {
    Action::WaitForVerb {
        handle: verb.handle.clone(),
        prompt: format!(
            "`{} {}` finished: run `ignite wait {}`.",
            verb.command, verb.handle, verb.handle
        ),
    }
}

/// A release turned into a wait while a `land` of the session has begun its push.
fn guard_push(chosen: Action, facts: &Facts) -> Action {
    if !facts.land_pushing || !matches!(chosen, Action::Release { .. }) {
        return chosen;
    }
    match facts
        .running_verbs
        .iter()
        .find(|verb| verb.command.split_whitespace().next() == Some("land"))
    {
        Some(land) => wait_for(land),
        None => Action::Park {
            report: "a land of the session has begun its push but is not listed as running, so \
                     the ticket was not released"
                .to_owned(),
        },
    }
}

/// A turn that would start now parked instead while the app is on a CLI hold.
fn guard_hold(chosen: Action, facts: &Facts) -> Action {
    let Some(hold) = &facts.hold else {
        return chosen;
    };
    if !matches!(chosen, Action::Resume { .. } | Action::BackOff { .. }) {
        return chosen;
    }
    Action::Park {
        report: format!(
            "{} is on a CLI hold ({}) since {}, so no turn starts: {}",
            hold.app,
            hold.reason.as_str(),
            hold.since,
            hold.message
        ),
    }
}

/// `text`, followed by every waiting message in order.
fn with_messages(text: &str, messages: &[String]) -> String {
    let mut prompt = text.to_owned();
    for message in messages {
        prompt.push_str("\n\n");
        prompt.push_str(message);
    }
    prompt
}

/// `cli-<app>-slot-<index>`, the slot's name in reports.
fn slot_name(slot: &Slot) -> String {
    format!("cli-{}-slot-{}", slot.app, slot.index)
}

/// The end's kind, for reports.
fn end_name(end: &TurnEnd) -> &'static str {
    match end {
        TurnEnd::Clean => "clean",
        TurnEnd::Interrupted => "interrupted",
        TurnEnd::PlanLimit { .. } => "plan limit",
        TurnEnd::Hold { .. } => "hold",
        TurnEnd::Unknown { .. } => "unknown",
    }
}

/// The text that keeps a worker going after its first stop with `ticket` held. `ignite
/// turn-ended` replies with it to the desktop app's `Stop` hook, and [`decide`] resumes a CLI
/// worker with it.
pub fn block_text(ticket: &TicketId) -> String {
    format!(
        "You are ending your turn while you hold {ticket}, and nothing will report on \
         it. Before you stop, do one of these:\n\
         1. If the work is done, run `ignite land -m <note>`.\n\
         2. If it is not, run `ignite release -m <exact state>`, with the dependency when it \
         waits on another ticket (see `ignite guide release`).\n\
         3. If the ticket cannot be completed as written (its acceptance contradicts its body, \
         or gates on something you cannot control, such as a timing on a loaded machine), run \
         `ignite justiciar \"<what is impossible, and the evidence>\"` and then `ignite \
         release`.\n\
         To wait on a long command, start it in the background (heavy work as `ignite run -- \
         <command>`) and end your turn: while anything you started still runs, Ignite lets you \
         stop and you are woken when it ends. A turn that ends with nothing running is final: \
         only a final turn takes a ✅, ⛔ or 🔍 title, and a turn that ends to wait keeps ⚒️ (or \
         🚒), since a ⛔ title releases the ticket at once (see `ignite guide turn-end`).\n\
         A figure measured on a loaded machine is reported, never gated on or chased, unless \
         the acceptance names a deterministic measure.\n\
         If you stop again with nothing running and none of these done, Ignite releases {ticket} \
         itself, saves its tree's work on the remote and uses your last message as the note."
    )
}

/// The reason given with a release after the second stop, quoting the worker's last message.
pub fn release_reason(note: Option<&str>) -> String {
    quoting(
        "The worker ended its turn twice holding the ticket without landing or releasing it.",
        note,
    )
}

/// `head`, followed by the first [`NOTE_CHARS`] characters of `note` when there is one.
pub fn quoting(head: &str, note: Option<&str>) -> String {
    match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(note) => format!("{head} Its last message: {}", cut(note, NOTE_CHARS)),
        None => head.to_owned(),
    }
}

/// The first `chars` characters of `text`, with `…` when anything was cut.
pub fn cut(text: &str, chars: usize) -> String {
    let mut kept: String = text.chars().take(chars).collect();
    if kept.len() < text.len() {
        kept.push('…');
    }
    kept
}

#[cfg(test)]
#[path = "decide_tests.rs"]
mod tests;
