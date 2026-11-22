//! A tell reaches only the session that held the ticket when the tell was made.

use super::tests::{Rig, detail, lock, rig, ticket_id};
use super::*;
use ignite_core::protocol::tickets::Status;

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
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
}

#[test]
fn a_release_does_not_send_a_tell_from_before_the_claim() {
    let rig = rig(&["cli-claude-00000001"]);
    rig.hub.insert_ticket(claimed(
        "T.1",
        "cli-claude-00000001",
        &[OLD_ORDER],
        CLAIMED_LATER,
    ));
    rig.hold("cli-claude-00000001", "T.1");
    let fake = super::cli_tests::cli(&rig, 1);
    super::cli_tests::tick(&rig);
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    // The hub takes the ticket back, but the daemon still lists the session as the holder.
    let mut released = claimed("T.1", "cli-claude-00000001", &[OLD_ORDER], CLAIMED_LATER);
    released.ticket.status = Status::Open;
    released.ticket.assignee = None;
    released.ticket.session = None;
    released.ticket.claimed_at = None;
    rig.hub.insert_ticket(released);
    change(&rig, "T.1");
    assert!(fake.sent().is_empty());
}

#[test]
fn headless_dispatch_does_not_send_an_old_tell_after_restart() {
    // A restart clears the in-memory record of sent tells.
    let rig = rig(&["cli-claude-00000001"]);
    rig.hub.insert_ticket(claimed(
        "T.1",
        "cli-claude-00000001",
        &[OLD_ORDER],
        CLAIMED_LATER,
    ));
    rig.hold("cli-claude-00000001", "T.1");
    let fake = super::cli_tests::cli(&rig, 1);
    super::cli_tests::tick(&rig);
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    change(&rig, "T.1");
    assert!(fake.sent().is_empty());
}

/// A Claude CLI session that holds T.1, which it claimed long before any tell, with the
/// daemon caught up on the hub feed.
fn cli_holder(rig: &Rig) -> (std::sync::Arc<crate::clid::FakeCliPool>, &'static str) {
    use super::cli_tests::{cli, reserve, tick};
    let fake = cli(rig, 1);
    reserve(rig, "T.1", "reservation-1", &[]);
    tick(rig);
    let worker = "cli-claude-00000001";
    rig.hold(worker, "T.1");
    rig.hub
        .insert_ticket(claimed("T.1", worker, &[], "1970-01-01T00:00:00.000Z"));
    tick(rig);
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    (fake, worker)
}

#[test]
fn tell_now_ends_a_cli_workers_turn_and_a_plain_tell_waits() {
    let rig = rig(&[]);
    let (fake, worker) = cli_holder(&rig);
    let reply = rig.tell(&["--now", "T.1", "Stop and release."]);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("running turn ends now"),
        "{}",
        reply.text
    );
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].session.as_str(), worker);
    assert_eq!(sent[0].text, "Message about T.1: Stop and release.");
    assert!(sent[0].urgent);
    // The CLI pool ends the session's wait when it queues the message.
    assert!(lock(&rig.host.interrupts).is_empty());

    assert_eq!(rig.tell(&["T.1", "Also this."]).exit, Exit::Success);
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 2);
    assert!(!sent[1].urgent);
}

#[test]
fn every_tell_from_a_justiciar_is_urgent_for_a_cli_worker() {
    let rig = rig(&[]);
    let (fake, _) = cli_holder(&rig);
    rig.hub.insert_ticket(detail(
        "J.1",
        "in_progress",
        Some("local_j1"),
        &["justiciar"],
        "",
    ));
    let reply = rig.tell_as("local_j1", &["T.1", "Release it."]);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].text,
        "Message about T.1: from local_j1: Release it."
    );
    assert!(sent[0].urgent);
}

#[test]
fn tell_now_to_a_desktop_session_is_refused() {
    let rig = rig(&["local_w1"]);
    rig.hub
        .insert_ticket(claimed("T.1", "local_w1", &[], "1970-01-01T00:00:00.000Z"));
    rig.hold("local_w1", "T.1");
    let reply = rig.tell(&["--now", "T.1", "Stop."]);
    assert_eq!(reply.exit, Exit::Usage);
    assert_eq!(
        reply.text,
        "refused  `--now` works only for CLI workers, and local_w1 is a desktop session"
    );
    assert!(rig.hub.batches().is_empty(), "nothing was written");
    assert_eq!(rig.tell(&["--all", "--now", "Stop."]).exit, Exit::Usage);
    assert!(rig.hub.batches().is_empty());
}
