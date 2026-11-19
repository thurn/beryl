use serde_json::json;

use ignite_core::ids::TreeId;
use ignite_core::protocol::client::Exit;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::local::{rows, trees as tree_rows};

use super::fakes::{World, id, session, ticket};
use super::{StartVerb, lock};
use crate::server::Verb;

const WORKER: &str = "local_worker";
const TREE: &str = "/trees/tree-01";

fn started() -> World {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world);
    world
}

fn start(world: &World) {
    let req = world.request("start", &["Tq.48"], "/repo", WORKER);
    let reply = StartVerb(world.claims.clone()).run(&world.ctx, &req);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
}

/// The `changed` lines one command from the tree prints.
fn command(world: &World) -> Vec<String> {
    let req = world.request("test", &["x"], TREE, WORKER);
    world.claims.changed(&world.ctx, &req)
}

fn minutes(world: &World, minutes: f64) {
    world.clock.advance(minutes * 60.0);
    world.claims.tick(&world.ctx);
}

#[test]
fn a_command_resets_the_silence_clock() {
    let world = started();
    let start = world.clock.now();
    assert_eq!(world.claims.quiet_since(&session(WORKER)), Some(start));
    minutes(&world, 40.0);
    assert_eq!(world.claims.quiet_since(&session(WORKER)), Some(start));
    command(&world);
    assert_eq!(
        world.claims.quiet_since(&session(WORKER)),
        Some(world.clock.now())
    );
}

#[test]
fn a_long_command_keeps_the_silence_clock_moving() {
    let world = started();
    lock(&world.commands.running).push(session(WORKER));
    minutes(&world, 40.0);
    assert_eq!(
        world.claims.quiet_since(&session(WORKER)),
        Some(world.clock.now())
    );
    assert!(world.claims.quiet_since(&session("local_other")).is_none());
}

#[test]
fn another_authors_comment_shows_once_and_the_sessions_own_never() {
    let world = started();
    world.desk.comment(
        "Tq.48",
        "derek",
        "Do not wait for multivac.\nLand when the gate passes.",
    );
    world
        .desk
        .comment("Tq.48", WORKER, "Noting my own progress.");
    world.hub.push_change("ticket", Some(id("Tq.48")));
    let lines = command(&world);
    assert_eq!(
        lines,
        [
            "changed  comment by derek: Do not wait for multivac.",
            "         Land when the gate passes."
        ]
    );
    assert!(command(&world).is_empty());
    // An edit to the acceptance is named, and the next brief is read again.
    world.desk.edit(
        "Tq.48",
        "derek",
        json!({"acceptance": ["a", "b"], "updated_at": [1, 2]}),
    );
    world
        .desk
        .edit("Tq.48", WORKER, json!({"title": ["a", "b"]}));
    world.hub.push_change("ticket", Some(id("Tq.48")));
    let lines = command(&world);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("changed  edit by derek: acceptance"),
        "{lines:?}"
    );
}

#[test]
fn a_told_comment_is_recorded_as_a_message_and_not_printed_as_its_text() {
    let world = started();
    world.desk.comment(
        "Tq.48",
        "local_teller",
        "Stop your landing.\nRun ignite land again.",
    );
    // `ignite tell` marks its comment for delivery in the ticket's metadata.
    world.desk.edit(
        "Tq.48",
        "local_teller",
        json!({"metadata": [{}, {"tell": 1}]}),
    );
    world.desk.comment("Tq.48", "derek", "An ordinary note.");
    world.hub.push_change("ticket", Some(id("Tq.48")));
    assert_eq!(
        command(&world),
        [
            "changed  comment: message from local_teller, delivered by the dispatcher, \
             starting \"Stop your landing.\"",
            "changed  comment by derek: An ordinary note."
        ]
    );
}

#[test]
fn start_shows_nothing_as_changed_and_the_hub_is_read_only_after_the_feed() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    world
        .desk
        .comment("Tq.48", "derek", "Read the pool code first.");
    start(&world);
    let reads = *lock(&world.desk.detail_reads);
    assert!(command(&world).is_empty());
    assert!(command(&world).is_empty());
    assert_eq!(
        *lock(&world.desk.detail_reads),
        reads,
        "no change in the feed, no read"
    );
    // A change to another ticket does not read this one.
    world.hub.push_change("ticket", Some(id("Tq.77")));
    assert!(command(&world).is_empty());
    assert_eq!(*lock(&world.desk.detail_reads), reads);
}

#[test]
fn more_than_five_changes_are_counted() {
    let world = started();
    for n in 0..7 {
        world.desk.comment("Tq.48", "derek", &format!("note {n}"));
    }
    world.hub.push_change("ticket", Some(id("Tq.48")));
    let lines = command(&world);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[5], "changed  and 2 more: ignite ticket show Tq.48");
}

/// What the trees module records in the database when it leases `tree-01` to the worker.
fn record_lease(conn: &rusqlite::Connection) {
    tree_rows::upsert(
        conn,
        &rows::Tree {
            id: "tree-01".to_owned(),
            path: TREE.to_owned(),
            kind: "ticket".to_owned(),
            ticket: Some("Tq.48".to_owned()),
            session: Some(WORKER.to_owned()),
            branch: Some("ignite/Tq.48".to_owned()),
            built_commit: None,
            leased_at: None,
            last_used: None,
            owner: None,
            state: tree_rows::State::Leased.as_str().to_owned(),
        },
    )
    .unwrap();
}

#[test]
fn a_restarted_daemon_does_not_repeat_a_change() {
    let path = std::env::temp_dir().join(format!("ignite-claim-restart-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let world = World::with_db(ignite_store::local::open(&path).unwrap());
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world);
    let conn = ignite_store::local::open(&path).unwrap();
    record_lease(&conn);
    world.desk.comment("Tq.48", "derek", "First correction.");
    world.hub.push_change("ticket", Some(id("Tq.48")));
    assert_eq!(command(&world).len(), 1);

    let restarted = World::with_db(ignite_store::local::open(&path).unwrap());
    // The same tickets on the hub; a fresh daemon in front of them.
    *lock(&restarted.desk.details) = lock(&world.desk.details).clone();
    let holder = restarted.claims.holder(TREE.as_ref()).unwrap();
    assert_eq!(holder.0, TreeId::new("tree-01").unwrap());
    assert!(
        command(&restarted).is_empty(),
        "the comment was shown before the restart"
    );
    restarted
        .desk
        .comment("Tq.48", "derek", "Second correction.");
    restarted.hub.push_change("ticket", Some(id("Tq.48")));
    assert_eq!(
        command(&restarted),
        ["changed  comment by derek: Second correction."]
    );
    drop(conn);
    let _ = std::fs::remove_file(&path);
}

/// Starts a daemon over a database file, as a restart does, with its clock at `minutes` after
/// the first daemon's start.
fn restarted_at(path: &std::path::Path, minutes: f64) -> World {
    let now = Timestamp::EPOCH.plus_seconds(minutes * 60.0);
    World::with_db_at(ignite_store::local::open(path).unwrap(), now)
}

#[test]
fn a_restart_keeps_how_long_a_session_has_been_quiet() {
    let path = std::env::temp_dir().join(format!("ignite-claim-quiet-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let world = World::with_db(ignite_store::local::open(&path).unwrap());
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world);
    record_lease(&ignite_store::local::open(&path).unwrap());
    let last_command = world.clock.now();
    minutes(&world, 20.0);

    // The daemon restarts 20 minutes after the session's last command. Before the clock was
    // saved, the restarted daemon counted the 30 minutes from its own start.
    let restarted = restarted_at(&path, 20.0);
    assert!(restarted.claims.holder(TREE.as_ref()).is_some());
    minutes(&restarted, 9.0);
    assert_eq!(
        restarted.claims.quiet_since(&session(WORKER)),
        Some(last_command)
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_freed_tree_leaves_no_clock_behind() {
    let path = std::env::temp_dir().join(format!("ignite-claim-freed-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let world = World::with_db(ignite_store::local::open(&path).unwrap());
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world);
    let conn = ignite_store::local::open(&path).unwrap();
    assert_eq!(ignite_store::local::activity::all(&conn).unwrap().len(), 1);
    world
        .claims
        .release_session(
            &world.ctx,
            &session(WORKER),
            &id("Tq.48"),
            "The session is gone.",
        )
        .unwrap();
    minutes(&world, 1.0);
    assert!(world.claims.holder(TREE.as_ref()).is_none());
    assert!(world.claims.quiet_since(&session(WORKER)).is_none());
    assert!(
        ignite_store::local::activity::all(&conn)
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_failed_save_keeps_the_ticket_held() {
    let world = started();
    *lock(&world.trees.save_fails) = true;
    let released = world.claims.release_session(
        &world.ctx,
        &session(WORKER),
        &id("Tq.48"),
        "The session is gone.",
    );
    assert!(released.is_err());
    assert!(lock(&world.desk.releases).is_empty());
    assert!(world.claims.holder(TREE.as_ref()).is_some());
}
