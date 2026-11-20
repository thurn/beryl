//! What the dispatcher does with a change in the hub's feed: delivering an `ignite tell`, and
//! finding the culprit of a regression ticket and the prompt that hands the regression to it.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{DispatchAction, Exit, Reply};
use ignite_core::protocol::tickets::{Status, TicketDetail};

use super::{Dispatcher, Purpose, Reserved, State, cli, start_arguments, tell};

/// Sends a new `tell` on `detail`'s ticket to `holder`, the session working it here, as a
/// `message` instruction. If the session is inside `ignite wait`, the wait ends with a reply that
/// says a message is waiting but does not carry its text: a wait's reply is tool output, which
/// agents rightly treat as data rather than instructions, while the dispatcher's message reaches
/// them as a teammate's request once their turn ends.
///
/// A CLI session gets the text through the CLI pool instead, which ends its wait itself, and
/// ends its running turn when the tell is urgent: when the ticket's `tell_now` key holds the
/// same comment number as `tell`.
///
/// Nothing is sent unless the ticket is in progress and assigned to `holder`, and its claim is
/// not newer than the comment. A tell made before the session claimed the ticket is already in
/// its brief, and a tell on a ticket that was just released was meant for whoever held it
/// before. The dispatcher still lists `holder` for a moment after a release, and a ticket's
/// `tell` number stays on it for good, so without these tests a restarted dispatcher (which has
/// forgotten which tells it sent) would send an old order to a session it was never meant for.
pub(super) fn deliver_tell(
    dispatcher: &Dispatcher,
    state: &mut State,
    holder: &SessionId,
    detail: &TicketDetail,
) {
    let ticket = &detail.ticket;
    let Some(tell) = ticket
        .metadata
        .get("tell")
        .and_then(serde_json::Value::as_u64)
    else {
        return;
    };
    if tell <= state.tells.get(&ticket.id).copied().unwrap_or(0) {
        return;
    }
    state.tells.insert(ticket.id.clone(), tell);
    let Some(comment) = detail
        .comments
        .iter()
        .flatten()
        .find(|c| u64::from(c.n) == tell)
    else {
        return;
    };
    let held_here =
        ticket.status == Status::InProgress && ticket.assignee.as_deref() == Some(holder.as_str());
    let claimed_before = ticket
        .claimed_at
        .is_some_and(|claimed| comment.at >= claimed);
    if !held_here || !claimed_before {
        return;
    }
    let text = format!("Message about {}: {}", ticket.id, comment.text);
    if cli::is_cli_session(holder) {
        let urgent = ticket
            .metadata
            .get(tell::NOW_KEY)
            .and_then(serde_json::Value::as_u64)
            == Some(tell);
        state.cli_calls.push(cli::Call::Message {
            session: holder.clone(),
            text,
            urgent,
        });
        return;
    }
    Dispatcher::queue(
        state,
        Some(holder),
        DispatchAction::Message,
        &text,
        Purpose::Message,
    );
    let notice = format!(
        "{} has a message for you from the dispatcher. End your turn to receive it.",
        ticket.id
    );
    dispatcher
        .host
        .interrupt(holder, Reply::text(Exit::Success, notice));
}

/// The ticket whose worker should take `detail` because it holds the change's context: for a
/// ticket whose landing was reverted, the ticket itself; for a regression, the ticket on its
/// `caused-by` line. `None` for any other ticket.
pub(super) fn cause(detail: &TicketDetail) -> Option<TicketId> {
    if detail.ticket.metadata.contains_key("reverted") {
        Some(detail.ticket.id.clone())
    } else if detail.ticket.labels.iter().any(|l| l == "regression") {
        caused_by(&detail.ticket.body)
    } else {
        None
    }
}

/// The hand-off prompt of `reserved`, a regression, for the worker whose last ticket caused it.
pub(super) fn regression_prompt(reserved: &Reserved, title: &str) -> String {
    format!(
        "{} is a regression caused by your last ticket: {title}. Run `ignite start {}` and fix \
         it.",
        reserved.ticket,
        start_arguments(reserved)
    )
}

/// The hand-off prompt of `reserved`, a ticket whose landing a tip round reverted, for the
/// worker that landed it. It names the checks that did not build.
pub(super) fn reverted_prompt(detail: &TicketDetail, reserved: &Reserved) -> String {
    let checks = detail
        .ticket
        .metadata
        .get("reverted")
        .and_then(|m| m.get("checks"))
        .and_then(serde_json::Value::as_array)
        .map(|checks| {
            checks
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!(
        "your landing of {} was reverted: the tip did not build with it ({checks}). Run `ignite \
         start {}`",
        reserved.ticket,
        start_arguments(reserved)
    )
}

/// The prompt that hands `detail` to the worker whose ticket caused it: the reverted ticket's
/// or the regression's.
pub(super) fn culprit_prompt(detail: &TicketDetail, reserved: &Reserved) -> String {
    if detail.ticket.metadata.contains_key("reverted") {
        reverted_prompt(detail, reserved)
    } else {
        regression_prompt(reserved, &detail.ticket.title)
    }
}

/// The ticket named on a regression's `caused-by <ticket>` line.
pub(super) fn caused_by(body: &str) -> Option<TicketId> {
    body.lines()
        .find_map(|line| line.strip_prefix("caused-by "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|id| TicketId::new(id).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_culprit_is_the_ticket_on_the_caused_by_line() {
        let body = "Regression: x\ncaused-by Tw8gv.21.73 (commit 5e1b0aa)\nfirst bad 5e1b0aa";
        assert_eq!(caused_by(body).unwrap().as_str(), "Tw8gv.21.73");
        assert_eq!(caused_by("first bad 5e1b0aa"), None);
    }
}
