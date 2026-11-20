//! A tell reaches only the session that held the ticket when the tell was made.

use super::tests::{Rig, detail, lock, rig, ticket_id};
use super::*;

/// An in-progress ticket held by `holder` since `claimed_at`, with `comments` as
/// `(n, written_at, text)` and its `tell` metadata set to the last comment's number.
pub(super) fn claimed(
    id: &str,
    holder: &str,
    comments: &[(u32, &str, &str)],
    claimed_at: &str,
) -> TicketDetail {
    let mut ticket = detail(id, "in_progress", Some(holder), &[], "");
    ticket.ticket.claimed_at = Some(claimed_at.parse().unwrap());
    if let Some((n, _, _)) = comments.last() {
        ticket
            .ticket
            .metadata
            .insert("tell".to_owned(), (*n).into());
    }
    ticket.comments = Some(
        comments
            .iter()
            .map(|(n, at, text)| {
                serde_json::from_value(serde_json::json!({
                    "ticket": id, "n": n, "author": "test", "at": at, "text": text,
                }))
                .unwrap()
            })
            .collect(),
    );
    ticket
}

const OLD_ORDER: (u32, &str, &str) = (4, "2027-01-01T00:00:00.000Z", "Stop work now.");
const CLAIMED_LATER: &str = "2027-01-02T00:00:00.000Z";

fn change(rig: &Rig, ticket: &str) {
    rig.hub.push_change("ticket", Some(ticket_id(ticket)));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
}

fn nothing_sent(rig: &Rig) {
    assert!(rig.step().is_none());
    assert!(lock(&rig.host.interrupts).is_empty());
}

#[test]
fn a_release_does_not_send_a_tell_from_before_the_claim() {
    let rig = rig(&["local_w1"]);
    rig.hub
        .insert_ticket(claimed("T.1", "local_w1", &[OLD_ORDER], CLAIMED_LATER));
    rig.hold("local_w1", "T.1");
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    // The hub takes the ticket back, but the dispatcher still lists the session as the holder.
    let mut released = claimed("T.1", "local_w1", &[OLD_ORDER], CLAIMED_LATER);
    released.ticket.status = Status::Open;
    released.ticket.assignee = None;
    released.ticket.session = None;
    released.ticket.claimed_at = None;
    rig.hub.insert_ticket(released);
    change(&rig, "T.1");
    nothing_sent(&rig);
}

#[test]
fn a_restarted_dispatcher_does_not_send_a_tell_from_before_the_claim() {
    // A fresh dispatcher has an empty record of the tells it sent.
    let rig = rig(&["local_w1"]);
    rig.hub
        .insert_ticket(claimed("T.1", "local_w1", &[OLD_ORDER], CLAIMED_LATER));
    rig.hold("local_w1", "T.1");
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    change(&rig, "T.1");
    nothing_sent(&rig);
}
