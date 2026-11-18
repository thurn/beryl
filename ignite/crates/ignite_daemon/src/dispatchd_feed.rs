//! What the dispatcher does with a change in the hub's feed: delivering an `ignite tell`, and
//! finding the culprit of a regression ticket.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{DispatchAction, Exit, Reply};
use ignite_core::protocol::tickets::TicketDetail;

use super::{Dispatcher, Purpose, State};

/// Sends a new `tell` on `detail`'s ticket to `holder`, the session working it here: as a
/// `message` instruction, and as the reply that ends its `ignite wait` if it is in one. A tell
/// made before the session claimed the ticket is already in its brief and is not sent.
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
    if ticket
        .claimed_at
        .is_some_and(|claimed| comment.at < claimed)
    {
        return;
    }
    let text = format!("Message about {}: {}", ticket.id, comment.text);
    Dispatcher::queue(
        state,
        Some(holder),
        DispatchAction::Message,
        &text,
        Purpose::Message,
    );
    dispatcher
        .host
        .interrupt(holder, Reply::text(Exit::Success, text));
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
