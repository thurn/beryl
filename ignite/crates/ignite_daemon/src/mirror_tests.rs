//! The ticket mirror against the fake hub: the first copy, following the feed, a hub restored
//! from an older snapshot, and a restore that rolls tickets back.

use ignite_core::fakes::{FakeClock, FakeHub};
use ignite_core::protocol::tickets::History;
use serde_json::json;

use super::restore;
use super::*;

const START: &str = "2026-11-18T10:00:00.000Z";

fn whole(id: &str, title: &str) -> WholeTicket {
    serde_json::from_value(json!({
        "id": id, "parent": null, "next_child": 1, "title": title, "type": "task",
        "status": "open", "priority": 2, "assignee": null, "defer_until": null,
        "close_reason": null, "created_at": START, "created_by": "test", "updated_at": START,
        "closed_at": null, "version": 0,
    }))
    .unwrap()
}

/// `whole` edited by `actor` at `at`, with the history row the hub would add.
fn edited(id: &str, title: &str, actor: &str, at: Timestamp) -> WholeTicket {
    let mut ticket = whole(id, title);
    ticket.ticket.updated_at = at;
    ticket.history.push(History {
        ticket: ticket.ticket.id.clone(),
        seq: Seq(0),
        actor: actor.to_owned(),
        at,
        change: json!({"title": ["old", title]}),
    });
    ticket
}

struct Rig {
    hub: FakeHub,
    clock: Arc<FakeClock>,
    mirror: Mirror,
}

impl Rig {
    fn new() -> Rig {
        let clock = Arc::new(FakeClock::new(Timestamp::parse(START).unwrap()));
        Rig {
            hub: FakeHub::default(),
            mirror: Mirror::in_memory(clock.clone()).unwrap(),
            clock,
        }
    }

    fn round(&mut self) -> Progress {
        self.mirror.round(&self.hub, 0).unwrap()
    }

    /// Rounds until a copy under way has finished; the versions stored.
    fn copy(&mut self) -> usize {
        let mut stored = 0;
        loop {
            let progress = self.round();
            stored += progress.stored;
            if !progress.copying {
                return stored;
            }
        }
    }

    fn status(&self) -> Status {
        status(self.mirror.connection()).unwrap()
    }

    fn titles(&self, id: &str) -> Vec<String> {
        let mut statement = self
            .mirror
            .connection()
            .prepare("SELECT whole_json FROM versions WHERE ticket = ?1 ORDER BY id")
            .unwrap();
        statement
            .query_map([id], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|json| {
                serde_json::from_str::<WholeTicket>(&json.unwrap())
                    .unwrap()
                    .ticket
                    .title
            })
            .collect()
    }
}

#[test]
fn the_first_copy_pages_through_every_ticket_then_follows_the_feed() {
    let mut rig = Rig::new();
    for n in 0..250 {
        rig.hub.put_whole(whole(&format!("T.{n:03}"), "A"));
    }
    assert_eq!(rig.copy(), 250);
    let status = rig.status();
    assert_eq!(status.feed, Some(Seq(250)));
    assert_eq!((status.versions, status.tickets), (250, 250));
    assert_eq!(status.copying, None);

    // The changes the copy already holds are read from the feed and add nothing.
    assert_eq!(rig.round().stored, 0);
    assert!(rig.status().caught_up_at.is_some());

    rig.clock.advance(5.0);
    rig.hub.put_whole(whole("T.007", "B"));
    rig.hub.put_whole(whole("T.300", "new"));
    assert_eq!(rig.round().stored, 2);
    assert_eq!(rig.titles("T.007"), ["A", "B"]);

    // A change that leaves the content as it was (only `version` moves) adds no version.
    rig.hub.put_whole(whole("T.007", "B"));
    assert_eq!(rig.round().stored, 0);
    assert_eq!(rig.status().feed, Some(Seq(253)));
    assert_eq!(rig.status().versions, 252);
}

#[test]
fn a_change_made_during_the_first_copy_is_read_from_the_feed_afterwards() {
    let mut rig = Rig::new();
    rig.hub.put_whole(whole("T.1", "A"));
    // The copy notes the feed position first, then pages.
    assert!(rig.round().copying);
    rig.hub.put_whole(whole("T.2", "made during the copy"));
    rig.copy();
    rig.hub.put_whole(whole("T.1", "B"));
    rig.round();
    assert_eq!(rig.titles("T.1"), ["A", "B"]);
    assert_eq!(rig.titles("T.2"), ["made during the copy"]);
}

#[test]
fn a_hub_restored_from_an_older_snapshot_is_copied_again_keeping_every_version() {
    let mut rig = Rig::new();
    rig.hub.put_whole(whole("T.1", "A"));
    rig.hub.put_whole(whole("T.2", "A"));
    rig.copy();
    rig.hub.put_whole(whole("T.1", "B"));
    rig.round();
    assert_eq!(rig.status().feed, Some(Seq(3)));

    // The hub goes back to a snapshot in which T.1 still reads "A", and its feed restarts lower.
    rig.hub.put_whole(whole("T.1", "A"));
    rig.hub.restart_feed(1);
    assert!(rig.round().copying, "a feed that went back starts a copy");
    assert_eq!(rig.copy(), 1, "only the ticket that differs gets a version");
    assert_eq!(rig.titles("T.1"), ["A", "B", "A"]);
    assert_eq!(rig.titles("T.2"), ["A"]);
    assert_eq!(rig.status().feed, Some(Seq(1)));

    // It follows the restored hub from its new position.
    rig.hub.put_whole(whole("T.2", "C"));
    assert_eq!(rig.round().stored, 1);
}

#[test]
fn a_restore_at_a_moment_rolls_back_what_was_changed_after_it() {
    let mut rig = Rig::new();
    let start = Timestamp::parse(START).unwrap();
    rig.hub.put_whole(whole("T.1", "good"));
    rig.hub.put_whole(whole("T.2", "good"));
    rig.copy();
    let before = start.plus_seconds(30.0);

    rig.clock.advance(60.0);
    let later = rig.clock.now();
    rig.hub
        .put_whole(edited("T.1", "vandalized", "rogue", later));
    rig.hub.put_whole(edited("T.2", "improved", "alice", later));
    rig.hub.put_whole(edited("T.3", "spam", "rogue", later));
    rig.round();

    let plan = restore::plan(rig.mirror.connection(), Some(before), Some("rogue")).unwrap();
    let ids: Vec<&str> = plan.tickets.iter().map(|w| w.ticket.id.as_str()).collect();
    assert_eq!(ids, ["T.1"], "only the rogue's tickets");
    assert!(plan.force);
    assert_eq!(plan.created_later, [TicketId::new("T.3").unwrap()]);

    let imported = restore::send(&plan, |request| rig.hub.import(request)).unwrap();
    assert_eq!(imported, 1);
    let title = |id: &str| {
        rig.hub
            .whole(&TicketId::new(id).unwrap())
            .unwrap()
            .ticket
            .title
    };
    assert_eq!(
        title("T.1"),
        "good",
        "the forced import replaced a later copy"
    );
    assert_eq!(title("T.2"), "improved");
    assert!(rig.hub.imports().iter().all(|request| request.force));

    // Without an actor, every ticket goes back.
    let everyone = restore::plan(rig.mirror.connection(), Some(before), None).unwrap();
    assert_eq!(everyone.tickets.len(), 2);
}

#[test]
fn a_restore_without_a_moment_sends_the_newest_versions_parents_first() {
    let mut rig = Rig::new();
    let mut child = whole("T.1", "child");
    child.ticket.parent = Some(TicketId::new("T.9").unwrap());
    rig.hub.put_whole(child);
    rig.hub.put_whole(whole("T.9", "parent"));
    rig.copy();
    let plan = restore::plan(rig.mirror.connection(), None, None).unwrap();
    let ids: Vec<&str> = plan.tickets.iter().map(|w| w.ticket.id.as_str()).collect();
    assert_eq!(ids, ["T.9", "T.1"]);
    assert!(!plan.force);
    assert!(restore::plan(rig.mirror.connection(), None, Some("rogue")).is_err());
}
